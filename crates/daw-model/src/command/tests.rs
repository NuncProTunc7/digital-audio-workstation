use super::*;
use crate::effect::EffectKind;
use crate::instrument::InstrumentKind;
use crate::session::Session;

fn note(pitch: u8, start: f64, len: f64) -> NoteInput {
    NoteInput {
        pitch,
        start_beats: start,
        length_beats: len,
        velocity: 100,
        id: None,
    }
}

/// Default project plus a clip (id 4) with notes (ids 5, 6, 7) on Keys and
/// a reverb (id 8) on Keys and a limiter (id 9) on the master.
fn fixture() -> Project {
    let mut p = Project::default();
    Command::CreateClip {
        track_id: 1,
        start_beats: 0.0,
        length_beats: 8.0,
        name: Some("Verse".into()),
        notes: vec![note(60, 0.1, 1.0), note(64, 1.05, 0.9), note(67, 2.0, 1.0)],
    }
    .apply(&mut p)
    .expect("clip");
    Command::AddEffect {
        track_id: Some(1),
        kind: EffectKind::Reverb,
        index: None,
    }
    .apply(&mut p)
    .expect("fx");
    Command::AddEffect {
        track_id: None,
        kind: EffectKind::Limiter,
        index: None,
    }
    .apply(&mut p)
    .expect("master fx");
    p
}

fn clip_id(p: &Project) -> ClipId {
    p.tracks[0].clips[0].id
}

fn sample_commands(p: &Project) -> Vec<Command> {
    let clip = clip_id(p);
    let first_note = p.tracks[0].clips[0].notes[0].id;
    let fx = p.tracks[0].mixer.effects[0].id;
    let master_fx = p.master.effects[0].id;
    vec![
        Command::RenameProject {
            name: "Boss Theme".into(),
        },
        Command::SetTempo { bpm: 140.0 },
        Command::SetTimeSignature {
            numerator: 6,
            denominator: 8,
        },
        Command::SetLoop {
            enabled: Some(true),
            start_beats: Some(4.0),
            end_beats: Some(12.0),
        },
        Command::AddTrack {
            name: "Lead".into(),
            instrument: InstrumentKind::Synth,
            preset: Some("Bright Lead".into()),
            index: Some(0),
        },
        Command::RemoveTrack { track_id: 2 },
        Command::RenameTrack {
            track_id: 3,
            name: "Kit".into(),
        },
        Command::MoveTrack {
            track_id: 3,
            index: 0,
        },
        Command::SetTrackMixer {
            track_id: 1,
            volume_db: Some(-6.0),
            pan: Some(-0.5),
            mute: Some(true),
            solo: None,
        },
        Command::SetMasterVolume { volume_db: -3.0 },
        Command::SetInstrumentParam {
            track_id: 1,
            param: "filter.cutoff_hz".into(),
            value: 440.0,
        },
        Command::LoadPreset {
            track_id: 2,
            preset: "Acid Bass".into(),
        },
        Command::SetInstrument {
            track_id: 1,
            instrument: Instrument::from_preset(InstrumentKind::Synth, "Chip Square")
                .expect("preset"),
        },
        Command::AddEffect {
            track_id: Some(2),
            kind: EffectKind::Delay,
            index: None,
        },
        Command::RemoveEffect {
            track_id: Some(1),
            effect_id: fx,
        },
        Command::SetEffectParam {
            track_id: None,
            effect_id: master_fx,
            param: "ceiling_db".into(),
            value: -3.0,
        },
        Command::SetEffectEnabled {
            track_id: Some(1),
            effect_id: fx,
            enabled: false,
        },
        Command::CreateClip {
            track_id: 3,
            start_beats: 8.0,
            length_beats: 4.0,
            name: None,
            notes: vec![note(36, 0.0, 0.25), note(38, 1.0, 0.25)],
        },
        Command::DeleteClip { clip_id: clip },
        Command::MoveClip {
            clip_id: clip,
            start_beats: Some(16.0),
            track_id: Some(2),
        },
        Command::ResizeClip {
            clip_id: clip,
            length_beats: 4.0,
        },
        Command::RenameClip {
            clip_id: clip,
            name: "Chorus".into(),
        },
        Command::DuplicateClip {
            clip_id: clip,
            start_beats: None,
        },
        Command::AddNotes {
            clip_id: clip,
            notes: vec![note(72, 4.0, 2.0)],
        },
        Command::RemoveNotes {
            clip_id: clip,
            note_ids: vec![first_note],
        },
        Command::EditNotes {
            clip_id: clip,
            edits: vec![NoteEdit {
                id: first_note,
                pitch: Some(61),
                start_beats: None,
                length_beats: Some(2.0),
                velocity: Some(50),
            }],
        },
        Command::QuantizeNotes {
            clip_id: clip,
            grid_beats: 0.5,
            strength: None,
            lengths: true,
            note_ids: None,
        },
        Command::TransposeNotes {
            clip_id: clip,
            semitones: -12,
            note_ids: Some(vec![first_note]),
        },
    ]
}

