//! The app's window toolkit (Tauri) runs a job posted from the main thread
//! right away instead of queueing it. Choosing a plugin in the window syncs
//! the engine on the main thread; the plugin's background load must still
//! wait until the sync is over, or it waits for the sync's own lock and
//! the app freezes (found with Surge XT, 2026-10-09).

#![allow(unsafe_code)]

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use daw_engine::{Engine, PLUGIN_LOADING};
use daw_model::plugin::PluginRef;
use daw_model::{Command, Instrument, InstrumentKind, Project, Session};

type Job = Box<dyn FnOnce() + Send>;

#[test]
fn choosing_a_plugin_on_the_main_thread_does_not_freeze() {
    let path = "in-process://main-thread/NPT Test.vst3";
    // SAFETY: GetPluginFactory returns an owned factory reference.
    let module = unsafe {
        daw_plugins::Module::from_factory(
            daw_test_plugin::GetPluginFactory(),
            std::path::Path::new(path),
        )
    }
    .expect("factory");
    std::mem::forget(module);

    let (done_tx, done_rx) = mpsc::channel::<Result<(), String>>();
    // This thread plays the app's main thread.
    std::thread::spawn(move || {
        let main = std::thread::current().id();
        let (tx, rx) = mpsc::channel::<Job>();
        let tx = Mutex::new(tx);
        // Like Tauri's run_on_main_thread: inline on the main thread,
        // queued from anywhere else.
        daw_plugins::main_thread::install(move |job| {
            if std::thread::current().id() == main {
                job();
            } else if let Ok(tx) = tx.lock() {
                let _ = tx.send(job);
            }
        });

        let mut s = Session::new(Project::default());
        let id = s.project().tracks[0].id;
        let (engine, _processor) = Engine::new(s.project(), 48_000);
        let loaded = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&loaded);
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
                    params: Default::default(),
                    state: None,
                })),
            },
        })
        .expect("plugin");
        // What the Plugin menu does, on the main thread.
        engine.sync(s.project());
        if !matches!(engine.plugin(id), Some(Err(e)) if e == PLUGIN_LOADING) {
            let _ = done_tx.send(Err("should still be loading after the sync".into()));
            return;
        }
        // The main thread's event loop runs the queued load afterwards.
        for _ in 0..200 {
            while let Ok(job) = rx.try_recv() {
                job();
            }
            if !loaded.lock().expect("lock").is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let ready = matches!(engine.plugin(id), Some(Ok(_)));
        let _ = done_tx.send(if ready {
            Ok(())
        } else {
            Err("the plugin never finished loading".into())
        });
    });
    match done_rx.recv_timeout(Duration::from_secs(20)) {
        Ok(result) => result.expect("loads"),
        Err(_) => panic!("the main thread froze while a plugin was chosen"),
    }
}
