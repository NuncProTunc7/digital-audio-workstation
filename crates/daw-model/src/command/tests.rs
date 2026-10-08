use super::*;
use crate::effect::EffectKind;
use crate::instrument::InstrumentKind;
use crate::session::Session;

fn note(pitch: u8, start: f64, len: f64) -> NoteInput {
    NoteInput {
        chance: 100,
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
        source_bpm: None,
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
                chance: None,
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
        Command::SetClipTempo {
            clip_id: AUDIO_CLIP,
            source_bpm: Some(120.0),
        },
        Command::LoadSamplePack {
            track_id: 13,
            path: Some("C:\\Samples\\Salamander\\SalamanderGrandPiano.sfz".into()),
        },
        Command::RemoveAutomationLane {
            track_id: 1,
            lane_id: LANE,
        },
        Command::SetClipSwing {
            clip_id: clip,
            swing: Some(crate::project::Swing {
                amount_percent: 60.0,
                grid_beats: 0.25,
            }),
        },
        Command::AddNotes {
            clip_id: clip,
            notes: vec![NoteInput {
                chance: 50,
                ..note(42, 3.5, 0.25)
            }],
        },
        Command::EditNotes {
            clip_id: clip,
            edits: vec![NoteEdit {
                id: first_note,
                pitch: None,
                start_beats: None,
                length_beats: None,
                velocity: None,
                chance: Some(25),
            }],
        },
    ]
}

#[test]
fn chance_and_swing_survive_undo_of_deletes_and_saving() {
    let mut p = fixture();
    let clip = clip_id(&p);
    let first = p.tracks[0].clips[0].notes[0].id;
    Command::EditNotes {
        clip_id: clip,
        edits: vec![NoteEdit {
            id: first,
            pitch: None,
            start_beats: None,
            length_beats: None,
            velocity: None,
            chance: Some(40),
        }],
    }
    .apply(&mut p)
    .expect("chance");
    let swing = crate::project::Swing {
        amount_percent: 55.0,
        grid_beats: 0.5,
    };
    Command::SetClipSwing {
        clip_id: clip,
        swing: Some(swing),
    }
    .apply(&mut p)
    .expect("swing");
    let before = p.clone();
    // Removing notes and putting them back keeps their chance.
    let undo = Command::RemoveNotes {
        clip_id: clip,
        note_ids: vec![first],
    }
    .apply(&mut p)
    .expect("remove");
    undo.apply(&mut p).expect("undo");
    assert_eq!(p.tracks[0].clips[0].notes, before.tracks[0].clips[0].notes);
    // Deleting the clip and undoing keeps both.
    let undo = Command::DeleteClip { clip_id: clip }
        .apply(&mut p)
        .expect("delete");
    undo.apply(&mut p).expect("undo");
    assert_eq!(p.tracks[0].clips[0], before.tracks[0].clips[0]);
    // JSON keeps them; full-chance notes and straight clips stay compact.
    let json = serde_json::to_string(&p.tracks[0].clips[0]).expect("json");
    assert!(json.contains("\"chance\":40") && json.contains("\"amount_percent\":55"));
    assert_eq!(json.matches("chance").count(), 1, "{json}");
    let back: crate::project::Clip = serde_json::from_str(&json).expect("back");
    assert_eq!(back, p.tracks[0].clips[0]);
}

#[test]
fn snapshot_commands_round_trip_through_json() {
    let mut p = fixture();
    let snapshot = crate::project::Snapshot {
        id: 99,
        name: "Calm".into(),
        song: p.song_state(),
    };
    let commands = [
        Command::TakeSnapshot {
            name: "Calm".into(),
        },
        Command::LoadSnapshot { snapshot_id: 99 },
        Command::RenameSnapshot {
            snapshot_id: 99,
            name: "Calmer".into(),
        },
        Command::DeleteSnapshot { snapshot_id: 99 },
        Command::RestoreSnapshot {
            snapshot: snapshot.clone(),
            index: 0,
        },
        Command::SetSongState {
            song: Box::new(snapshot.song.clone()),
        },
    ];
    for command in commands {
        let json = serde_json::to_string(&command).expect("serialize");
        let back: Command = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(command, back, "{json}");
    }
    p.snapshots.push(snapshot);
    let json = crate::file::project_to_json(&p);
    assert_eq!(
        crate::file::project_from_json(&json)
            .expect("load")
            .snapshots,
        p.snapshots
    );
}

