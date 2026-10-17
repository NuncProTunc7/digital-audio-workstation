use super::*;
use crate::NoteInput;
use crate::instrument::InstrumentKind;
use crate::music::ChordQuality;
use crate::project::{AudioRegion, AutomationTarget, Clip};
use crate::session::Session;

const KEYS: TrackId = 1;

fn note(pitch: u8, start: f64) -> NoteInput {
    NoteInput {
        chance: 100,
        pitch,
        start_beats: start,
        length_beats: 0.5,
        velocity: 100,
        id: None,
    }
}

/// Default song (120 BPM, 4/4) with an 8-beat Keys clip (notes in each
/// bar), a 16-beat audio clip of a 10-second file, markers Intro (0) and
/// Chorus (6), chords C (0) and G (6), and a Keys volume ramp
/// (0, -12) → (8, 0). Returns it with the audio track id.
fn song() -> (Project, TrackId) {
    let mut p = Project::default();
    let commands = vec![
        Command::CreateClip {
            track_id: KEYS,
            start_beats: 0.0,
            length_beats: 8.0,
            name: Some("Keys".into()),
            notes: vec![note(60, 0.0), note(62, 2.0), note(64, 4.0), note(65, 7.0)],
        },
        Command::AddTrack {
            name: "Vox".into(),
            instrument: InstrumentKind::Audio,
            preset: None,
            index: None,
        },
        Command::AddMarker {
            name: "Intro".into(),
            start_beats: 0.0,
        },
        Command::AddMarker {
            name: "Chorus".into(),
            start_beats: 6.0,
        },
        Command::AddChord {
            start_beats: 0.0,
            root: 0,
            quality: ChordQuality::Major,
            bass: None,
        },
        Command::AddChord {
            start_beats: 6.0,
            root: 7,
            quality: ChordQuality::Major,
            bass: None,
        },
        Command::AddAutomationLane {
            track_id: KEYS,
            target: AutomationTarget::Volume,
            points: vec![point(0.0, -12.0), point(8.0, 0.0)],
        },
    ];
    for c in commands {
        c.apply(&mut p).expect("setup");
    }
    let vox = p.tracks.last().expect("vox").id;
    Command::AddAudioClip {
        track_id: vox,
        start_beats: 0.0,
        audio: AudioRegion {
            file: "vox.wav".into(),
            file_seconds: 10.0,
            offset_seconds: 0.0,
            gain_db: 0.0,
            fade_in_seconds: 0.0,
            fade_out_seconds: 0.0,
            source_bpm: None,
        },
        length_beats: Some(16.0),
        name: None,
    }
    .apply(&mut p)
    .expect("audio");
    (p, vox)
}

fn clips(p: &Project, track: TrackId) -> Vec<Clip> {
    p.track(track).expect("track").clips.clone()
}

fn spans(p: &Project, track: TrackId) -> Vec<(f64, f64)> {
    clips(p, track)
        .iter()
        .map(|c| (c.start_beats, c.length_beats))
        .collect()
}

fn pitches(c: &Clip) -> Vec<(u8, f64)> {
    c.notes.iter().map(|n| (n.pitch, n.start_beats)).collect()
}

fn offsets(p: &Project, track: TrackId) -> Vec<f64> {
    clips(p, track)
        .iter()
        .map(|c| c.audio.as_ref().expect("audio").offset_seconds)
        .collect()
}

fn markers(p: &Project) -> Vec<(String, f64)> {
    p.markers
        .iter()
        .map(|m| (m.name.clone(), m.start_beats))
        .collect()
}

fn chords(p: &Project) -> Vec<(u8, f64)> {
    p.chords.iter().map(|c| (c.root, c.start_beats)).collect()
}

fn lane(p: &Project) -> Vec<(f64, f64)> {
    p.track(KEYS).expect("keys").automation[0]
        .points
        .iter()
        .map(|p| (p.beats, p.value))
        .collect()
}

/// Runs `command` in a session and checks undo and redo are exact.
fn run(p: Project, command: Command) -> Project {
    let before = p.clone();
    let mut s = Session::new(p);
    s.execute(command).expect("apply");
    let after = s.project().clone();
    assert!(s.undo());
    let mut undone = s.project().clone();
    undone.next_id = before.next_id;
    assert_eq!(undone, before, "undo is exact");
    assert!(s.redo());
    assert_eq!(s.project(), &after, "redo is exact, ids included");
    after
}

