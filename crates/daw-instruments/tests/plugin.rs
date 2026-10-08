//! Plugin instruments, using the in-process test plugin.

#![allow(unsafe_code)]

use std::path::Path;

use daw_model::plugin::PluginRef;
use daw_model::{Instrument, InstrumentKind};

const PATH: &str = "in-process://instruments/NPT Test.vst3";

fn instrument(params: &[(u32, f64)]) -> Instrument {
    // SAFETY: GetPluginFactory returns an owned factory reference.
    let module = unsafe {
        daw_plugins::Module::from_factory(daw_test_plugin::GetPluginFactory(), Path::new(PATH))
    }
    .expect("factory");
    std::mem::forget(module);
    Instrument {
        kind: InstrumentKind::Plugin,
        preset: String::new(),
        params: Default::default(),
        sample_pack: None,
        plugin: Some(Box::new(PluginRef {
            uid: "4E50543153594E544800000000000001".into(),
            name: "NPT Test Synth".into(),
            vendor: "Nunc Pro Tune".into(),
            path: PATH.into(),
            params: params.iter().copied().collect(),
            state: None,
        })),
    }
}

fn peak_after_note(inst: &mut dyn daw_instruments::InstrumentProcessor) -> f32 {
    inst.note_on(69, 1.0);
    let (mut l, mut r) = (vec![0.0f32; 4800], vec![0.0f32; 4800]);
    for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
        inst.process(a, b);
    }
    assert!(l.iter().all(|s| s.is_finite()));
    l.iter().fold(0.0, |m, s| m.max(s.abs()))
}

#[test]
fn a_plugin_track_plays_with_the_songs_settings() {
    let quiet = peak_after_note(&mut *daw_instruments::create(
        &instrument(&[(0, 0.5)]),
        48_000.0,
    ));
    let loud = peak_after_note(&mut *daw_instruments::create(
        &instrument(&[(0, 1.0)]),
        48_000.0,
    ));
    assert!(quiet > 0.2, "{quiet}");
    assert!(
        loud > quiet * 1.8,
        "the song's level applies: {loud} vs {quiet}"
    );
}

#[test]
fn a_missing_plugin_is_silent_and_says_why() {
    let mut inst = instrument(&[]);
    if let Some(p) = &mut inst.plugin {
        p.path = "C:/nowhere/Gone.vst3".into();
    }
    let (mut proc_, load) = daw_instruments::create_plugin(&inst, 48_000.0, None);
    assert!(matches!(load, Some(Err(e)) if e.contains("NPT Test Synth")));
    assert_eq!(peak_after_note(&mut *proc_), 0.0);
}

#[test]
fn a_rebuilt_track_reuses_the_live_plugin() {
    let inst = instrument(&[(0, 1.0)]);
    let (_first, load) = daw_instruments::create_plugin(&inst, 48_000.0, None);
    let live = load.expect("plugin").expect("loaded");
    // Settings changed live since (as from the plugin's window), not in
    // the song: a reused plugin keeps them.
    let mut changed = inst.clone();
    if let Some(p) = &mut changed.plugin {
        p.params.insert(0, 0.1);
    }
    let (mut again, load2) = daw_instruments::create_plugin(&changed, 48_000.0, Some(&live));
    assert!(load2.is_some_and(|l| l.is_ok()));
    assert!(peak_after_note(&mut *again) > 0.4, "still at full level");
}
