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

fn region(file: &str, seconds: f64) -> crate::project::AudioRegion {
    crate::project::AudioRegion {
        file: file.into(),
        file_seconds: seconds,
        offset_seconds: 0.0,
        gain_db: 0.0,
        fade_in_seconds: 0.0,
        fade_out_seconds: 0.0,
    }
}

/// Default project plus a clip (id 4) with notes (ids 5, 6, 7) on Keys and
/// a reverb (id 8) on Keys and a limiter (id 9) on the master, and an audio
/// track "Vox" (id 10) holding a 4-second audio clip (id 11) at beat 0.
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
    Command::AddTrack {
        name: "Vox".into(),
        instrument: InstrumentKind::Audio,
        preset: None,
        index: None,
    }
    .apply(&mut p)
    .expect("audio track");
    Command::AddAudioClip {
        track_id: 10,
        start_beats: 0.0,
        audio: region("vox-0123abcd.wav", 4.0),
        length_beats: None,
        name: None,
    }
    .apply(&mut p)
    .expect("audio clip");
    Command::AddAutomationLane {
        track_id: 1,
        target: AutomationTarget::Volume,
        points: vec![pt(0.0, -12.0), pt(8.0, 0.0)],
    }
    .apply(&mut p)
    .expect("lane");
    Command::AddTrack {
        name: "Piano".into(),
        instrument: InstrumentKind::Sampler,
        preset: None,
        index: None,
    }
    .apply(&mut p)
    .expect("sampler");
    p
}

const AUDIO_CLIP: ClipId = 11;
/// The Keys volume lane in the fixture.
const LANE: LaneId = 12;

