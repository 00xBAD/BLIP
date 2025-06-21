use anyhow::{anyhow, Result};
use btleplug::api::{Peripheral as _};
use futures::StreamExt;
use log::{debug, error, info};
use tokio::time;
use std::time::Duration;

use crate::ble::{BleDevice, BLE_MIDI_CHARACTERISTIC_UUID, BLE_MIDI_SERVICE_UUID};
use crate::midi::{MidiOutput, MidiMessage};

#[derive(Clone)]
pub struct Config {
    pub virtual_midi_port_name: String,
    pub ble_scan_timeout: Duration,
    pub ble_keepalive_interval: Duration,
    pub ble_status_check_interval: Duration,
    pub octave_offset: i8,
}

pub struct BleMidiBridge {
    ble_device: BleDevice,
    midi_output: MidiOutput,
    config: Config,
}

impl BleMidiBridge {
    pub async fn new(config: &Config) -> Result<Self> {
        let ble_device = BleDevice::discover(config.ble_scan_timeout).await?;
        
        // Try to connect to loopMIDI virtual port
        info!("Looking for MIDI port '{}'...", config.virtual_midi_port_name);
        let midi_output = match MidiOutput::new_with_device_name(&config.virtual_midi_port_name) {
            Ok(output) => output,
            Err(_) => {
                error!("Could not find MIDI port '{}'. Please create it in loopMIDI:", config.virtual_midi_port_name);
                error!("1. Download and install loopMIDI from: https://www.tobias-erichsen.de/software/loopmidi.html");
                error!("2. Run loopMIDI");
                error!("3. Click the '+' button to create a new virtual port");
                error!("4. Double click the port name and rename it to: {}", config.virtual_midi_port_name);
                error!("5. Run this program again");
                return Err(anyhow!("MIDI port '{}' not found", config.virtual_midi_port_name));
            }
        };        Ok(BleMidiBridge {
            ble_device,
            midi_output,
            config: config.clone(),
        })
    }

