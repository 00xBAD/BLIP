use anyhow::{anyhow, Result};
use btleplug::api::{
    Central, Manager as _, Peripheral as _, ScanFilter,
};
use btleplug::platform::{Manager, Peripheral};
use log::{info, warn, debug};
use std::time::Duration;
use tokio::time;
use uuid::Uuid;

// BLE-MIDI protocol UUIDs
pub const BLE_MIDI_CHARACTERISTIC_UUID: Uuid = Uuid::from_u128(0x7772E5DB_3868_4112_A1A9_F2669D106BF3);
pub const BLE_MIDI_SERVICE_UUID: Uuid = Uuid::from_u128(0x03B80E5A_EDE8_4B33_A751_6CE34EC4C700);

pub struct BleDevice {
    pub peripheral: Peripheral,
}

impl BleDevice {
    pub async fn discover(scan_timeout: Duration) -> Result<Self> {
        let manager = Manager::new().await?;
        let adapters = manager.adapters().await?;
        
        if adapters.is_empty() {
            return Err(anyhow!("No Bluetooth adapters found"));
        }

        let central = &adapters[0];
        info!("Using Bluetooth adapter: {}", central.adapter_info().await?);

        // Start scanning
        info!("Scanning for BLE devices...");
        central.start_scan(ScanFilter::default()).await?;

        let start_time = std::time::Instant::now();

        // Poll for devices every second until we find our target or timeout
        let mut found_peripheral = None;
        while start_time.elapsed() < scan_timeout {
            let peripherals = central.peripherals().await?;
            for peripheral in peripherals {
                if let Ok(Some(properties)) = peripheral.properties().await {
                    if let Some(name) = properties.local_name {
                        info!("Found device: {}", name);
                        if name.contains("LPK25") || name.contains("AKAI") {
                            info!("Found target device: {}", name);
                            found_peripheral = Some(peripheral);
                            break;
                        }
                    }
                }
            }

            if found_peripheral.is_some() {
                break;
            }

            // Wait a short time before checking again
            time::sleep(Duration::from_millis(1000)).await;
        }

        // Stop scanning
        central.stop_scan().await?;

        let peripheral = found_peripheral
            .ok_or_else(|| anyhow!("Could not find LPK25 or AKAI device within {} seconds", scan_timeout.as_secs()))?;

        // Connect to device
        info!("Connecting to device...");
        peripheral.connect().await?;
        info!("Connected successfully");

        // Discover services and characteristics
        info!("Discovering services...");
        peripheral.discover_services().await?;
        
        // List all services and characteristics for debugging
        for service in peripheral.services() {
            info!("Found service: {}", service.uuid);
            for characteristic in service.characteristics {
                info!("  Characteristic: {} (properties: {:?})", characteristic.uuid, characteristic.properties);
            }
        }

        Ok(BleDevice { peripheral })
    }

    pub async fn start_keepalive(&self, characteristic_uuid: Uuid, interval: Duration) {
        let peripheral_clone = self.peripheral.clone();
        let characteristic = self.get_characteristic(characteristic_uuid).await
            .expect("Characteristic should exist");

        tokio::spawn(async move {
            let mut interval = time::interval(interval);
            loop {
                interval.tick().await;
                if let Err(e) = peripheral_clone.read(&characteristic).await {
                    warn!("Keep-alive read failed: {}", e);
                } else {
                    debug!("Keep-alive ping successful");
                }
            }
        });
    }

    pub async fn get_characteristic(&self, uuid: Uuid) -> Result<btleplug::api::Characteristic> {
        for service in self.peripheral.services() {
            for characteristic in service.characteristics {
                if characteristic.uuid == uuid {
                    return Ok(characteristic);
                }
            }
        }
        Err(anyhow!("Characteristic not found: {}", uuid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use futures::stream;
    use tokio::sync::Mutex;

    // Mock types for testing
    #[derive(Clone)]
    struct MockPeripheral {
        name: String,
        is_connected: Arc<Mutex<bool>>,
    }

    impl MockPeripheral {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                is_connected: Arc::new(Mutex::new(false)),
            }
        }

        async fn mock_connect(&self) -> Result<()> {
            let mut connected = self.is_connected.lock().await;
            *connected = true;
            Ok(())
        }

        async fn mock_is_connected(&self) -> Result<bool> {
            Ok(*self.is_connected.lock().await)
        }
    }

    #[tokio::test]
    async fn test_device_connection() {
        let mock_peripheral = MockPeripheral::new("AKAI LPK25");
        
        // Test connection
        mock_peripheral.mock_connect().await.unwrap();
        assert!(mock_peripheral.mock_is_connected().await.unwrap());
    }

    #[test]
    fn test_ble_uuids() {
        // Test that our UUIDs are correctly defined
        assert_eq!(
            BLE_MIDI_SERVICE_UUID,
            Uuid::from_u128(0x03B80E5A_EDE8_4B33_A751_6CE34EC4C700)
        );
        assert_eq!(
            BLE_MIDI_CHARACTERISTIC_UUID,
            Uuid::from_u128(0x7772E5DB_3868_4112_A1A9_F2669D106BF3)
        );
    }
}
