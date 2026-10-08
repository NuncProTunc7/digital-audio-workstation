//! Plugin instruments in the engine, using the in-process test plugin.

#![allow(unsafe_code)]

use daw_engine::Engine;
use daw_model::plugin::PluginRef;
use daw_model::{Command, Instrument, InstrumentKind, NoteInput, Project, Session};

const SR: u32 = 48_000;
const PATH: &str = "in-process://engine/NPT Test.vst3";

fn plugin_song() -> (Session, daw_model::TrackId) {
    // SAFETY: GetPluginFactory returns an owned factory reference.
    let module = unsafe {
        daw_plugins::Module::from_factory(
            daw_test_plugin::GetPluginFactory(),
            std::path::Path::new(PATH),
        )
    }
    .expect("factory");
    std::mem::forget(module);
    let mut s = Session::new(Project::default());
    let id = s.project().tracks[0].id;
    s.execute(Command::SetInstrument {
        track_id: id,
        instrument: Instrument {
            kind: InstrumentKind::Plugin,
            preset: String::new(),
            params: Default::default(),
            sample_pack: None,
            plugin: Some(Box::new(PluginRef {
                uid: "4E50543153594E544800000000000001".into(),
                name: "NPT Test Synth".into(),
                vendor: "Nunc Pro Tune".into(),
                path: PATH.into(),
                params: [(0, 0.5)].into_iter().collect(),
                state: None,
            })),
        },
    })
    .expect("plugin");
    s.execute(Command::CreateClip {
        track_id: id,
        start_beats: 0.0,
        length_beats: 4.0,
        name: None,
        notes: vec![NoteInput {
            chance: 100,
            pitch: 69,
            start_beats: 0.0,
            length_beats: 4.0,
            velocity: 127,
            id: None,
        }],
    })
    .expect("clip");
    // Only the plugin track sounds.
    for t in 1..s.project().tracks.len() {
        let other = s.project().tracks[t].id;
        s.execute(Command::SetTrackMixer {
            track_id: other,
            volume_db: None,
            pan: None,
            mute: Some(true),
            solo: None,
        })
        .expect("mute");
    }
    (s, id)
}

fn peak(p: &mut daw_engine::AudioProcessor, seconds: f32) -> f32 {
    let mut out = vec![0.0f32; (seconds * SR as f32) as usize * 2];
    for chunk in out.chunks_mut(512) {
        p.process_interleaved(chunk, 2);
    }
    assert!(out.iter().all(|s| s.is_finite()));
    out.iter().fold(0.0, |m, s| m.max(s.abs()))
}

#[test]
fn a_plugin_track_plays_and_keeps_its_plugin_when_the_song_changes() {
    let (mut session, id) = plugin_song();
    let (engine, mut processor) = Engine::new(session.project(), SR);
    // Only the plugin: no metronome clicks in the measurement.
    engine.set_metronome(false);
    let first = engine.plugin(id).expect("plugin track").expect("loaded");
    engine.play();
    let a = peak(&mut processor, 0.3);
    assert!(
        (a - 0.25).abs() < 0.01,
        "the plugin plays the clip once: {a}"
    );

    // Adding a track rebuilds the engine's tracks; the plugin stays.
    session
        .execute(Command::AddTrack {
            name: "Pad".into(),
            instrument: InstrumentKind::Synth,
            preset: None,
            index: None,
        })
        .expect("add");
    engine.sync(session.project());
    let after = engine.plugin(id).expect("still").expect("loaded");
    assert!(after.same_as(&first), "same running plugin");

    // A parameter change (from Claude, undo, or the plugin's window) is
    // heard and shown in the plugin.
    session
        .execute(Command::SetPluginParams {
            track_id: id,
            effect_id: None,
            params: [(0, 1.0)].into_iter().collect(),
        })
        .expect("louder");
    engine.sync(session.project());
    engine.locate(0.0);
    let b = peak(&mut processor, 0.3);
    assert!(b > a * 1.6, "louder: {b} vs {a}");
    let shown = after.params().expect("params");
    assert_eq!(shown[0].display, "100");
}

#[test]
fn offline_renders_use_the_songs_plugin_settings() {
    let (session, _) = plugin_song();
    let quiet = daw_engine::offline::render_song(
        session.project(),
        &daw_engine::AudioPool::in_temp_dir(),
        SR,
        0.5,
    );
    let mut louder = session.project().clone();
    if let Some(p) = &mut louder.tracks[0].instrument.plugin {
        p.params.insert(0, 1.0);
    }
    let loud =
        daw_engine::offline::render_song(&louder, &daw_engine::AudioPool::in_temp_dir(), SR, 0.5);
    let pk = |x: &[f32]| x.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(pk(&quiet) > 0.05);
    assert!(pk(&loud) > pk(&quiet) * 1.6);
}