#[test]
fn commands_round_trip_through_json() {
    for command in sample_commands(&fixture()) {
        let json = serde_json::to_string(&command).expect("serialize");
        let back: Command = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(command, back, "{json}");
    }
}

#[test]
fn json_uses_snake_case_tag() {
    let json = serde_json::to_value(Command::SetTempo { bpm: 90.0 }).expect("serialize");
    assert_eq!(
        json,
        serde_json::json!({ "command": "set_tempo", "bpm": 90.0 })
    );
}

#[test]
fn every_command_changes_something_and_undoes_exactly() {
    let base = fixture();
    for command in sample_commands(&base) {
        let mut project = base.clone();
        let inverse = command.clone().apply(&mut project).expect("apply");
        assert_ne!(project, base, "{command:?} changed nothing");
        let redo = inverse.clone().apply(&mut project).expect("undo");
        // next_id may advance; everything else must match.
        let mut restored = project.clone();
        restored.next_id = base.next_id;
        assert_eq!(restored, base, "{command:?} did not undo");
        // Redo reproduces the edited state.
        let mut again = project.clone();
        redo.apply(&mut again).expect("redo");
        let mut expected = base.clone();
        command.apply(&mut expected).expect("reapply");
        expected.next_id = again.next_id;
        assert_eq!(again.tracks.len(), expected.tracks.len(), "{inverse:?}");
    }
}

#[test]
fn undo_redo_of_added_track_keeps_its_id() {
    let mut s = Session::new(fixture());
    s.execute(Command::AddTrack {
        name: "Lead".into(),
        instrument: InstrumentKind::Synth,
        preset: None,
        index: None,
    })
    .expect("add");
    let id = s.project().tracks.last().expect("track").id;
    s.execute(Command::SetTrackMixer {
        track_id: id,
        volume_db: Some(-12.0),
        pan: None,
        mute: None,
        solo: None,
    })
    .expect("mixer");
    assert!(s.undo() && s.undo());
    assert!(s.redo() && s.redo(), "redo chain broke");
    let t = s.project().track(id).expect("same id restored");
    assert_eq!(t.mixer.volume_db, -12.0);
}

#[test]
fn undo_of_delete_restores_clip_with_same_ids() {
    let mut s = Session::new(fixture());
    let before = s.project().clone();
    let clip = clip_id(&before);
    s.execute(Command::DeleteClip { clip_id: clip })
        .expect("delete");
    assert!(s.project().clip(clip).is_none());
    s.undo();
    assert_eq!(s.project().tracks, before.tracks);
}

#[test]
fn invalid_commands_are_rejected_without_changes() {
    let base = fixture();
    let clip = clip_id(&base);
    let bad = [
        Command::RenameProject { name: "  ".into() },
        Command::SetTempo { bpm: 5.0 },
        Command::SetTempo { bpm: f64::NAN },
        Command::SetTimeSignature {
            numerator: 4,
            denominator: 3,
        },
        Command::SetLoop {
            enabled: None,
            start_beats: Some(10.0),
            end_beats: Some(2.0),
        },
        Command::SetInstrumentParam {
            track_id: 99,
            param: "filter.cutoff_hz".into(),
            value: 440.0,
        },
        Command::SetInstrumentParam {
            track_id: 1,
            param: "filter.cutoff_hz".into(),
            value: 99_999.0,
        },
        Command::SetInstrumentParam {
            track_id: 3,
            param: "filter.cutoff_hz".into(),
            value: 440.0,
        },
        Command::LoadPreset {
            track_id: 1,
            preset: "Classic Kit".into(),
        },
        Command::AddTrack {
            name: "x".into(),
            instrument: InstrumentKind::Drums,
            preset: Some("Warm Keys".into()),
            index: None,
        },
        Command::SetTrackMixer {
            track_id: 1,
            volume_db: Some(20.0),
            pan: None,
            mute: None,
            solo: None,
        },
        Command::SetTrackMixer {
            track_id: 1,
            volume_db: None,
            pan: Some(2.0),
            mute: None,
            solo: None,
        },
        Command::SetEffectParam {
            track_id: Some(2),
            effect_id: base.tracks[0].mixer.effects[0].id,
            param: "mix".into(),
            value: 0.5,
        },
        Command::CreateClip {
            track_id: 1,
            start_beats: -1.0,
            length_beats: 4.0,
            name: None,
            notes: vec![],
        },
        Command::CreateClip {
            track_id: 1,
            start_beats: 0.0,
            length_beats: 4.0,
            name: None,
            notes: vec![note(200, 0.0, 1.0)],
        },
        Command::AddNotes {
            clip_id: clip,
            notes: vec![NoteInput {
                velocity: 0,
                ..note(60, 0.0, 1.0)
            }],
        },
        Command::RemoveNotes {
            clip_id: clip,
            note_ids: vec![12345],
        },
        Command::TransposeNotes {
            clip_id: clip,
            semitones: 100,
            note_ids: None,
        },
        Command::ResizeClip {
            clip_id: clip,
            length_beats: 0.0,
        },
        Command::DeleteClip { clip_id: 999 },
        Command::AddNotes {
            clip_id: clip,
            notes: vec![NoteInput {
                id: Some(1),
                ..note(60, 0.0, 1.0)
            }],
        },
    ];
    for command in bad {
        let mut project = base.clone();
        assert!(command.clone().apply(&mut project).is_err(), "{command:?}");
        assert_eq!(project, base, "{command:?} changed the project");
    }
}

