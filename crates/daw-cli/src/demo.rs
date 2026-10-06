//! A four-bar demo groove using the default project's Keys, Bass, and Drums
//! tracks. Used to check the instruments by ear and in tests.

use daw_engine::EngineMessage;
use daw_engine::offline::{TimedMessage, render_project};
use daw_model::Project;

const KEYS: u32 = 1;
const BASS: u32 = 2;
const DRUMS: u32 = 3;
const BEAT_S: f64 = 0.5; // 120 BPM

fn note(
    track_id: u32,
    note: u8,
    velocity: f32,
    start_beats: f64,
    len_beats: f64,
) -> [TimedMessage; 2] {
    [
        TimedMessage {
            at_seconds: start_beats * BEAT_S,
            message: EngineMessage::NoteOn {
                track_id,
                note,
                velocity,
            },
        },
        TimedMessage {
            at_seconds: (start_beats + len_beats) * BEAT_S,
            message: EngineMessage::NoteOff { track_id, note },
        },
    ]
}

/// Renders the demo as interleaved stereo.
pub fn render(sample_rate_hz: u32) -> Vec<f32> {
    // Am – F – C – G, one chord per bar.
    let chords: [[u8; 3]; 4] = [[57, 60, 64], [57, 60, 65], [55, 60, 64], [55, 59, 62]];
    let roots: [u8; 4] = [45, 41, 48, 43];
    let mut events = Vec::new();
    for (bar, (chord, root)) in chords.iter().zip(roots).enumerate() {
        let start = bar as f64 * 4.0;
        for &n in chord {
            events.extend(note(KEYS, n, 0.7, start, 3.75));
        }
        for (i, offset) in [0.0, 1.5, 2.0, 3.0, 3.5].iter().enumerate() {
            let octave = if i == 3 { 12 } else { 0 };
            events.extend(note(BASS, root - 12 + octave, 0.9, start + offset, 0.4));
        }
        for eighth in 0..8 {
            let t = start + f64::from(eighth) * 0.5;
            events.extend(note(
                DRUMS,
                42,
                if eighth % 2 == 0 { 0.8 } else { 0.5 },
                t,
                0.1,
            ));
        }
        for kick in [0.0, 1.5, 2.0] {
            events.extend(note(DRUMS, 36, 1.0, start + kick, 0.1));
        }
        for snare in [1.0, 3.0] {
            events.extend(note(DRUMS, 38, 0.9, start + snare, 0.1));
        }
    }
    events.extend(note(DRUMS, 49, 0.9, 16.0, 0.1));
    events.extend(note(DRUMS, 36, 1.0, 16.0, 0.1));
    events.extend(note(KEYS, 57, 0.7, 16.0, 2.0));
    events.extend(note(BASS, 33, 0.9, 16.0, 2.0));
    render_project(
        &Project::default(),
        events.into_iter().collect(),
        20.0 * BEAT_S,
        sample_rate_hz,
    )
}
