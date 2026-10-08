//! Third-party (VST3) effects, run through `daw-plugins`.

use daw_model::effect::Effect;
use daw_plugins::song::{PluginLoad, start};
use daw_plugins::{Instance, PluginKind, PluginProcessor};

use crate::EffectProcessor;

/// Builds a plugin effect: a new audio handle for `reuse` (the same plugin
/// already running, so it keeps its state), or a fresh instance from the
/// song. A plugin that can't load passes sound through unchanged; the
/// error comes back with it.
pub fn create_plugin(
    effect: &Effect,
    sample_rate_hz: f32,
    reuse: Option<&Instance>,
) -> (Box<dyn EffectProcessor>, Option<PluginLoad>) {
    let Some(p) = &effect.plugin else {
        return (passthrough(), None);
    };
    match start(p, PluginKind::Effect, sample_rate_hz, reuse) {
        Ok((instance, rt)) => (Box::new(PluginEffect(rt)), Some(Ok(instance))),
        Err(e) => (passthrough(), Some(Err(e))),
    }
}

/// An effect that leaves sound unchanged (a stand-in while a plugin loads).
pub fn passthrough() -> Box<dyn EffectProcessor> {
    Box::new(Passthrough)
}

struct Passthrough;

impl EffectProcessor for Passthrough {
    // RT-SAFE
    fn set_param(&mut self, _index: usize, _value: f32) {}
    // RT-SAFE
    fn reset(&mut self) {}
    // RT-SAFE
    fn process(&mut self, _left: &mut [f32], _right: &mut [f32]) {}
}

struct PluginEffect(PluginProcessor);

impl EffectProcessor for PluginEffect {
    // RT-SAFE
    fn set_param(&mut self, _index: usize, _value: f32) {}
    // RT-SAFE
    fn set_tempo(&mut self, bpm: f32) {
        self.0.set_tempo(f64::from(bpm));
    }
    // RT-SAFE
    fn reset(&mut self) {}
    // RT-SAFE
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.0.process(left, right);
    }
    // RT-SAFE
    fn set_plugin_param(&mut self, id: u32, value: f64) {
        self.0.set_param(id, value);
    }
}