#[test]
fn insert_bars_opens_a_gap_everywhere() {
    let (p, vox) = song();
    let keys_id = clips(&p, KEYS)[0].id;
    let p = run(p, Command::InsertBars { at_bar: 2, bars: 2 });
    // The clip crossing bar 2 is split; its second half moves 2 bars.
    assert_eq!(spans(&p, KEYS), vec![(0.0, 4.0), (12.0, 4.0)]);
    let keys = clips(&p, KEYS);
    assert_eq!(keys[0].id, keys_id, "the first half keeps its id");
    assert_eq!(pitches(&keys[0]), vec![(60, 0.0), (62, 2.0)]);
    assert_eq!(pitches(&keys[1]), vec![(64, 0.0), (65, 3.0)]);
    // Audio picks up 2 seconds (4 beats at 120 BPM) into the file.
    assert_eq!(spans(&p, vox), vec![(0.0, 4.0), (12.0, 12.0)]);
    assert_eq!(offsets(&p, vox), vec![0.0, 2.0]);
    assert_eq!(
        markers(&p),
        vec![("Intro".into(), 0.0), ("Chorus".into(), 14.0)]
    );
    assert_eq!(chords(&p), vec![(0, 0.0), (7, 14.0)]);
    // The ramp holds its bar-2 value through the gap.
    assert_eq!(
        lane(&p),
        vec![(0.0, -12.0), (4.0, -6.0), (12.0, -6.0), (16.0, 0.0)]
    );
    assert_eq!(p.loop_region.end_beats, 24.0, "the loop grows");
}

#[test]
fn delete_bars_cuts_every_track_and_closes_the_gap() {
    let (mut p, vox) = song();
    Command::CreateClip {
        track_id: KEYS,
        start_beats: 8.0,
        length_beats: 4.0,
        name: None,
        notes: vec![note(67, 0.0)],
    }
    .apply(&mut p)
    .expect("clip");
    let p = run(
        p,
        Command::DeleteBars {
            from_bar: 2,
            bars: 1,
        },
    );
    assert_eq!(spans(&p, KEYS), vec![(0.0, 4.0), (4.0, 4.0)]);
    assert_eq!(pitches(&clips(&p, KEYS)[1]), vec![(67, 0.0)]);
    // Audio: bar 2 of the recording is skipped.
    assert_eq!(spans(&p, vox), vec![(0.0, 4.0), (4.0, 8.0)]);
    assert_eq!(offsets(&p, vox), vec![0.0, 4.0]);
    // The chorus and its G chord were playing at the cut's end, so they
    // carry on from where the cut began.
    assert_eq!(
        markers(&p),
        vec![("Intro".into(), 0.0), ("Chorus".into(), 4.0)]
    );
    assert_eq!(chords(&p), vec![(0, 0.0), (7, 4.0)]);
    assert_eq!(lane(&p), vec![(0.0, -12.0), (4.0, -6.0), (4.0, 0.0)]);
    assert_eq!(p.loop_region.end_beats, 12.0);
}

#[test]
fn delete_bars_in_the_middle_of_a_clip_keeps_both_ends() {
    let (p, vox) = song();
    let p = run(
        p,
        Command::DeleteBars {
            from_bar: 2,
            bars: 2,
        },
    );
    assert_eq!(spans(&p, vox), vec![(0.0, 4.0), (4.0, 4.0)]);
    assert_eq!(offsets(&p, vox), vec![0.0, 6.0]);
    assert_eq!(spans(&p, KEYS), vec![(0.0, 4.0)]);
}

#[test]
fn repeat_bars_plays_a_section_again() {
    let (p, vox) = song();
    let p = run(
        p,
        Command::RepeatBars {
            from_bar: 2,
            bars: 1,
            times: 2,
        },
    );
    // Bar 2 of Keys is copied twice; nothing followed it.
    assert_eq!(spans(&p, KEYS), vec![(0.0, 8.0), (8.0, 4.0), (12.0, 4.0)]);
    for copy in &clips(&p, KEYS)[1..] {
        assert_eq!(pitches(copy), vec![(64, 0.0), (65, 3.0)]);
    }
    // The audio after bar 2 moves 2 bars later; the copies replay bar 2.
    assert_eq!(
        spans(&p, vox),
        vec![(0.0, 8.0), (8.0, 4.0), (12.0, 4.0), (16.0, 8.0)]
    );
    assert_eq!(offsets(&p, vox), vec![0.0, 2.0, 2.0, 4.0]);
    assert_eq!(
        markers(&p),
        vec![
            ("Intro".into(), 0.0),
            ("Chorus".into(), 6.0),
            ("Chorus 2".into(), 10.0),
            ("Chorus 3".into(), 14.0)
        ]
    );
    // Each copy starts on the chord playing at bar 2 (C), then G.
    assert_eq!(
        chords(&p),
        vec![
            (0, 0.0),
            (7, 6.0),
            (0, 8.0),
            (7, 10.0),
            (0, 12.0),
            (7, 14.0)
        ]
    );
    // Each copy replays bar 2's part of the ramp, -6 → 0.
    assert_eq!(
        lane(&p),
        vec![
            (0.0, -12.0),
            (8.0, 0.0),
            (8.0, -6.0),
            (12.0, 0.0),
            (12.0, -6.0),
            (16.0, 0.0)
        ]
    );
}