#[test]
fn loading_a_version_brings_back_its_music_and_undoes_exactly() {
    let mut s = Session::new(fixture());
    s.execute(Command::TakeSnapshot {
        name: "Before Claude's changes".into(),
    })
    .expect("take");
    let version = s.project().snapshots[0].id;
    let before = s.project().clone();
    // Change the music a lot.
    s.execute(Command::SetTempo { bpm: 150.0 }).expect("tempo");
    s.execute(Command::RemoveTrack { track_id: 1 })
        .expect("remove");
    s.execute(Command::AddTrack {
        name: "Lead".into(),
        instrument: InstrumentKind::Synth,
        preset: None,
        index: None,
    })
    .expect("add");
    let changed = s.project().clone();
    let next_id = changed.next_id;
    s.execute(Command::LoadSnapshot {
        snapshot_id: version,
    })
    .expect("load");
    assert_eq!(s.project().song_state(), before.song_state());
    // The version list and the name stay; ids keep moving forward.
    assert_eq!(s.project().snapshots, before.snapshots);
    assert!(s.project().next_id >= next_id);
    s.undo();
    assert_eq!(s.project().song_state(), changed.song_state());
    s.redo();
    assert_eq!(s.project().song_state(), before.song_state());
}

#[test]
fn versions_can_be_renamed_deleted_and_restored() {
    let mut p = fixture();
    let undo_take = Command::TakeSnapshot { name: "A".into() }
        .apply(&mut p)
        .expect("take");
    Command::TakeSnapshot { name: "B".into() }
        .apply(&mut p)
        .expect("take");
    let a = p.snapshots[0].id;
    let undo_rename = Command::RenameSnapshot {
        snapshot_id: a,
        name: "Darker mix".into(),
    }
    .apply(&mut p)
    .expect("rename");
    assert_eq!(p.snapshots[0].name, "Darker mix");
    undo_rename.apply(&mut p).expect("undo rename");
    let with_both = p.clone();
    let undo_delete = Command::DeleteSnapshot { snapshot_id: a }
        .apply(&mut p)
        .expect("delete");
    assert_eq!(p.snapshots.len(), 1);
    undo_delete.apply(&mut p).expect("restore");
    assert_eq!(p.snapshots, with_both.snapshots);
    // Undoing the first take removes only that version.
    undo_take.apply(&mut p).expect("undo take");
    assert_eq!(
        p.snapshots
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["B"]
    );
    // Bad names and unknown versions are refused.
    for bad in [
        Command::TakeSnapshot { name: "  ".into() },
        Command::LoadSnapshot { snapshot_id: 12345 },
        Command::DeleteSnapshot { snapshot_id: 12345 },
    ] {
        let before = p.clone();
        assert!(bad.apply(&mut p).is_err());
        assert_eq!(p, before);
    }
}

#[test]
fn audio_used_only_by_a_version_is_still_part_of_the_song() {
    let mut p = fixture();
    Command::TakeSnapshot {
        name: "With vocals".into(),
    }
    .apply(&mut p)
    .expect("take");
    Command::DeleteClip {
        clip_id: AUDIO_CLIP,
    }
    .apply(&mut p)
    .expect("delete");
    assert_eq!(p.audio_files(), vec!["vox-0123abcd.wav".to_owned()]);
}

#[test]
fn markers_divide_the_song_into_sections() {
    let mut s = Session::new(fixture());
    // The fixture's clips end at beat 8: two bars.
    s.execute(Command::AddMarker {
        name: "Combat".into(),
        start_beats: 4.0,
    })
    .expect("combat");
    s.execute(Command::AddMarker {
        name: " Explore ".into(),
        start_beats: 0.0,
    })
    .expect("explore");
    let p = s.project();
    assert_eq!(
        p.markers
            .iter()
            .map(|m| m.name.as_str())
            .collect::<Vec<_>>(),
        ["Explore", "Combat"],
        "kept in time order, names trimmed"
    );
    let sections: Vec<_> = p
        .sections()
        .into_iter()
        .map(|x| (x.name, x.start_beats, x.end_beats))
        .collect();
    assert_eq!(
        sections,
        [("Explore".into(), 0.0, 4.0), ("Combat".into(), 4.0, 8.0)]
    );
    let summary = crate::summary::song_summary(p);
    assert_eq!(summary["sections"][1]["name"], "Combat");
    // One marker per beat; names can't be empty.
    for bad in [
        Command::AddMarker {
            name: "Again".into(),
            start_beats: 4.0,
        },
        Command::AddMarker {
            name: "  ".into(),
            start_beats: 2.0,
        },
        Command::AddMarker {
            name: "Late".into(),
            start_beats: -1.0,
        },
    ] {
        assert!(s.execute(bad).is_err());
    }
}

