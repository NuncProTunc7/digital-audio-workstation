//! Starting the plugins a song uses (instruments and effects alike).

use daw_model::plugin::PluginRef;

use crate::instance::Setup;
use crate::{Instance, PluginInfo, PluginKind, PluginProcessor};

/// Largest block a plugin is handed at once (longer blocks are split).
pub const MAX_BLOCK: usize = 1024;

/// What happened when a song's plugin was started: the live plugin (to
/// show its window and read its settings), or why it couldn't load.
pub type PluginLoad = Result<Instance, String>;

/// The description of a song's plugin.
pub fn info(p: &PluginRef, kind: PluginKind) -> PluginInfo {
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

/// Starts a song's plugin: a new audio handle for `reuse` (the same plugin
/// already playing, so it keeps its sound), or a fresh instance from the
/// song's saved settings and parameter values.
pub fn start(
    p: &PluginRef,
    kind: PluginKind,
    sample_rate_hz: f32,
    reuse: Option<&Instance>,
) -> Result<(Instance, PluginProcessor), String> {
    if let Some(instance) = reuse {
        return Ok((instance.clone(), instance.processor()));
    }
    let setup = Setup {
        sample_rate_hz: f64::from(sample_rate_hz),
        max_block: MAX_BLOCK,
        offline: false,
    };
    let params: Vec<(u32, f64)> = p.params.iter().map(|(k, v)| (*k, *v)).collect();
    Instance::start(&info(p, kind), setup, p.state.as_deref(), &params)
        .map_err(|e| format!("{}: {e}", p.name))
}
