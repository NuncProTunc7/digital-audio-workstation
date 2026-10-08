//! Third-party (VST3) instruments, played through `daw-plugins`.

use daw_model::Instrument;
use daw_plugins::song::{PluginLoad, start};
use daw_plugins::{Instance, PluginKind, PluginProcessor};

use crate::{InstrumentProcessor, Silent};

/// Builds a plugin instrument (see [`daw_plugins::song::start`]). A plugin that can't
/// load plays silence; the error comes back with it.
pub fn create_plugin(
    instrument: &Instrument,
    sample_rate_hz: f32,
    reuse: Option<&Instance>,
) -> (Box<dyn InstrumentProcessor>, Option<PluginLoad>) {
    let Some(p) = &instrument.plugin else {
        return (Box::new(Silent), None);
    };
    match start(p, PluginKind::Instrument, sample_rate_hz, reuse) {
        Ok((instance, rt)) => (Box::new(PluginInstrument(rt)), Some(Ok(instance))),
        Err(e) => (Box::new(Silent), Some(Err(e))),
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