    pub async fn start(&self, config: &Config) -> Result<()> {
        // Find the BLE-MIDI service and characteristic
        let midi_service = self
            .ble_device
            .peripheral
            .services()
            .into_iter()
            .find(|s| s.uuid == BLE_MIDI_SERVICE_UUID)
            .ok_or_else(|| anyhow!("BLE-MIDI service not found"))?;

        let characteristic = midi_service
            .characteristics
            .into_iter()
            .find(|c| c.uuid == BLE_MIDI_CHARACTERISTIC_UUID)
            .ok_or_else(|| anyhow!("BLE-MIDI characteristic not found"))?;

        info!("Found BLE-MIDI service: {}", midi_service.uuid);
        info!("Found BLE-MIDI characteristic: {}", characteristic.uuid);

        // Subscribe to notifications
        self.ble_device.peripheral.subscribe(&characteristic).await?;
        info!("Subscribed to BLE-MIDI notifications");

        // Start keep-alive
        self.ble_device.start_keepalive(
            BLE_MIDI_CHARACTERISTIC_UUID,
            config.ble_keepalive_interval
        ).await;

        // Main processing loop
        let mut notifications = self.ble_device.peripheral.notifications().await?;
        let mut consecutive_errors = 0;
        
        loop {
            tokio::select! {
                Some(notification) = notifications.next() => {
                    if notification.uuid == BLE_MIDI_CHARACTERISTIC_UUID {
                        match self.process_ble_midi_packet(&notification.value).await {
                            Ok(_) => {
                                // Reset error counter on successful processing
                                consecutive_errors = 0;
                            }
                            Err(e) => {
                                consecutive_errors += 1;
                                error!("Error processing BLE-MIDI packet: {}", e);
                                
                                // If we get too many consecutive errors, propagate the error up
                                if consecutive_errors > 10 {
                                    return Err(anyhow!("Too many consecutive BLE-MIDI packet errors, last error: {}", e));
                                }
                            }
                        }
                    }
                }
                _ = time::sleep(config.ble_status_check_interval) => {
                    // Check connection status periodically
                    if !self.ble_device.peripheral.is_connected().await? {
                        error!("Device disconnected unexpectedly");
                        return Err(anyhow!("BLE device disconnected unexpectedly - please check if the device is turned on and within range"));
                    }
                }
            }
        }
    }    async fn process_ble_midi_packet(&self, data: &[u8]) -> Result<()> {
        if data.len() < 2 {
            return Err(anyhow!("BLE-MIDI packet too short"));
        }

        debug!("Received BLE-MIDI packet: {:02X?}", data);
        debug!("Packet length: {}", data.len());
        
        // Debug header byte
        debug!("Header byte: 0x{:02X}", data[0]);
        debug!("Timestamp byte: 0x{:02X}", data[1]);

        // In BLE-MIDI, each packet has the format: [header, timestamp, status, data1, data2]
        // The header and timestamp are BLE-specific, the actual MIDI message starts at index 2
        if data.len() >= 5 {
            let status = data[2];   // MIDI status byte
            let mut data1 = data[3]; // First MIDI data byte (note number)
            let data2 = data[4];    // Second MIDI data byte (velocity)

            // Apply octave transposition for Note On/Off messages
            let message_type = status & 0xF0;
            if message_type == 0x90 || message_type == 0x80 {
                let octave_shift = self.config.octave_offset * 12;
                let original_note = data1;
                let new_note = (data1 as i16 + octave_shift as i16).clamp(0, 127) as u8;
                data1 = new_note;
                  // Log transposition details only in debug mode
                debug!(
                    "Note transposition: {} ({}) -> {} ({}) [offset: {} octaves]",
                    MidiMessage { status, data1: original_note, data2 }.note_name(),
                    original_note,
                    MidiMessage { status, data1: new_note, data2 }.note_name(),
                    new_note,
                    self.config.octave_offset
                );
            }

            let message = MidiMessage { status, data1, data2 };
            let msg = if message.message_type() == "Note On" {
                format!(
                    "Note On: {} (velocity: {}) [status: {:02X}, note: {:02X}, velocity: {:02X}]",
                    message.note_name(),
                    message.velocity(),
                    message.status,
                    message.data1,
                    message.data2
                )
            } else if message.message_type() == "Note Off" {
                format!(
                    "Note Off: {} [status: {:02X}, note: {:02X}, velocity: {:02X}]",
                    message.note_name(),
                    message.status,
                    message.data1,
                    message.data2
                )
            } else {
                format!(
                    "MIDI Message: {} [status: {:02X}, data1: {:02X}, data2: {:02X}]",
                    message.message_type(),
                    message.status,
                    message.data1,
                    message.data2
                )
            };
            debug!("{}", msg);

            // Send the MIDI message
            self.midi_output.send_message(&message)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_config_creation() {
        let config = Config {
            virtual_midi_port_name: "TEST_PORT".to_string(),
            ble_scan_timeout: Duration::from_secs(30),
            ble_keepalive_interval: Duration::from_secs(10),
            ble_status_check_interval: Duration::from_secs(1),
            octave_offset: 1,
        };

        assert_eq!(config.virtual_midi_port_name, "TEST_PORT");
        assert_eq!(config.ble_scan_timeout, Duration::from_secs(30));
        assert_eq!(config.ble_keepalive_interval, Duration::from_secs(10));
        assert_eq!(config.ble_status_check_interval, Duration::from_secs(1));
        assert_eq!(config.octave_offset, 1);
    }

    // This test ensures the durations are positive and reasonable
    #[test]
    fn test_config_validation() {
        let config = Config {
            virtual_midi_port_name: "TEST_PORT".to_string(),
            ble_scan_timeout: Duration::from_secs(30),
            ble_keepalive_interval: Duration::from_secs(10),
            ble_status_check_interval: Duration::from_secs(1),
            octave_offset: 0,
        };

        assert!(config.ble_scan_timeout > Duration::from_secs(0));
        assert!(config.ble_keepalive_interval > Duration::from_secs(0));
        assert!(config.ble_status_check_interval > Duration::from_secs(0));
        
        // Check that keepalive interval is longer than status check interval
        assert!(config.ble_keepalive_interval > config.ble_status_check_interval);
        
        // Check octave offset range
        assert!(config.octave_offset >= -11 && config.octave_offset <= 11);
    }

    #[test]
    fn test_note_transposition() {
        // Test note transposition with different octave offsets
        let test_cases = vec![
            // (original_note, octave_offset, expected_note)
            (60, 1, 72),    // Middle C -> C5
            (60, -1, 48),   // Middle C -> C3
            (120, 1, 127),  // High note clamped to max
            (0, -1, 0),     // Low note clamped to min
            (60, 0, 60),    // No transposition
        ];

        for (original_note, octave_offset, expected_note) in test_cases {
            // Create a test MIDI packet
            let mut packet = vec![0x80, 0x80];  // Header and timestamp
            packet.extend_from_slice(&[0x90, original_note, 0x7F]); // Note On, note, velocity
            
            let config = Config {
                virtual_midi_port_name: "TEST_PORT".to_string(),
                ble_scan_timeout: Duration::from_secs(30),
                ble_keepalive_interval: Duration::from_secs(10),
                ble_status_check_interval: Duration::from_secs(1),
                octave_offset,
            };

            let message = MidiMessage {
                status: 0x90,
                data1: original_note,
                data2: 0x7F,
            };

            let transposed_note = ((original_note as i16) + ((octave_offset * 12) as i16))
                .clamp(0, 127) as u8;
            assert_eq!(transposed_note, expected_note);
        }
    }
}
