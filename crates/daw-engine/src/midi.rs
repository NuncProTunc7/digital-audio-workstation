//! MIDI keyboard input via midir (WinMM on Windows).

use std::sync::Arc;

use midir::{MidiInput, MidiInputConnection};

/// The MIDI messages the app cares about. Channel is 0–15.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MidiEvent {
    NoteOn {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    NoteOff {
        channel: u8,
        note: u8,
    },
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
    /// -8192..=8191, 0 is centered.
    PitchBend {
        channel: u8,
        value: i16,
    },
}

/// Parses one MIDI message. Returns `None` for messages we ignore (clock,
/// sysex, aftertouch, program change).
pub fn parse(bytes: &[u8]) -> Option<MidiEvent> {
    let (&status, data) = bytes.split_first()?;
    let channel = status & 0x0F;
    match (status & 0xF0, data) {
        // Note-on with velocity 0 is a note-off by convention.
        (0x90, &[note, 0, ..]) | (0x80, &[note, _, ..]) => Some(MidiEvent::NoteOff {
            channel,
            note: note & 0x7F,
        }),
        (0x90, &[note, velocity, ..]) => Some(MidiEvent::NoteOn {
            channel,
            note: note & 0x7F,
            velocity: velocity & 0x7F,
        }),
        (0xB0, &[controller, value, ..]) => Some(MidiEvent::ControlChange {
            channel,
            controller: controller & 0x7F,
            value: value & 0x7F,
        }),
        (0xE0, &[lsb, msb, ..]) => {
            let raw = (i16::from(msb & 0x7F) << 7) | i16::from(lsb & 0x7F);
            Some(MidiEvent::PitchBend {
                channel,
                value: raw - 8192,
            })
        }
        _ => None,
    }
}

/// Open connections to every MIDI input port. Dropping it disconnects.
pub struct MidiInputs {
    connections: Vec<MidiInputConnection<()>>,
    port_names: Vec<String>,
}

impl MidiInputs {
    /// Connects to every available input port. Ports that fail to open are
    /// skipped (another app may have them exclusively on Windows).
    pub fn connect_all(on_event: Arc<dyn Fn(MidiEvent) + Send + Sync>) -> Self {
        let mut connections = Vec::new();
        let mut port_names = Vec::new();
        let port_count = MidiInput::new("Nunc Pro Tune")
            .map(|m| m.ports().len())
            .unwrap_or(0);
        for index in 0..port_count {
            let Ok(input) = MidiInput::new("Nunc Pro Tune") else {
                continue;
            };
            let ports = input.ports();
            let Some(port) = ports.get(index) else {
                continue;
            };
            let name = input
                .port_name(port)
                .unwrap_or_else(|_| format!("MIDI input {}", index + 1));
            let callback = Arc::clone(&on_event);
            if let Ok(conn) = input.connect(
                port,
                "nunc-pro-tune-in",
                move |_timestamp, bytes, _| {
                    if let Some(event) = parse(bytes) {
                        callback(event);
                    }
                },
                (),
            ) {
                connections.push(conn);
                port_names.push(name);
            }
        }
        Self {
            connections,
            port_names,
        }
    }

    pub fn port_names(&self) -> &[String] {
        &self.port_names
    }

    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_note_messages() {
        assert_eq!(
            parse(&[0x90, 60, 100]),
            Some(MidiEvent::NoteOn {
                channel: 0,
                note: 60,
                velocity: 100
            })
        );
        assert_eq!(
            parse(&[0x93, 60, 0]),
            Some(MidiEvent::NoteOff {
                channel: 3,
                note: 60
            })
        );
        assert_eq!(
            parse(&[0x80, 61, 64]),
            Some(MidiEvent::NoteOff {
                channel: 0,
                note: 61
            })
        );
    }

    #[test]
    fn parses_controllers_and_pitch_bend() {
        assert_eq!(
            parse(&[0xB0, 64, 127]),
            Some(MidiEvent::ControlChange {
                channel: 0,
                controller: 64,
                value: 127
            })
        );
        assert_eq!(
            parse(&[0xE0, 0, 64]),
            Some(MidiEvent::PitchBend {
                channel: 0,
                value: 0
            })
        );
        assert_eq!(
            parse(&[0xE0, 0, 0]),
            Some(MidiEvent::PitchBend {
                channel: 0,
                value: -8192
            })
        );
        assert_eq!(
            parse(&[0xE0, 127, 127]),
            Some(MidiEvent::PitchBend {
                channel: 0,
                value: 8191
            })
        );
    }

    #[test]
    fn ignores_other_and_truncated_messages() {
        assert_eq!(parse(&[]), None);
        assert_eq!(parse(&[0xF8]), None);
        assert_eq!(parse(&[0x90, 60]), None);
        assert_eq!(parse(&[0xC0, 5]), None);
    }
}