#[test]
fn marker_commands_round_trip_undo_and_drag_as_one_step() {
    let mut s = Session::new(fixture());
    s.execute(Command::AddMarker {
        name: "Boss".into(),
        start_beats: 8.0,
    })
    .expect("add");
    let id = s.project().markers[0].id;
    let marker = s.project().markers[0].clone();
    for command in [
        Command::AddMarker {
            name: "Boss".into(),
            start_beats: 8.0,
        },
        Command::MoveMarker {
            marker_id: id,
            start_beats: 12.0,
        },
        Command::RenameMarker {
            marker_id: id,
            name: "Final boss".into(),
        },
        Command::RemoveMarker { marker_id: id },
        Command::RestoreMarker {
            marker: marker.clone(),
        },
    ] {
        let json = serde_json::to_string(&command).expect("serialize");
        assert_eq!(
            command,
            serde_json::from_str::<Command>(&json).expect("back"),
            "{json}"
        );
    }
    // Dragging a marker through several beats is one undo step.
    for beat in [9.0, 10.0, 11.0, 12.0] {
        s.execute(Command::MoveMarker {
            marker_id: id,
            start_beats: beat,
        })
        .expect("move");
    }
    s.end_gesture();
    s.undo();
    assert_eq!(s.project().markers[0].start_beats, 8.0);
    // Removing and undoing keeps the id.
    s.execute(Command::RemoveMarker { marker_id: id })
        .expect("remove");
    assert!(s.project().markers.is_empty());
    s.undo();
    assert_eq!(s.project().markers, vec![marker]);
    // Saved versions keep their markers.
    s.execute(Command::TakeSnapshot { name: "v".into() })
        .expect("take");
    s.execute(Command::RemoveMarker { marker_id: id })
        .expect("remove");
    let version = s.project().snapshots[0].id;
    s.execute(Command::LoadSnapshot {
        snapshot_id: version,
    })
    .expect("load");
    assert_eq!(s.project().markers.len(), 1);
    // And files keep them.
    let json = crate::file::project_to_json(s.project());
    assert_eq!(
        crate::file::project_from_json(&json).expect("load").markers,
        s.project().markers
    );
}

