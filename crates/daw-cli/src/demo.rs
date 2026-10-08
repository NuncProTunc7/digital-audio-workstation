//! A four-bar demo song on the default project's Keys, Bass, and Drums
//! tracks, built with the same Commands the UI and Claude use.

use daw_model::{Command, EffectKind, NoteInput, Project, Session};

const KEYS: u32 = 1;
const BASS: u32 = 2;
const DRUMS: u32 = 3;

fn n(pitch: u8, start: f64, len: f64, velocity: u8) -> NoteInput {
    NoteInput {
        chance: 100,
        pitch,
        start_beats: start,
        length_beats: len,
        velocity,
        id: None,
    }
}

/// The demo project: Am–F–C–G with a bass line, a beat, and some effects.
pub fn project() -> Project {
    let mut s = Session::default();
    let mut run = |c: Command| {
        if let Err(e) = s.execute(c) {
            unreachable!("demo command failed: {e}");
        }
    };
    run(Command::RenameProject {
        name: "Demo Groove".into(),
    });

    let chords: [[u8; 3]; 4] = [[57, 60, 64], [57, 60, 65], [55, 60, 64], [55, 59, 62]];
    let roots: [u8; 4] = [45, 41, 48, 43];
    let mut keys = Vec::new();
    let mut bass = Vec::new();
    let mut drums = Vec::new();
    for (bar, (chord, root)) in chords.iter().zip(roots).enumerate() {
        let start = bar as f64 * 4.0;
        for &p in chord {
            keys.push(n(p, start, 3.75, 90));
        }
        for (i, offset) in [0.0, 1.5, 2.0, 3.0, 3.5].iter().enumerate() {
            let octave = if i == 3 { 12 } else { 0 };
            bass.push(n(root - 12 + octave, start + offset, 0.4, 115));
        }
        for eighth in 0..8 {
            let t = start + f64::from(eighth) * 0.5;
            drums.push(n(42, t, 0.1, if eighth % 2 == 0 { 100 } else { 64 }));
        }
        for kick in [0.0, 1.5, 2.0] {
            drums.push(n(36, start + kick, 0.1, 127));
        }
        for snare in [1.0, 3.0] {
            drums.push(n(38, start + snare, 0.1, 115));
        }
    }
    drums.push(n(49, 0.0, 0.1, 110));
    for (track_id, name, notes) in [
        (KEYS, "Chords", keys),
        (BASS, "Bass line", bass),
        (DRUMS, "Beat", drums),
    ] {
        run(Command::CreateClip {
            track_id,
            start_beats: 0.0,
            length_beats: 16.0,
            name: Some(name.into()),
            notes,
        });
    }
    run(Command::AddEffect {
        track_id: Some(KEYS),
        kind: EffectKind::Reverb,
        index: None,
    });
    run(Command::AddEffect {
        track_id: None,
        kind: EffectKind::Limiter,
        index: None,
    });
    run(Command::SetLoop {
        enabled: Some(true),
        start_beats: Some(0.0),
        end_beats: Some(16.0),
    });
    s.project().clone()
}
