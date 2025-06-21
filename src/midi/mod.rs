use anyhow::{anyhow, Result};
use std::ffi::CStr;
use windows::Win32::Media::Audio::{
    midiOutClose, midiOutGetDevCapsA, midiOutGetNumDevs, midiOutOpen, midiOutShortMsg, 
    HMIDIOUT, MIDIOUTCAPSA, CALLBACK_NULL,
};
use log::{info, debug};

#[derive(Debug)]
pub struct MidiMessage {
    pub status: u8,
    pub data1: u8,
    pub data2: u8,
}

impl MidiMessage {
    pub fn to_midi_word(&self) -> u32 {
        (self.data2 as u32) << 16 | (self.data1 as u32) << 8 | (self.status as u32)
    }

    pub fn message_type(&self) -> &'static str {
        match self.status & 0xF0 {
            0x80 => "Note Off",
            0x90 => if self.data2 == 0 { "Note Off" } else { "Note On" },
            0xA0 => "Polyphonic Key Pressure",
            0xB0 => "Control Change",
            0xC0 => "Program Change",
            0xD0 => "Channel Pressure",
            0xE0 => "Pitch Bend",
            _ => "Unknown",
        }
    }

    pub fn note_name(&self) -> String {
        if (self.status & 0xF0) != 0x90 && (self.status & 0xF0) != 0x80 {
            return String::new(); // Not a note message
        }
        
        const NOTES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
        let note_number = self.data1;
        let octave = (note_number / 12) as i32 - 1; // MIDI note 60 is middle C (C4)
        let note = NOTES[(note_number % 12) as usize];
        format!("{}{}", note, octave)
    }

    pub fn velocity(&self) -> u8 {
        self.data2
    }
}

pub struct MidiOutput {
    handle: HMIDIOUT,
}

impl MidiOutput {
    pub fn list_devices() -> Result<Vec<(usize, String)>> {
        let mut devices = Vec::new();
        unsafe {
            let num_devices = midiOutGetNumDevs();
            for i in 0..num_devices {
                let mut caps = MIDIOUTCAPSA::default();
                let result = midiOutGetDevCapsA(i as usize, &mut caps, std::mem::size_of::<MIDIOUTCAPSA>() as u32);
                if result == 0 {
                    if let Ok(name) = CStr::from_ptr(caps.szPname.as_ptr() as *const i8).to_str() {
                        devices.push((i as usize, name.to_string()));
                    }
                }
            }
        }
        Ok(devices)
    }

    pub fn new_with_device_name(target_name: &str) -> Result<Self> {
        unsafe {
            let devices = Self::list_devices()?;
            info!("Available MIDI output devices:");
            for (idx, name) in &devices {
                info!("  {}: {}", idx, name);
            }

            let device_id = devices.iter()
                .find(|(_, name)| name.contains(target_name))
                .map(|(idx, _)| *idx)
                .ok_or_else(|| anyhow!("No MIDI output device found containing '{}'", target_name))?;

            let mut handle = HMIDIOUT::default();
            let result = midiOutOpen(
                &mut handle,
                device_id as u32,
                0,
                0,
                CALLBACK_NULL,
            );

            if result == 0 {
                info!("Successfully opened MIDI output device: {}", target_name);
                Ok(MidiOutput { handle })
            } else {
                Err(anyhow!("Failed to open MIDI output device, error code: {}", result))
            }
        }
    }

    pub fn send_message(&self, message: &MidiMessage) -> Result<()> {
        unsafe {
            let midi_word = message.to_midi_word();
            let result = midiOutShortMsg(self.handle, midi_word);
            
            if result == 0 {
                debug!("Sent MIDI message: {:08X}", midi_word);
                Ok(())
            } else {
                Err(anyhow!("Failed to send MIDI message, error code: {}", result))
            }
        }
    }
}

impl Drop for MidiOutput {
    fn drop(&mut self) {
        unsafe {
            let _ = midiOutClose(self.handle);
            info!("Closed MIDI output device");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_midi_message_to_midi_word() {
        let msg = MidiMessage {
            status: 0x90,  // Note On
            data1: 0x40,   // Note number (64)
            data2: 0x7F,   // Velocity (127)
        };
        assert_eq!(msg.to_midi_word(), 0x7F4090);
    }

    #[test]
    fn test_midi_message_type() {
        let test_cases = vec![
            (MidiMessage { status: 0x80, data1: 0, data2: 0 }, "Note Off"),
            (MidiMessage { status: 0x90, data1: 0, data2: 64 }, "Note On"),
            (MidiMessage { status: 0x90, data1: 0, data2: 0 }, "Note Off"), // Note On with velocity 0 is Note Off
            (MidiMessage { status: 0xA0, data1: 0, data2: 0 }, "Polyphonic Key Pressure"),
            (MidiMessage { status: 0xB0, data1: 0, data2: 0 }, "Control Change"),
            (MidiMessage { status: 0xC0, data1: 0, data2: 0 }, "Program Change"),
            (MidiMessage { status: 0xD0, data1: 0, data2: 0 }, "Channel Pressure"),
            (MidiMessage { status: 0xE0, data1: 0, data2: 0 }, "Pitch Bend"),
            (MidiMessage { status: 0xF0, data1: 0, data2: 0 }, "Unknown"),
        ];

        for (msg, expected) in test_cases {
            assert_eq!(msg.message_type(), expected);
        }
    }

    #[test]
    fn test_note_name() {
        let test_cases = vec![
            (MidiMessage { status: 0x90, data1: 60, data2: 64 }, "C4"),  // Middle C
            (MidiMessage { status: 0x90, data1: 61, data2: 64 }, "C#4"),
            (MidiMessage { status: 0x90, data1: 62, data2: 64 }, "D4"),
            (MidiMessage { status: 0x90, data1: 72, data2: 64 }, "C5"),
            (MidiMessage { status: 0x80, data1: 69, data2: 64 }, "A4"),
            // Test non-note message
            (MidiMessage { status: 0xB0, data1: 60, data2: 64 }, ""),
        ];

        for (msg, expected) in test_cases {
            assert_eq!(msg.note_name(), expected);
        }
    }

    #[test]
    fn test_velocity() {
        let msg = MidiMessage {
            status: 0x90,
            data1: 60,
            data2: 100,
        };
        assert_eq!(msg.velocity(), 100);
    }
}