#[test]
fn buses_take_tracks_sends_and_effects_and_undo_cleanly() {
    let mut s = Session::new(fixture());
    s.execute(Command::AddBus {
        name: "Drums".into(),
    })
    .expect("bus");
    s.execute(Command::AddBus {
        name: "Reverb".into(),
    })
    .expect("bus");
    let (drums, verb) = (s.project().buses[0].id, s.project().buses[1].id);
    s.execute(Command::SetTrackOutput {
        track_id: 3,
        bus_id: Some(drums),
    })
    .expect("route");
    s.execute(Command::SetSend {
        track_id: 1,
        bus_id: verb,
        level_db: None,
        pre_fader: None,
    })
    .expect("send");
    // Effects on a bus are addressed by its id.
    s.execute(Command::AddEffect {
        track_id: Some(verb),
        kind: EffectKind::Reverb,
        index: None,
    })
    .expect("bus reverb");
    s.execute(Command::SetBusMixer {
        bus_id: drums,
        volume_db: Some(-3.0),
        pan: None,
        mute: None,
    })
    .expect("bus level");
    let p = s.project();
    assert_eq!(p.tracks[2].output, Some(drums));
    assert_eq!(
        p.tracks[0].sends,
        vec![crate::project::Send {
            bus_id: verb,
            level_db: -6.0,
            pre_fader: false,
        }]
    );
    assert_eq!(p.buses[1].mixer.effects.len(), 1);
    assert_eq!(p.buses[0].mixer.volume_db, -3.0);
    let summary = crate::summary::song_summary(p);
    assert_eq!(summary["buses"][0]["tracks_playing_into_it"][0], 3);

    // A send drag is one undo step.
    for db in [-5.0, -4.0, -3.0] {
        s.execute(Command::SetSend {
            track_id: 1,
            bus_id: verb,
            level_db: Some(db),
            pre_fader: None,
        })
        .expect("drag");
    }
    s.end_gesture();
    s.undo();
    assert_eq!(s.project().tracks[0].sends[0].level_db, -6.0);

    // Removing a bus sends its tracks to the master; undo puts it all back.
    let before = s.project().clone();
    s.execute(Command::RemoveBus { bus_id: verb })
        .expect("remove");
    s.execute(Command::RemoveBus { bus_id: drums })
        .expect("remove");
    let p = s.project();
    assert!(p.buses.is_empty() && p.tracks[2].output.is_none() && p.tracks[0].sends.is_empty());
    s.undo();
    s.undo();
    assert_eq!(s.project(), &before);

    // Bad routing is refused.
    for bad in [
        Command::SetTrackOutput {
            track_id: 1,
            bus_id: Some(9_999),
        },
        Command::SetSend {
            track_id: 1,
            bus_id: 9_999,
            level_db: None,
            pre_fader: None,
        },
        Command::SetSend {
            track_id: 1,
            bus_id: drums,
            level_db: Some(40.0),
            pre_fader: None,
        },
        Command::RemoveSend {
            track_id: 2,
            bus_id: drums,
        },
        Command::AddBus { name: " ".into() },
    ] {
        assert!(s.execute(bad).is_err());
    }

    // Commands round-trip; files keep buses and drop routing to lost buses.
    let bus = s.project().buses[0].clone();
    for command in [
        Command::AddBus { name: "Fx".into() },
        Command::RenameBus {
            bus_id: drums,
            name: "Kit".into(),
        },
        Command::RemoveSend {
            track_id: 1,
            bus_id: verb,
        },
        Command::RestoreBus {
            bus,
            index: 0,
            outputs: vec![3],
            sends: vec![(
                1,
                crate::project::Send {
                    bus_id: drums,
                    level_db: -12.0,
                    pre_fader: true,
                },
            )],
        },
    ] {
        let json = serde_json::to_string(&command).expect("serialize");
        assert_eq!(
            command,
            serde_json::from_str::<Command>(&json).expect("back"),
            "{json}"
        );
    }
    let mut p = s.project().clone();
    let json = crate::file::project_to_json(&p);
    assert_eq!(crate::file::project_from_json(&json).expect("load"), p);
    p.tracks[1].output = Some(4_242);
    let json = crate::file::project_to_json(&p);
    assert_eq!(
        crate::file::project_from_json(&json).expect("load").tracks[1].output,
        None
    );
}

#[test]
fn compressors_can_listen_to_another_track() {
    let mut s = Session::new(fixture());
    s.execute(Command::AddEffect {
        track_id: Some(2),
        kind: EffectKind::Compressor,
        index: None,
    })
    .expect("comp");
    let comp = s.project().tracks[1].mixer.effects[0].id;
    let command = Command::SetEffectSidechain {
        track_id: Some(2),
        effect_id: comp,
        source: Some(3),
    };
    let json = serde_json::to_string(&command).expect("json");
    assert_eq!(
        serde_json::from_str::<Command>(&json).expect("back"),
        command
    );
    s.execute(command).expect("sidechain");
    assert_eq!(s.project().tracks[1].mixer.effects[0].sidechain, Some(3));
    let file = crate::file::project_to_json(s.project());
    assert_eq!(
        &crate::file::project_from_json(&file).expect("load"),
        s.project()
    );
    s.undo();
    assert_eq!(s.project().tracks[1].mixer.effects[0].sidechain, None);
    // Only compressors, only real tracks.
    let reverb = s.project().tracks[0].mixer.effects[0].id;
    for bad in [
        Command::SetEffectSidechain {
            track_id: Some(1),
            effect_id: reverb,
            source: Some(3),
        },
        Command::SetEffectSidechain {
            track_id: Some(2),
            effect_id: comp,
            source: Some(999),
        },
    ] {
        assert!(s.execute(bad).is_err());
    }
}

