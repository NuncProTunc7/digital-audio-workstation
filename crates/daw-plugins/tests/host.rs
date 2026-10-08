//! Hosting the test plugin (crates/daw-test-plugin): in this process, and
//! as a real `.vst3` file on disk.

#![allow(unsafe_code)]

use std::path::{Path, PathBuf};

use daw_plugins::instance::Setup;
use daw_plugins::{Instance, Module, PluginInfo, PluginKind};

const SR: f64 = 48_000.0;
const SETUP: Setup = Setup {
    sample_rate_hz: SR,
    max_block: 256,
    offline: true,
};

/// The test plugin, linked into this test (no file needed).
fn in_process() -> Vec<PluginInfo> {
    let path = Path::new("in-process://NPT Test.vst3");
    // SAFETY: GetPluginFactory returns an owned factory reference.
    let module = unsafe { Module::from_factory(daw_test_plugin::GetPluginFactory(), path) }
        .expect("factory");
    let plugins = module.plugins().expect("plugins");
    // Keep the module cached for Instance::create.
    std::mem::forget(module);
    plugins
}

fn find(plugins: &[PluginInfo], name: &str) -> PluginInfo {
    plugins
        .iter()
        .find(|p| p.name == name)
        .cloned()
        .expect(name)
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, s| m.max(s.abs()))
}

/// Renders `blocks` blocks of 128 samples.
fn render(p: &mut daw_plugins::PluginProcessor, blocks: usize) -> Vec<f32> {
    let mut out = Vec::new();
    for _ in 0..blocks {
        let mut l = [0.0f32; 128];
        let mut r = [0.0f32; 128];
        p.process(&mut l, &mut r);
        assert_eq!(l, r, "the test synth is mono in stereo");
        out.extend_from_slice(&l);
    }
    out
}

#[test]
fn the_factory_lists_an_instrument_and_an_effect() {
    let plugins = in_process();
    assert_eq!(plugins.len(), 2, "controllers aren't listed: {plugins:?}");
    let synth = find(&plugins, "NPT Test Synth");
    assert_eq!(synth.kind, PluginKind::Instrument);
    assert_eq!(synth.vendor, "Nunc Pro Tune");
    assert_eq!(synth.uid, "4E50543153594E544800000000000001");
    assert_eq!(find(&plugins, "NPT Test Gain").kind, PluginKind::Effect);
}

#[test]
fn an_instrument_plays_notes_and_its_settings_survive_saving() {
    let synth = find(&in_process(), "NPT Test Synth");
    let (instance, mut rt) = Instance::create(&synth, SETUP, None).expect("create");

    assert!(
        peak(&render(&mut rt, 4)) == 0.0,
        "silent until a note plays"
    );
    rt.note_on(69, 1.0);
    let loud = peak(&render(&mut rt, 8));
    assert!(
        loud > 0.2 && loud <= 0.26,
        "default level plays a sine: {loud}"
    );

    // Parameters: listed with the plugin's own display text; changes reach
    // the sound on the next block.
    let params = instance.params().expect("params");
    assert_eq!(params.len(), 1);
    assert_eq!(
        (params[0].name.as_str(), params[0].display.as_str()),
        ("Level", "50")
    );
    rt.set_param(0, 1.0);
    let louder = peak(&render(&mut rt, 8));
    assert!(louder > loud * 1.8, "{louder} vs {loud}");
    rt.note_off(69);
    assert_eq!(peak(&render(&mut rt, 2)), 0.0, "note off stops it");

    // Its state carries the level into a fresh copy.
    let state = instance.state().expect("state");
    let (copy, mut rt2) = Instance::create(&synth, SETUP, Some(state)).expect("restore");
    rt2.note_on(69, 1.0);
    let restored = peak(&render(&mut rt2, 8));
    assert!((restored - louder).abs() < 0.01, "{restored} vs {louder}");
    assert_eq!(copy.params().expect("params")[0].display, "100");

    // Held notes are released together.
    rt2.note_on(60, 1.0);
    rt2.all_notes_off();
    assert_eq!(peak(&render(&mut rt2, 2)), 0.0);
}

#[test]
fn an_effect_processes_in_place() {
    let gain = find(&in_process(), "NPT Test Gain");
    let (_instance, mut rt) = Instance::create(&gain, SETUP, None).expect("create");
    let mut l = vec![0.25f32; 1000];
    let mut r = vec![-0.25f32; 1000];
    // Longer than max_block: processed in pieces.
    rt.process(&mut l, &mut r);
    assert!(
        l.iter().all(|&s| (s - 0.25).abs() < 1e-6),
        "0.5 = unchanged"
    );
    assert!(r.iter().all(|&s| (s + 0.25).abs() < 1e-6));
    rt.set_param(0, 1.0);
    rt.process(&mut l, &mut r);
    assert!((l[999] - 0.5).abs() < 1e-6, "1.0 doubles: {}", l[999]);
}

/// Builds the test plugin as a real library, in its own target folder so
/// it doesn't wait on the build running these tests.
fn build_plugin_file(dest: &Path) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let target = root.join("target").join("test-plugin");
    let status = std::process::Command::new(env!("CARGO"))
        .args(["build", "-q", "-p", "daw-test-plugin", "--target-dir"])
        .arg(&target)
        .current_dir(&root)
        .status()
        .expect("cargo");
    assert!(status.success(), "building the test plugin failed");
    let built = if cfg!(windows) {
        target.join("debug").join("daw_test_plugin.dll")
    } else {
        target.join("debug").join("libdaw_test_plugin.so")
    };
    let file = dest.join("NPT Test.vst3");
    std::fs::copy(&built, &file).expect("copy plugin");
    file
}

#[test]
fn a_plugin_file_is_found_loaded_and_played() {
    let dir = tempfile::tempdir().expect("tmp");
    let folder = dir.path().join("VST3");
    std::fs::create_dir_all(&folder).expect("dir");
    build_plugin_file(&folder);

    let cache = daw_plugins::scan::rescan(
        &[folder],
        &daw_plugins::scan::ScanCache::default(),
        daw_plugins::scan::scan_in_process,
    );
    assert!(cache.failures().is_empty(), "{:?}", cache.failures());
    let plugins = cache.plugins();
    assert_eq!(plugins.len(), 2);
    let synth = find(&plugins, "NPT Test Synth");
    let (_instance, mut rt) = Instance::create(&synth, SETUP, None).expect("create from file");
    rt.note_on(60, 0.5);
    assert!(peak(&render(&mut rt, 4)) > 0.05);
}

#[test]
fn a_rebuilt_handle_keeps_the_plugin_and_saved_params_apply() {
    let synth = find(&in_process(), "NPT Test Synth");
    let (instance, mut rt) = Instance::create(&synth, SETUP, None).expect("create");
    instance.apply_params(&mut rt, &[(0, 1.0)]).expect("params");
    assert_eq!(instance.params().expect("params")[0].display, "100");
    rt.note_on(69, 1.0);
    let before = peak(&render(&mut rt, 4));
    // The engine rebuilt its tracks: a new handle, same plugin and level,
    // and whatever was sounding is released.
    drop(rt);
    let mut rt2 = instance.processor();
    assert_eq!(peak(&render(&mut rt2, 2)), 0.0, "notes released");
    rt2.note_on(69, 1.0);
    let after = peak(&render(&mut rt2, 4));
    assert!((after - before).abs() < 0.01, "{after} vs {before}");
}
