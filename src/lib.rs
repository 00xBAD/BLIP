pub mod ble;
pub mod midi;
pub mod bridge;

// Re-export main types for convenience
pub use bridge::{BleMidiBridge, Config};