#[test]
fn humanize_nudges_notes_repeatably_and_undoes() {
    let base = fixture();
    let clip = clip_id(&base);
    let humanize = |seed| Command::HumanizeNotes {
        clip_id: clip,
        timing_beats: 0.03,
        velocity: 12,
        seed,
        note_ids: None,
    };
    let json = serde_json::to_string(&humanize(7)).expect("json");
    assert_eq!(
        serde_json::from_str::<Command>(&json).expect("back"),
        humanize(7)
    );
    let mut a = base.clone();
    let undo = humanize(7).apply(&mut a).expect("humanize");
    let mut b = base.clone();
    humanize(7).apply(&mut b).expect("again");
    assert_eq!(a, b, "same seed, same result");
    let mut c = base.clone();
    humanize(8).apply(&mut c).expect("other seed");
    assert_ne!(a, c, "another seed varies");
    for (before, after) in base.tracks[0].clips[0]
        .notes
        .iter()
        .zip(&a.tracks[0].clips[0].notes)
    {
        assert!((before.start_beats - after.start_beats).abs() <= 0.03 + 1e-12);
        assert!(i32::from(before.velocity).abs_diff(i32::from(after.velocity)) <= 12);
    }
    undo.apply(&mut a).expect("undo");
    assert_eq!(a.tracks[0].clips[0].notes, base.tracks[0].clips[0].notes);
    let mut d = base.clone();
    assert!(
        Command::HumanizeNotes {
            clip_id: clip,
            timing_beats: 1.0,
            velocity: 0,
            seed: 1,
            note_ids: None,
        }
        .apply(&mut d)
        .is_err()
    );
}

#[test]
fn key_and_chord_track_round_trip_and_undo() {
    use crate::music::{ChordQuality, Key, Mode};
    let mut s = Session::new(fixture());
    let key = Key {
        tonic: 9,
        mode: Mode::Minor,
    };
    s.execute(Command::SetKey { key: Some(key) }).expect("key");
    for (beat, root, quality) in [
        (0.0, 9, ChordQuality::Minor),
        (4.0, 5, ChordQuality::Major),
        (2.0, 0, ChordQuality::Major7),
    ] {
        s.execute(Command::AddChord {
            start_beats: beat,
            root,
            quality,
            bass: None,
        })
        .expect("chord");
    }
    let p = s.project();
    assert_eq!(
        p.chords
            .iter()
            .map(crate::music::Chord::name)
            .collect::<Vec<_>>(),
        ["Am", "Cmaj7", "F"]
    );
    assert_eq!(
        p.chord_at(3.0).map(crate::music::Chord::name).as_deref(),
        Some("Cmaj7")
    );
    let summary = crate::summary::song_summary(p);
    assert_eq!(summary["key"]["name"], "A minor");
    assert_eq!(
        summary["chords"][1]["notes"],
        serde_json::json!(["C", "E", "G", "B"])
    );
    assert_eq!(summary["chords"][2]["end_beats"], 8.0);
    let first = p.chords[0].id;
    for command in [
        Command::SetKey { key: Some(key) },
        Command::SetChord {
            chord_id: first,
            root: 2,
            quality: ChordQuality::Minor7,
            bass: Some(5),
        },
        Command::MoveChord {
            chord_id: first,
            start_beats: 1.0,
        },
        Command::RemoveChord { chord_id: first },
        Command::RestoreChord {
            chord: p.chords[0].clone(),
        },
    ] {
        let json = serde_json::to_string(&command).expect("json");
        assert_eq!(
            serde_json::from_str::<Command>(&json).expect("back"),
            command,
            "{json}"
        );
    }
    s.execute(Command::SetChord {
        chord_id: first,
        root: 2,
        quality: ChordQuality::Minor7,
        bass: Some(5),
    })
    .expect("set");
    assert_eq!(s.project().chords[0].name(), "Dm7/F");
    s.undo();
    assert_eq!(s.project().chords[0].name(), "Am");
    s.execute(Command::RemoveChord { chord_id: first })
        .expect("remove");
    s.undo();
    assert_eq!(s.project().chords[0].id, first);
    let json = crate::file::project_to_json(s.project());
    assert_eq!(
        &crate::file::project_from_json(&json).expect("load"),
        s.project()
    );
    for bad in [
        Command::AddChord {
            start_beats: 4.0,
            root: 0,
            quality: ChordQuality::Major,
            bass: None,
        },
        Command::AddChord {
            start_beats: 6.0,
            root: 12,
            quality: ChordQuality::Major,
            bass: None,
        },
        Command::SetKey {
            key: Some(Key {
                tonic: 14,
                mode: Mode::Major,
            }),
        },
    ] {
        assert!(s.execute(bad).is_err());
    }
}

