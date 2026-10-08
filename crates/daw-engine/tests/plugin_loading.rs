//! In the app, new plugins load on the main thread in the background and
//! join the song when ready. This test plays the main thread itself.

#![allow(unsafe_code)]

use std::sync::Mutex;
use std::sync::mpsc;

use daw_engine::{Engine, PLUGIN_LOADING};
use daw_model::plugin::PluginRef;
use daw_model::{Command, Instrument, InstrumentKind, Project, Session};

type Job = Box<dyn FnOnce() + Send>;

#[test]
fn a_plugin_loads_in_the_background_and_then_plays() {
    let path = "in-process://later/NPT Test.vst3";
    // SAFETY: GetPluginFactory returns an owned factory reference.
    let module = unsafe {
        daw_plugins::Module::from_factory(
            daw_test_plugin::GetPluginFactory(),
            std::path::Path::new(path),
        )
    }
    .expect("factory");
    std::mem::forget(module);

    // This thread is the "main thread"; jobs wait until it runs them.
    let (tx, rx) = mpsc::channel::<Job>();
    let tx = Mutex::new(tx);
    daw_plugins::main_thread::install(move |job| {
        if let Ok(tx) = tx.lock() {
            let _ = tx.send(job);
        }
    });
    let pump = || {
        while let Ok(job) = rx.try_recv() {
            job();
        }
    };

    let mut s = Session::new(Project::default());
    let id = s.project().tracks[0].id;
    let (engine, mut processor) = Engine::new(s.project(), 48_000);
    engine.set_metronome(false);
    let loaded = std::sync::Arc::new(Mutex::new(Vec::new()));
    let seen = std::sync::Arc::clone(&loaded);
    engine.on_plugin_loaded(move |t| seen.lock().expect("lock").push(t));
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
                path: path.into(),
                params: [(0, 0.5)].into_iter().collect(),
                state: None,
            })),
        },
    })
    .expect("plugin");
    engine.sync(s.project());
    // Not loaded yet: the track is silent and says so.
    assert!(matches!(engine.plugin(id), Some(Err(e)) if e == PLUGIN_LOADING));
    // A louder setting made while it loads isn't lost.
    s.execute(Command::SetPluginParams {
        track_id: Some(id),
        effect_id: None,
        params: [(0, 1.0)].into_iter().collect(),
    })
    .expect("param");
    engine.sync(s.project());

    pump();
    assert_eq!(
        *loaded.lock().expect("lock"),
        vec![id],
        "the app hears about it"
    );
    let instance = engine.plugin(id).expect("plugin").expect("loaded");
    assert_eq!(instance.params().expect("params")[0].display, "100");
    let mut out = vec![0.0f32; 4800 * 2];
    processor.process_interleaved(&mut out[..256], 2); // takes the new instrument
    engine.note_on(id, 69, 1.0);
    for chunk in out.chunks_mut(512) {
        processor.process_interleaved(chunk, 2);
    }
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak > 0.4, "plays at the louder level: {peak}");
}