#[test]
fn repeat_bars_copies_only_the_part_of_a_clip_inside_the_range() {
    let (p, vox) = song();
    let p = run(
        p,
        Command::RepeatBars {
            from_bar: 2,
            bars: 2,
            times: 1,
        },
    );
    // Keys covers bars 1-2: the copy is only its bar 2, at bar 4.
    assert_eq!(spans(&p, KEYS), vec![(0.0, 8.0), (12.0, 4.0)]);
    assert_eq!(pitches(&clips(&p, KEYS)[1]), vec![(64, 0.0), (65, 3.0)]);
    assert_eq!(spans(&p, vox), vec![(0.0, 12.0), (12.0, 8.0), (20.0, 4.0)]);
    assert_eq!(offsets(&p, vox), vec![0.0, 2.0, 6.0]);
}

#[test]
fn bad_ranges_are_refused_and_change_nothing() {
    let (p, _) = song();
    for command in [
        Command::InsertBars { at_bar: 0, bars: 1 },
        Command::InsertBars { at_bar: 1, bars: 0 },
        Command::InsertBars {
            at_bar: 1,
            bars: 5000,
        },
        Command::DeleteBars {
            from_bar: 0,
            bars: 1,
        },
        Command::RepeatBars {
            from_bar: 1,
            bars: 1,
            times: 0,
        },
        Command::RepeatBars {
            from_bar: 1,
            bars: 1,
            times: 65,
        },
    ] {
        let mut q = p.clone();
        assert!(command.clone().apply(&mut q).is_err(), "{command:?}");
        assert_eq!(q, p, "{command:?}");
    }
}

#[test]
fn times_defaults_to_one_in_json() {
    let c: Command =
        serde_json::from_str(r#"{"command":"repeat_bars","from_bar":9,"bars":8}"#).expect("json");
    assert_eq!(
        c,
        Command::RepeatBars {
            from_bar: 9,
            bars: 8,
            times: 1
        }
    );
}

// ---- Tempo changes move with bar edits ----

/// (beats, bpm) of each tempo change, in order.
fn tempos(p: &Project) -> Vec<(f64, f64)> {
    p.tempo_changes.iter().map(|c| (c.beats, c.bpm)).collect()
}

fn with_tempos(changes: &[(f64, f64)]) -> Project {
    let (mut p, _) = song();
    for &(beats, bpm) in changes {
        Command::AddTempoChange {
            beats,
            bpm,
            ramp: false,
        }
        .apply(&mut p)
        .expect("tempo change");
    }
    p
}

#[test]
fn insert_bars_moves_later_tempo_changes() {
    let p = with_tempos(&[(2.0, 90.0), (8.0, 140.0)]);
    let p = run(p, Command::InsertBars { at_bar: 2, bars: 1 });
    // Before the gap stays; from the gap on moves one bar later.
    assert_eq!(tempos(&p), vec![(2.0, 90.0), (12.0, 140.0)]);
}

#[test]
fn delete_bars_keeps_the_tempo_playing_after_the_cut() {
    let p = with_tempos(&[(6.0, 140.0), (10.0, 100.0)]);
    let p = run(
        p,
        Command::DeleteBars {
            from_bar: 2,
            bars: 1,
        },
    );
    // The change inside the cut takes effect where the cut was, so what
    // followed it still plays at 140; the later one closes up.
    assert_eq!(tempos(&p), vec![(4.0, 140.0), (6.0, 100.0)]);
    assert_eq!(p.tempo_map().bpm_at(5.0), 140.0);
}

#[test]
fn delete_bars_at_the_start_makes_its_tempo_the_song_tempo() {
    let p = with_tempos(&[(2.0, 150.0), (8.0, 100.0)]);
    let p = run(
        p,
        Command::DeleteBars {
            from_bar: 1,
            bars: 1,
        },
    );
    assert_eq!(p.tempo_bpm, 150.0, "the cut's tempo now starts the song");
    assert_eq!(tempos(&p), vec![(4.0, 100.0)]);
    // A change right on the cut's end lands on beat 0 the same way.
    let p = with_tempos(&[(4.0, 160.0)]);
    let p = run(
        p,
        Command::DeleteBars {
            from_bar: 1,
            bars: 1,
        },
    );
    assert_eq!(p.tempo_bpm, 160.0);
    assert!(p.tempo_changes.is_empty());
}

#[test]
fn repeat_bars_copies_tempo_changes_and_restarts_each_copy() {
    // 120 BPM, then 140 from beat 6 (inside bar 2).
    let p = with_tempos(&[(6.0, 140.0), (16.0, 90.0)]);
    let p = run(
        p,
        Command::RepeatBars {
            from_bar: 2,
            bars: 1,
            times: 2,
        },
    );
    // Each copy starts at 120 like the original bar 2 did and speeds up
    // halfway; the later change moves past both copies.
    assert_eq!(
        tempos(&p),
        vec![
            (6.0, 140.0),
            (8.0, 120.0),
            (10.0, 140.0),
            (12.0, 120.0),
            (14.0, 140.0),
            (24.0, 90.0)
        ]
    );
}