#[test]
fn quantize_snaps_starts_and_lengths() {
    let mut p = fixture();
    let clip = clip_id(&p);
    Command::QuantizeNotes {
        clip_id: clip,
        grid_beats: 1.0,
        strength: None,
        lengths: true,
        note_ids: None,
    }
    .apply(&mut p)
    .expect("quantize");
    let starts: Vec<f64> = p.tracks[0].clips[0]
        .notes
        .iter()
        .map(|n| n.start_beats)
        .collect();
    let lengths: Vec<f64> = p.tracks[0].clips[0]
        .notes
        .iter()
        .map(|n| n.length_beats)
        .collect();
    assert_eq!(starts, vec![0.0, 1.0, 2.0]);
    assert_eq!(lengths, vec![1.0, 1.0, 1.0]);
}

#[test]
fn half_strength_quantize_moves_halfway() {
    let mut p = fixture();
    let clip = clip_id(&p);
    Command::QuantizeNotes {
        clip_id: clip,
        grid_beats: 1.0,
        strength: Some(0.5),
        lengths: false,
        note_ids: None,
    }
    .apply(&mut p)
    .expect("quantize");
    assert!((p.tracks[0].clips[0].notes[0].start_beats - 0.05).abs() < 1e-9);
}

#[test]
fn duplicate_places_copy_after_original_with_new_ids() {
    let mut p = fixture();
    let clip = clip_id(&p);
    Command::DuplicateClip {
        clip_id: clip,
        start_beats: None,
    }
    .apply(&mut p)
    .expect("dup");
    let clips = &p.tracks[0].clips;
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[1].start_beats, 8.0);
    assert_eq!(clips[1].notes.len(), 3);
    assert_ne!(clips[1].notes[0].id, clips[0].notes[0].id);
}

#[test]
fn mixer_inverse_only_restores_fields_that_were_set() {
    let mut p = fixture();
    let inverse = Command::SetTrackMixer {
        track_id: 1,
        volume_db: Some(-10.0),
        pan: None,
        mute: None,
        solo: None,
    }
    .apply(&mut p)
    .expect("mixer");
    assert_eq!(
        inverse,
        Command::SetTrackMixer {
            track_id: 1,
            volume_db: Some(0.0),
            pan: None,
            mute: None,
            solo: None,
        }
    );
}

#[test]
fn out_of_range_error_tells_claude_the_valid_range() {
    let err = Command::SetInstrumentParam {
        track_id: 1,
        param: "osc1.wave".into(),
        value: 9.0,
    }
    .apply(&mut Project::default())
    .expect_err("rejected");
    assert!(err.to_string().contains("1 (Saw)"), "{err}");
}

#[test]
fn set_instrument_fills_missing_params_with_defaults() {
    let mut project = Project::default();
    let sparse = Instrument {
        kind: InstrumentKind::Synth,
        preset: "Custom".into(),
        params: [("filter.cutoff_hz".to_owned(), 300.0)].into(),
    };
    Command::SetInstrument {
        track_id: 1,
        instrument: sparse,
    }
    .apply(&mut project)
    .expect("ok");
    let inst = &project.track(1).expect("track").instrument;
    assert_eq!(inst.params.len(), crate::instrument::SYNTH_PARAMS.len());
}

#[test]
fn schema_carries_descriptions_for_claude() {
    let schema = command_schema().to_string();
    for needle in [
        "set_tempo",
        "create_clip",
        "quantize_notes",
        "Set the project tempo in beats per minute",
        "60 = middle C",
    ] {
        assert!(schema.contains(needle), "schema is missing {needle}");
    }
}
