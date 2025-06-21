use anyhow::Result;
use log::{info, error};
use std::time::Duration;
use blip::{BleMidiBridge, Config};

//-----------------------------------------------------------------------------
// USER CONFIGURATION
// You can safely modify these values to customize the bridge behavior
//-----------------------------------------------------------------------------

// Set the loopMIDI virtual port name
// This must match the name of the virtual port created in loopMIDI
const VIRTUAL_MIDI_PORT_NAME: &str = "AKAI_LPK25_IN_BLE";

// BLE device scan timeout
const BLE_SCAN_TIMEOUT_SECS: u64 = 30;

// Connection keepalive interval
const BLE_KEEPALIVE_SECS: u64 = 10;

// Connection status check interval
const BLE_STATUS_CHECK_SECS: u64 = 1;

// Octave offset for transposing MIDI notes (-11 to +11 octaves)
const OCTAVE_OFFSET: i8 = 0;

//-----------------------------------------------------------------------------
// MAIN FUNCTION
// This is the entry point of the application
// Don't modify this unless you know what you're doing
//-----------------------------------------------------------------------------

fn display_logo() {
    println!(r#"
    ██████╗ ██╗     ██╗██████╗ 
    ██╔══██╗██║     ██║██╔══██╗
    ██████╔╝██║     ██║██████╔╝
    ██╔══██╗██║     ██║██╔═══╝ 
    ██████╔╝███████╗██║██║     
    ╚═════╝ ╚══════╝╚═╝╚═╝     
                                
    BLE LPK25 INTERFACE PROGRAM
    Version: 1.0.0
    For more information, visit: https://github.com/00xBAD/BLIP

    ---------------------------------------------------------------------------

    This program bridges the AKAI LPK25 BLE MIDI controller,
    to a virtual MIDI port using loopMIDI via the BLE-MIDI protocol.

    ---------------------------------------------------------------------------

    Be sure to have loopMIDI running and a virtual port named:
    "{VIRTUAL_MIDI_PORT_NAME}" created before starting this program.

    If you don't have loopMIDI installed, you can get it here:
    https://www.tobias-erichsen.de/software/loopmidi.html

    ---------------------------------------------------------------------------
    "#);
}

#[tokio::main]
async fn main() -> Result<()> {
    // Set different default log levels for debug and release builds
    let mut builder = env_logger::Builder::new();
    
    if cfg!(debug_assertions) {
        // Debug build: show all debug logs
        builder.filter_level(log::LevelFilter::Debug);
    } else {
        // Release build: show only info and above, and filter debug noise
        builder.filter_level(log::LevelFilter::Info)
               .filter_module("btleplug", log::LevelFilter::Warn)
               .filter_module("ble_midi_bridge", log::LevelFilter::Info);
    }

    builder.init();

    display_logo();
    info!("Starting BLE-MIDI Bridge for AKAI LPK25");
    if cfg!(debug_assertions) {
        info!("Running in debug mode - detailed logging enabled");
    }
    info!("Press Ctrl+C to exit");

    // Create configuration
    let config = Config {
        virtual_midi_port_name: VIRTUAL_MIDI_PORT_NAME.to_string(),
        ble_scan_timeout: Duration::from_secs(BLE_SCAN_TIMEOUT_SECS),
        ble_keepalive_interval: Duration::from_secs(BLE_KEEPALIVE_SECS),
        ble_status_check_interval: Duration::from_secs(BLE_STATUS_CHECK_SECS),
        octave_offset: OCTAVE_OFFSET,
    };

    // Create bridge instance
    let bridge_result = BleMidiBridge::new(&config).await;
    if let Err(ref e) = bridge_result {
        error!("Failed to create bridge: {}", e);
        info!("Press Ctrl+C to exit...");
    }
    
    let bridge = match bridge_result {
        Ok(b) => b,
        Err(_) => {
            // Wait for Ctrl+C before exiting on error
            tokio::signal::ctrl_c().await?;
            return Ok(());
        }
    };
    
    // Handle Ctrl+C gracefully
    let ctrl_c = tokio::signal::ctrl_c();
    
    tokio::select! {
        result = bridge.start(&config) => {
            match result {
                Ok(_) => info!("Bridge stopped normally"),
                Err(e) => {
                    error!("Bridge error: {}", e);
                    info!("Press Ctrl+C to exit...");
                    // Wait for Ctrl+C before exiting on bridge error
                    tokio::signal::ctrl_c().await?;
                }
            }
        }
        _ = ctrl_c => {
            info!("Received Ctrl+C, shutting down...");
        }
    }

    Ok(())
}