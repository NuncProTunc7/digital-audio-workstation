//! Third-party (VST3) plugins on this computer, and the work around them
//! that touches files and live plugins: finding them, loading one onto a
//! track, reading its parameters, turning edits made in the plugin's own
//! window into Commands, and saving its settings with the song.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use daw_model::plugin::PluginRef;
use daw_model::{Command, Instrument, InstrumentKind, TrackId};
use daw_plugins::scan::{ScanCache, rescan as rescan_folders};
use daw_plugins::{Edit, Instance, ParamInfo, PluginInfo, PluginKind};

use crate::discovery::APP_ID;
use crate::host::Host;

/// `%APPDATA%\io.github.nuncprotunc7.nuncprotune\plugins.json` on Windows.
pub fn cache_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_ID)
        .join("plugins.json")
}

/// The remembered plugin list (empty until the first scan).
pub fn load_cache(path: &Path) -> ScanCache {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Looks through the plugin folders again (only new or changed plugins
/// are opened), and remembers the result.
pub fn rescan<H: Host>(host: &H) -> ScanCache {
    let path = host.plugin_cache_path();
    let old = load_cache(&path);
    let folders = host.plugin_folders();
    let cache = match host.plugin_scanner() {
        Some(exe) => rescan_folders(&folders, &old, |m| {
            daw_plugins::scan::scan_in_child(&exe, m)
        }),
        None => rescan_folders(&folders, &old, daw_plugins::scan::scan_in_process),
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(&cache) {
        let _ = std::fs::write(&path, text);
    }
    for (module, error) in cache.failures() {
        let name = Path::new(&module)
            .file_name()
            .map_or(module.clone(), |n| n.to_string_lossy().into_owned());
        crate::diagnostics::log(&format!("Plugin {name} can't be used: {error}"));
    }
    cache
}

/// A plugin by its id, from the remembered list.
fn find<H: Host>(host: &H, uid: &str) -> Result<PluginInfo, String> {
    let cache = load_cache(&host.plugin_cache_path());
    cache
        .plugins()
        .into_iter()
        .find(|p| p.uid.eq_ignore_ascii_case(uid))
        .ok_or_else(|| format!("no installed plugin has id {uid}; list them with plugins"))
}

/// The live plugin on a track (after the engine built it).
pub fn live<H: Host>(host: &H, track_id: TrackId) -> Result<Instance, String> {
    let engine = host
        .engine()
        .ok_or("audio isn't running, so plugins aren't loaded")?;
    match engine.plugin(track_id) {
        Some(Ok(instance)) => Ok(instance),
        Some(Err(e)) => Err(format!("the plugin couldn't load: {e}")),
        None => Err("that track doesn't play a plugin".into()),
    }
}

/// Gives a track an installed plugin instrument (one undo step), then
/// records the plugin's parameter list in the song.
pub fn load_plugin<H: Host>(host: &H, track_id: TrackId, uid: &str) -> Result<PluginInfo, String> {
    let info = find(host, uid)?;
    if info.kind != PluginKind::Instrument {
        return Err(format!(
            "{} is an effect, not an instrument; plugin effects come later",
            info.name
        ));
    }
    let instrument = Instrument {
        kind: InstrumentKind::Plugin,
        preset: info.name.clone(),
        params: BTreeMap::new(),
        sample_pack: None,
        plugin: Some(Box::new(PluginRef {
            uid: info.uid.clone(),
            name: info.name.clone(),
            vendor: info.vendor.clone(),
            path: info.path.clone(),
            params: BTreeMap::new(),
            state: None,
        })),
    };
    host.session()?
        .execute(Command::SetInstrument {
            track_id,
            instrument,
        })
        .map_err(|e| e.to_string())?;
    host.project_changed(&format!("Load plugin {}", info.name));
    note_live(host, track_id);
    Ok(info)
}

/// Copies a live plugin's parameter list (and anything new in it) into
/// the song, without an undo step.
pub fn note_live<H: Host>(host: &H, track_id: TrackId) {
    let Ok(instance) = live(host, track_id) else {
        return;
    };
    let Ok(values) = instance.values() else {
        return;
    };
    let params: BTreeMap<u32, f64> = values.into_iter().collect();
    if let Ok(mut s) = host.session() {
        s.note_plugin(track_id, &params, None);
    }
}

/// Before saving or rendering: stores every live plugin's own settings in
/// the song, so the file (and offline renders) sound like what's playing.
pub fn store_states<H: Host>(host: &H) {
    let Some(engine) = host.engine() else {
        return;
    };
    let tracks: Vec<TrackId> = match host.session() {
        Ok(s) => s
            .project()
            .tracks
            .iter()
            .filter(|t| t.instrument.plugin.is_some())
            .map(|t| t.id)
            .collect(),
        Err(_) => return,
    };
    for id in tracks {
        let Some(Ok(instance)) = engine.plugin(id) else {
            continue;
        };
        let Ok(state) = instance.state() else {
            continue;
        };
        let encoded = base64::engine::general_purpose::STANDARD.encode(state);
        if let Ok(mut s) = host.session() {
            s.note_plugin(id, &BTreeMap::new(), Some(encoded));
        }
    }
}

/// A plugin track's parameters with names, values and display text.
pub fn params<H: Host>(host: &H, track_id: TrackId) -> Result<Vec<ParamInfo>, String> {
    live(host, track_id)?.params()
}

/// Turns an edit made in a plugin's own window into the matching Command,
/// so it's undoable and Claude sees it. A whole drag is one undo step.
pub fn apply_edit<H: Host>(host: &H, track_id: TrackId, edit: Edit) -> Result<(), String> {
    match edit {
        Edit::Begin(_) => Ok(()),
        Edit::Perform(id, value) => {
            {
                let mut s = host.session()?;
                let known = s
                    .project()
                    .track(track_id)
                    .and_then(|t| t.instrument.plugin.as_ref())
                    .is_some_and(|p| p.params.contains_key(&id));
                if !known {
                    // A parameter the plugin added since loading.
                    s.note_plugin(track_id, &[(id, value)].into_iter().collect(), None);
                }
                s.execute(Command::SetPluginParams {
                    track_id,
                    effect_id: None,
                    params: [(id, value.clamp(0.0, 1.0))].into_iter().collect(),
                })
                .map_err(|e| e.to_string())?;
            }
            host.project_changed("Plugin setting");
            Ok(())
        }
        Edit::End(_) => {
            host.session()?.end_gesture();
            Ok(())
        }
        Edit::Restart => {
            // The plugin changed many things at once (e.g. picked one of its
            // presets): record the differences as one undo step.
            let instance = live(host, track_id)?;
            let values: BTreeMap<u32, f64> = instance.values()?.into_iter().collect();
            {
                let mut s = host.session()?;
                s.note_plugin(track_id, &values, None);
                let current = s
                    .project()
                    .track(track_id)
                    .and_then(|t| t.instrument.plugin.as_ref())
                    .map(|p| p.params.clone())
                    .unwrap_or_default();
                let changed: BTreeMap<u32, f64> = values
                    .into_iter()
                    .filter(|(id, v)| current.get(id).is_some_and(|c| (c - v).abs() > 1e-9))
                    .collect();
                if changed.is_empty() {
                    return Ok(());
                }
                s.end_gesture();
                s.execute(Command::SetPluginParams {
                    track_id,
                    effect_id: None,
                    params: changed,
                })
                .map_err(|e| e.to_string())?;
                s.end_gesture();
            }
            host.project_changed("Plugin preset");
            Ok(())
        }
    }
}

/// Points every live plugin's window edits at `send`. Call after the
/// engine syncs (new plugins appear then); edits must be applied off the
/// plugin's own call, e.g. by a thread reading the other end.
pub fn connect_edits<H: Host>(host: &H, send: &std::sync::mpsc::Sender<(TrackId, Edit)>) {
    let Some(engine) = host.engine() else {
        return;
    };
    let Ok(s) = host.session() else {
        return;
    };
    for t in s
        .project()
        .tracks
        .iter()
        .filter(|t| t.instrument.plugin.is_some())
    {
        if let Some(Ok(instance)) = engine.plugin(t.id) {
            let tx = send.clone();
            let id = t.id;
            let tx = std::sync::Mutex::new(tx);
            instance.on_edit(move |e| {
                if let Ok(tx) = tx.lock() {
                    let _ = tx.send((id, e));
                }
            });
        }
    }
}
