//! Third-party (VST3) instruments, played through `daw-plugins`.

use base64::Engine as _;
use daw_model::Instrument;
use daw_model::plugin::PluginRef;
use daw_plugins::instance::Setup;
use daw_plugins::{Instance, PluginInfo, PluginKind, PluginProcessor};

use crate::{InstrumentProcessor, Silent};

/// Largest block a plugin is handed at once (longer blocks are split).
const MAX_BLOCK: usize = 1024;

/// What happened when a plugin track was built: the live plugin (to show
/// its window and read its settings), or why it couldn't load.
pub type PluginLoad = Result<Instance, String>;

/// The `daw-plugins` description of a song's plugin.
pub fn plugin_info(p: &PluginRef, kind: PluginKind) -> PluginInfo {
    PluginInfo {
        uid: p.uid.clone(),
        name: p.name.clone(),
        vendor: p.vendor.clone(),
        version: String::new(),
        kind,
        categories: String::new(),
        path: p.path.clone(),
    }
}

/// Builds a plugin instrument: a new audio handle for `reuse` (the same
/// plugin already playing, so it keeps its sound), or a fresh instance from
/// the song's saved settings. A plugin that can't load plays silence; the
/// error comes back with it.
pub fn create_plugin(
    instrument: &Instrument,
    sample_rate_hz: f32,
    reuse: Option<&Instance>,
) -> (Box<dyn InstrumentProcessor>, Option<PluginLoad>) {
    let Some(p) = &instrument.plugin else {
        return (Box::new(Silent), None);
    };
    if let Some(instance) = reuse {
        let rt = instance.processor();
        return (Box::new(PluginInstrument(rt)), Some(Ok(instance.clone())));
    }
    let setup = Setup {
        sample_rate_hz: f64::from(sample_rate_hz),
        max_block: MAX_BLOCK,
        offline: false,
    };
    let state = p
        .state
        .as_deref()
        .and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok());
    let params: Vec<(u32, f64)> = p.params.iter().map(|(k, v)| (*k, *v)).collect();
    let made = Instance::create(&plugin_info(p, PluginKind::Instrument), setup, state).and_then(
        |(instance, mut rt)| {
            instance.apply_params(&mut rt, &params)?;
            Ok((instance, rt))
        },
    );
    match made {
        Ok((instance, rt)) => (Box::new(PluginInstrument(rt)), Some(Ok(instance))),
        Err(e) => (Box::new(Silent), Some(Err(format!("{}: {e}", p.name)))),
    }
}

struct PluginInstrument(PluginProcessor);

impl InstrumentProcessor for PluginInstrument {
    // RT-SAFE
    fn note_on(&mut self, note: u8, velocity: f32) {
        self.0.note_on(note, velocity);
    }
    // RT-SAFE
    fn note_off(&mut self, note: u8) {
        self.0.note_off(note);
    }
    // RT-SAFE
    fn all_notes_off(&mut self) {
        self.0.all_notes_off();
    }
    // RT-SAFE
    fn set_param(&mut self, _index: usize, _value: f32) {}
    // RT-SAFE
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.0.process(left, right);
    }
    // RT-SAFE
    fn set_plugin_param(&mut self, id: u32, value: f64) {
        self.0.set_param(id, value);
    }
    // RT-SAFE
    fn set_tempo(&mut self, bpm: f64) {
        self.0.set_tempo(bpm);
    }
}