#[test]
fn comping_picks_the_heard_take_and_undoes() {
    let mut s = Session::new(fixture());
    // Two more takes over the fixture's 4-second recording (clip 11).
    for _ in 0..2 {
        let id = s.project().next_id.max(1);
        s.execute(Command::Batch {
            commands: vec![
                Command::AddAudioClip {
                    track_id: 10,
                    start_beats: 0.0,
                    audio: region("vox-0123abcd.wav", 4.0),
                    length_beats: None,
                    name: None,
                },
                Command::CompTake { clip_id: id },
            ],
        })
        .expect("take");
    }
    let clips = &s.project().tracks[3].clips;
    let muted: Vec<bool> = clips.iter().map(|c| c.muted).collect();
    // The newest take plays; the older ones are kept, muted.
    assert_eq!(muted, [true, true, false]);
    let first = clips[0].id;
    let before = s.project().clone();
    s.execute(Command::CompTake { clip_id: first })
        .expect("comp");
    let clips = &s.project().tracks[3].clips;
    assert_eq!(
        clips.iter().map(|c| c.muted).collect::<Vec<_>>(),
        [false, true, true]
    );
    let a = clips[0].audio.as_ref().expect("audio");
    assert!(a.fade_in_seconds >= 0.01 && a.fade_out_seconds >= 0.01);
    s.undo();
    assert_eq!(s.project(), &before);
    // Muting is its own command too, and round-trips.
    let mute = Command::SetClipMuted {
        clip_id: first,
        muted: false,
    };
    let json = serde_json::to_string(&mute).expect("json");
    assert_eq!(serde_json::from_str::<Command>(&json).expect("back"), mute);
    s.execute(mute).expect("unmute");
    assert!(!s.project().tracks[3].clips[0].muted);
    let file = crate::file::project_to_json(s.project());
    assert_eq!(
        &crate::file::project_from_json(&file).expect("load"),
        s.project()
    );
}

#[test]
fn swing_slider_drag_is_one_undo_step() {
    let mut s = Session::new(fixture());
    let clip = clip_id(s.project());
    for amount in [10.0, 30.0, 50.0] {
        s.execute(Command::SetClipSwing {
            clip_id: clip,
            swing: Some(crate::project::Swing {
                amount_percent: amount,
                grid_beats: 0.25,
            }),
        })
        .expect("swing");
    }
    s.end_gesture();
    s.undo();
    assert_eq!(s.project().tracks[0].clips[0].swing, None);
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
        Command::SetClipSwing {
            clip_id: clip,
            swing: Some(crate::project::Swing {
                amount_percent: 150.0,
                grid_beats: 0.25,
            }),
        },
        Command::SetClipSwing {
            clip_id: clip,
            swing: Some(crate::project::Swing {
                amount_percent: 50.0,
                grid_beats: 0.0,
            }),
        },
        Command::SetClipSwing {
            clip_id: AUDIO_CLIP,
            swing: Some(crate::project::Swing {
                amount_percent: 50.0,
                grid_beats: 0.25,
            }),
        },
        Command::AddNotes {
            clip_id: clip,
            notes: vec![NoteInput {
                chance: 0,
                ..note(42, 0.0, 0.25)
            }],
        },
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

#[test]
fn clips_that_follow_tempo_measure_the_file_in_their_own_tempo() {
    let mut p = fixture();
    Command::SetClipTempo {
        clip_id: AUDIO_CLIP,
        source_bpm: Some(120.0),
    }
    .apply(&mut p)
    .expect("follow");
    Command::SetTempo { bpm: 60.0 }
        .apply(&mut p)
        .expect("slower");
    // Still 8 beats long, and splitting at beat 2 is 1 s into the file
    // (the recording's 120 BPM), not 2 s (the song's 60 BPM).
    Command::SplitClip {
        clip_id: AUDIO_CLIP,
        at_beats: 2.0,
    }
    .apply(&mut p)
    .expect("split");
    let second = &p.track(10).expect("t").clips[1];
    assert!((second.audio.as_ref().expect("a").offset_seconds - 1.0).abs() < 1e-9);
    assert!(
        Command::SetClipTempo {
            clip_id: clip_id(&p),
            source_bpm: Some(120.0)
        }
        .apply(&mut p)
        .is_err(),
        "note clips can't be stretched"
    );
}