fn pt(beats: f64, value: f64) -> AutomationPoint {
    AutomationPoint { beats, value }
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
        Command::SplitClip {
            clip_id: clip,
            at_beats: 1.5,
        },
        Command::TrimClipStart {
            clip_id: clip,
            start_beats: 1.0,
        },
        Command::AddAudioClip {
            track_id: 10,
            start_beats: 16.0,
            audio: region("guitar.wav", 10.0),
            length_beats: Some(4.0),
            name: Some("Riff".into()),
        },
        Command::SetAudioClip {
            clip_id: AUDIO_CLIP,
            gain_db: Some(-6.0),
            fade_in_seconds: None,
            fade_out_seconds: Some(0.5),
        },
        Command::SplitClip {
            clip_id: AUDIO_CLIP,
            at_beats: 2.0,
        },
        Command::TrimClipStart {
            clip_id: AUDIO_CLIP,
            start_beats: 1.0,
        },
        Command::DuplicateClip {
            clip_id: AUDIO_CLIP,
            start_beats: None,
        },
        Command::AddAutomationLane {
            track_id: 1,
            target: AutomationTarget::InstrumentParam {
                param: "filter.cutoff_hz".into(),
            },
            points: vec![pt(0.0, 300.0), pt(16.0, 6000.0)],
        },
        Command::AddAutomationLane {
            track_id: 1,
            target: AutomationTarget::EffectParam {
                effect_id: fx,
                param: "mix".into(),
            },
            points: vec![],
        },
        Command::SetAutomationPoints {
            track_id: 1,
            lane_id: LANE,
            points: vec![pt(4.0, -60.0), pt(2.0, -6.0)],
        },
        Command::SetAutomationEnabled {
            track_id: 1,
            lane_id: LANE,
            enabled: false,
        },
        Command::LoadSamplePack {
            track_id: 13,
            path: Some("C:\\Samples\\Salamander\\SalamanderGrandPiano.sfz".into()),
        },
        Command::RemoveAutomationLane {
            track_id: 1,
            lane_id: LANE,
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
        sample_pack: None,
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

#[test]
fn batch_applies_as_one_step_and_undoes_in_reverse() {
    let mut s = Session::new(fixture());
    let before = s.project().clone();
    s.execute(Command::Batch {
        commands: vec![
            Command::SetTempo { bpm: 90.0 },
            Command::AddTrack {
                name: "Lead".into(),
                instrument: InstrumentKind::Synth,
                preset: Some("Bright Lead".into()),
                index: None,
            },
            Command::RenameProject {
                name: "Boss".into(),
            },
        ],
    })
    .expect("batch");
    assert_eq!(s.project().tempo_bpm, 90.0);
    assert_eq!(s.project().tracks.len(), before.tracks.len() + 1);
    assert!(s.undo());
    assert_eq!(s.project().tracks, before.tracks);
    assert_eq!(s.project().tempo_bpm, before.tempo_bpm);
    assert!(!s.can_undo());
    assert!(s.redo());
    assert_eq!(s.project().name, "Boss");
}

#[test]
fn failed_batch_changes_nothing_and_names_the_step() {
    let base = fixture();
    let mut p = base.clone();
    let err = Command::Batch {
        commands: vec![
            Command::SetTempo { bpm: 90.0 },
            Command::AddTrack {
                name: "Lead".into(),
                instrument: InstrumentKind::Synth,
                preset: None,
                index: None,
            },
            Command::DeleteClip { clip_id: 999 },
        ],
    }
    .apply(&mut p)
    .expect_err("fails");
    let mut restored = p.clone();
    restored.next_id = base.next_id;
    assert_eq!(restored, base);
    let message = err.to_string();
    assert!(message.contains("step 3 (delete_clip)"), "{message}");
}

#[test]
fn command_name_is_snake_case() {
    assert_eq!(Command::SetTempo { bpm: 1.0 }.name(), "set_tempo");
}

#[test]
fn audio_clip_defaults_to_the_files_length_and_name() {
    let p = fixture();
    let (track, c) = p.clip(AUDIO_CLIP).expect("clip");
    assert_eq!(track, 10);
    assert_eq!(c.name, "vox");
    // 4 seconds at 120 BPM is 8 beats.
    assert!((c.length_beats - 8.0).abs() < 1e-9);
}

#[test]
fn splitting_audio_continues_the_recording_in_the_second_half() {
    let mut p = fixture();
    Command::SplitClip {
        clip_id: AUDIO_CLIP,
        at_beats: 2.0,
    }
    .apply(&mut p)
    .expect("split");
    let clips = &p.track(10).expect("track").clips;
    assert_eq!(clips.len(), 2);
    let (a, b) = (&clips[0], &clips[1]);
    assert_eq!(a.id, AUDIO_CLIP);
    assert!((a.length_beats - 2.0).abs() < 1e-9);
    assert!((b.start_beats - 2.0).abs() < 1e-9);
    assert!((b.length_beats - 6.0).abs() < 1e-9);
    // 2 beats at 120 BPM = 1 second into the file.
    let offset = b.audio.as_ref().expect("audio").offset_seconds;
    assert!((offset - 1.0).abs() < 1e-9);
}

#[test]
fn splitting_notes_moves_later_notes_to_the_new_clip() {
    let mut p = fixture();
    let id = clip_id(&p);
    Command::SplitClip {
        clip_id: id,
        at_beats: 1.5,
    }
    .apply(&mut p)
    .expect("split");
    let clips = &p.tracks[0].clips;
    assert_eq!(clips[0].notes.len(), 2);
    assert_eq!(clips[1].notes.len(), 1);
    assert!((clips[1].notes[0].start_beats - 0.5).abs() < 1e-9);
}

#[test]
fn trimming_audio_start_skips_into_the_recording() {
    let mut p = fixture();
    Command::TrimClipStart {
        clip_id: AUDIO_CLIP,
        start_beats: 1.0,
    }
    .apply(&mut p)
    .expect("trim");
    let (_, c) = p.clip(AUDIO_CLIP).expect("clip");
    assert!((c.start_beats - 1.0).abs() < 1e-9);
    assert!((c.length_beats - 7.0).abs() < 1e-9);
    assert!((c.audio.as_ref().expect("audio").offset_seconds - 0.5).abs() < 1e-9);
    // Can't drag back past the start of the file.
    let mut q = p.clone();
    q.clip_mut(AUDIO_CLIP)
        .expect("clip")
        .audio
        .as_mut()
        .expect("a")
        .offset_seconds = 0.0;
    let err = Command::TrimClipStart {
        clip_id: AUDIO_CLIP,
        start_beats: 0.0,
    }
    .apply(&mut q);
    assert!(err.is_err());
}

#[test]
fn audio_and_note_clips_stay_on_their_own_kind_of_track() {
    let base = fixture();
    let note_clip = clip_id(&base);
    let rejected = [
        Command::MoveClip {
            clip_id: AUDIO_CLIP,
            start_beats: None,
            track_id: Some(1),
        },
        Command::MoveClip {
            clip_id: note_clip,
            start_beats: None,
            track_id: Some(10),
        },
        Command::CreateClip {
            track_id: 10,
            start_beats: 0.0,
            length_beats: 4.0,
            name: None,
            notes: vec![],
        },
        Command::AddAudioClip {
            track_id: 1,
            start_beats: 0.0,
            audio: region("a.wav", 1.0),
            length_beats: None,
            name: None,
        },
        Command::AddNotes {
            clip_id: AUDIO_CLIP,
            notes: vec![note(60, 0.0, 1.0)],
        },
        Command::SetAudioClip {
            clip_id: note_clip,
            gain_db: Some(-3.0),
            fade_in_seconds: None,
            fade_out_seconds: None,
        },
        Command::SetInstrument {
            track_id: 10,
            instrument: Instrument::from_preset(InstrumentKind::Synth, "Init").expect("preset"),
        },
    ];
    for command in rejected {
        let mut p = base.clone();
        assert!(
            command.clone().apply(&mut p).is_err(),
            "{command:?} was allowed"
        );
        assert_eq!(p, base, "{command:?} changed the project");
    }
}

#[test]
fn audio_file_names_cannot_be_paths() {
    for bad in ["../x.wav", "C:\\x.wav", "a/b.wav", "", ".hidden.wav"] {
        let mut p = fixture();
        let r = Command::AddAudioClip {
            track_id: 10,
            start_beats: 0.0,
            audio: region(bad, 1.0),
            length_beats: None,
            name: None,
        }
        .apply(&mut p);
        assert!(r.is_err(), "{bad} was accepted");
    }
}

#[test]
fn audio_clip_gain_drag_is_one_undo_step() {
    let mut s = Session::new(fixture());
    for g in [-1.0, -2.0, -3.0] {
        s.execute(Command::SetAudioClip {
            clip_id: AUDIO_CLIP,
            gain_db: Some(g),
            fade_in_seconds: None,
            fade_out_seconds: None,
        })
        .expect("gain");
    }
    s.end_gesture();
    assert!(s.undo());
    let (_, c) = s.project().clip(AUDIO_CLIP).expect("clip");
    assert_eq!(c.audio.as_ref().expect("audio").gain_db, 0.0);
}

#[test]
fn automation_points_are_checked_sorted_and_interpolated() {
    let mut p = fixture();
    Command::SetAutomationPoints {
        track_id: 1,
        lane_id: LANE,
        points: vec![pt(8.0, 0.0), pt(0.0, -20.0)],
    }
    .apply(&mut p)
    .expect("set");
    let lane = &p.track(1).expect("t").automation[0];
    assert_eq!(lane.points[0].beats, 0.0);
    assert_eq!(lane.value_at(-1.0), Some(-20.0));
    assert_eq!(lane.value_at(4.0), Some(-10.0));
    assert_eq!(lane.value_at(100.0), Some(0.0));
    let rejected = [
        Command::SetAutomationPoints {
            track_id: 1,
            lane_id: LANE,
            points: vec![pt(0.0, 12.0)],
        },
        Command::AddAutomationLane {
            track_id: 1,
            target: AutomationTarget::Volume,
            points: vec![],
        },
        Command::AddAutomationLane {
            track_id: 1,
            target: AutomationTarget::InstrumentParam {
                param: "nope".into(),
            },
            points: vec![],
        },
        Command::AddAutomationLane {
            track_id: 1,
            target: AutomationTarget::EffectParam {
                effect_id: 999,
                param: "mix".into(),
            },
            points: vec![],
        },
    ];
    for c in rejected {
        let mut q = p.clone();
        assert!(c.clone().apply(&mut q).is_err(), "{c:?}");
        assert_eq!(q, p);
    }
}

#[test]
fn automation_drag_is_one_undo_step() {
    let mut s = Session::new(fixture());
    for v in [-10.0, -8.0, -6.0] {
        s.execute(Command::SetAutomationPoints {
            track_id: 1,
            lane_id: LANE,
            points: vec![pt(0.0, v)],
        })
        .expect("drag");
    }
    s.end_gesture();
    assert!(s.undo());
    assert_eq!(
        s.project().track(1).expect("t").automation[0].points.len(),
        2
    );
}

#[test]
fn sample_packs_are_sfz_files_on_sampler_tracks() {
    let base = fixture();
    for (track_id, path) in [(1, Some("a.sfz")), (13, Some("piano.wav")), (13, Some(""))] {
        let mut p = base.clone();
        let r = Command::LoadSamplePack {
            track_id,
            path: path.map(str::to_owned),
        }
        .apply(&mut p);
        assert!(r.is_err(), "{track_id} {path:?}");
    }
}
