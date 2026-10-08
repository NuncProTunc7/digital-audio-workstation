//! Third-party (VST3) plugins on this computer, and the work around them
//! that touches files and live plugins: finding them, loading one onto a
//! track or into an effect chain, reading its parameters, turning edits
//! made in the plugin's own window into Commands, and saving its settings
//! with the song.
//!
//! A plugin is named by an id: its track's id for a plugin instrument, its
//! effect id for a plugin effect (ids never repeat within a song).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use daw_model::plugin::PluginRef;
use daw_model::{Command, EffectId, Id, Instrument, InstrumentKind, Project, TrackId};
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

fn song_ref(info: &PluginInfo) -> Box<PluginRef> {
    Box::new(PluginRef {
        uid: info.uid.clone(),
        name: info.name.clone(),
        vendor: info.vendor.clone(),
        path: info.path.clone(),
        params: BTreeMap::new(),
        state: None,
    })
}

/// The running plugin with this id: a track's plugin instrument (track id)
/// or a plugin effect (effect id).
pub fn live<H: Host>(host: &H, id: Id) -> Result<Instance, String> {
    let engine = host
        .engine()
        .ok_or("audio isn't running, so plugins aren't loaded")?;
    match engine.plugin(id) {
        Some(Ok(instance)) => Ok(instance),
        Some(Err(e)) if e == daw_engine::PLUGIN_LOADING => {
            Err("the plugin is still loading; try again in a moment".into())
        }
        Some(Err(e)) => Err(format!("the plugin couldn't load: {e}")),
        None => Err(format!("{id} isn't a plugin track or a plugin effect")),
    }
}

/// Where a plugin is, as `SetPluginParams` addresses it: (track or bus,
/// effect). An instrument is (its track, None); a master effect is
/// (None, its id).
fn address(project: &Project, id: Id) -> Option<(Option<TrackId>, Option<EffectId>)> {
    if project
        .tracks
        .iter()
        .any(|t| t.id == id && t.instrument.plugin.is_some())
    {
        return Some((Some(id), None));
    }
    project
        .tracks
        .iter()
        .map(|t| (Some(t.id), &t.mixer.effects))
        .chain(project.buses.iter().map(|b| (Some(b.id), &b.mixer.effects)))
        .chain(std::iter::once((None, &project.master.effects)))
        .find(|(_, chain)| chain.iter().any(|e| e.id == id && e.plugin.is_some()))
        .map(|(owner, _)| (owner, Some(id)))
}

/// Every plugin the song uses (instrument track ids and effect ids).
fn plugin_ids(project: &Project) -> Vec<Id> {
    let instruments = project
        .tracks
        .iter()
        .filter(|t| t.instrument.plugin.is_some())
        .map(|t| t.id);
    let effects = project
        .tracks
        .iter()
        .map(|t| &t.mixer.effects)
        .chain(project.buses.iter().map(|b| &b.mixer.effects))
        .chain(std::iter::once(&project.master.effects))
        .flat_map(|c| c.iter())
        .filter(|e| e.plugin.is_some())
        .map(|e| e.id);
    instruments.chain(effects).collect()
}

/// Applies `command` as one undo step, updates the engine, and tells the
/// screens.
fn apply<H: Host>(host: &H, command: Command, description: &str) -> Result<(), String> {
    {
        let mut s = host.session()?;
        s.execute(command).map_err(|e| e.to_string())?;
        s.end_gesture();
        crate::host::sync(host, &s);
    }
    host.project_changed(description);
    Ok(())
}

/// Gives a track an installed plugin instrument (one undo step), then
/// records the plugin's parameter list in the song.
pub fn load_plugin<H: Host>(host: &H, track_id: TrackId, uid: &str) -> Result<PluginInfo, String> {
    let info = find(host, uid)?;
    if info.kind != PluginKind::Instrument {
        return Err(format!(
            "{} is an effect, not an instrument; add it with add_plugin_effect",
            info.name
        ));
    }
    let instrument = Instrument {
        kind: InstrumentKind::Plugin,
        preset: info.name.clone(),
        params: BTreeMap::new(),
        sample_pack: None,
        plugin: Some(song_ref(&info)),
    };
    apply(
        host,
        Command::SetInstrument {
            track_id,
            instrument,
        },
        &format!("Load plugin {}", info.name),
    )?;
    // In the app the plugin is still loading; its list arrives with
    // `plugin_loaded`.
    note_live(host, track_id);
    Ok(info)
}

/// Adds an installed plugin effect at the end of a chain: a track's, a
/// bus's (by its id), or the master's (None). Returns the new effect's id.
pub fn add_plugin_effect<H: Host>(
    host: &H,
    track_id: Option<TrackId>,
    uid: &str,
) -> Result<(EffectId, PluginInfo), String> {
    let info = find(host, uid)?;
    if info.kind != PluginKind::Effect {
        return Err(format!(
            "{} is an instrument; put it on a track with load_plugin",
            info.name
        ));
    }
    apply(
        host,
        Command::AddPluginEffect {
            track_id,
            plugin: song_ref(&info),
            index: None,
        },
        &format!("Add plugin effect {}", info.name),
    )?;
    let effect_id = {
        let s = host.session()?;
        let p = s.project();
        let chain = match track_id {
            None => Some(&p.master.effects),
            Some(id) => p
                .tracks
                .iter()
                .find(|t| t.id == id)
                .map(|t| &t.mixer.effects)
                .or_else(|| {
                    p.buses
                        .iter()
                        .find(|b| b.id == id)
                        .map(|b| &b.mixer.effects)
                }),
        };
        chain
            .and_then(|c| c.last())
            .map(|e| e.id)
            .ok_or("the effect went away")?
    };
    note_live(host, effect_id);
    Ok((effect_id, info))
}

/// Copies a live plugin's parameter list (and anything new in it) into
/// the song, without an undo step.
pub fn note_live<H: Host>(host: &H, id: Id) {
    let Ok(instance) = live(host, id) else {
        return;
    };
    let Ok(values) = instance.values() else {
        return;
    };
    let params: BTreeMap<u32, f64> = values.into_iter().collect();
    if let Ok(mut s) = host.session() {
        s.note_plugin(id, &params, None);
    }
}

/// A plugin finished loading in the background: record its parameters,
/// send its window's edits to `edits`, and tell the screens.
pub fn plugin_loaded<H: Host>(host: &H, id: Id, edits: &std::sync::mpsc::Sender<(Id, Edit)>) {
    note_live(host, id);
    connect_edits(host, edits);
    host.project_changed("Plugin loaded");
}

/// Before saving or rendering: stores every live plugin's own settings in
/// the song, so the file (and offline renders) sound like what's playing.
pub fn store_states<H: Host>(host: &H) {
    let Some(engine) = host.engine() else {
        return;
    };
    let ids = match host.session() {
        Ok(s) => plugin_ids(s.project()),
        Err(_) => return,
    };
    for id in ids {
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

/// A plugin's parameters with names, values and display text.
pub fn params<H: Host>(host: &H, id: Id) -> Result<Vec<ParamInfo>, String> {
    live(host, id)?.params()
}

/// Turns an edit made in a plugin's own window into the matching Command,
/// so it's undoable and Claude sees it. A whole drag is one undo step.
pub fn apply_edit<H: Host>(host: &H, id: Id, edit: Edit) -> Result<(), String> {
    match edit {
        Edit::Begin(_) => Ok(()),
        Edit::Perform(param, value) => {
            {
                let mut s = host.session()?;
                let (track_id, effect_id) =
                    address(s.project(), id).ok_or("that plugin was removed")?;
                let known = s
                    .project()
                    .plugin(id)
                    .is_some_and(|p| p.params.contains_key(&param));
                if !known {
                    // A parameter the plugin added since loading.
                    s.note_plugin(id, &[(param, value)].into_iter().collect(), None);
                }
                s.execute(Command::SetPluginParams {
                    track_id,
                    effect_id,
                    params: [(param, value.clamp(0.0, 1.0))].into_iter().collect(),
                })
                .map_err(|e| e.to_string())?;
                crate::host::sync(host, &s);
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
            let values: BTreeMap<u32, f64> = live(host, id)?.values()?.into_iter().collect();
            {
                let mut s = host.session()?;
                s.note_plugin(id, &values, None);
                let (track_id, effect_id) =
                    address(s.project(), id).ok_or("that plugin was removed")?;
                let current = s
                    .project()
                    .plugin(id)
                    .map(|p| p.params.clone())
                    .unwrap_or_default();
                let changed: BTreeMap<u32, f64> = values
                    .into_iter()
                    .filter(|(k, v)| current.get(k).is_some_and(|c| (c - v).abs() > 1e-9))
                    .collect();
                if changed.is_empty() {
                    return Ok(());
                }
                s.end_gesture();
                s.execute(Command::SetPluginParams {
                    track_id,
                    effect_id,
                    params: changed,
                })
                .map_err(|e| e.to_string())?;
                s.end_gesture();
                crate::host::sync(host, &s);
            }
            host.project_changed("Plugin preset");
            Ok(())
        }
    }
}

/// Points every live plugin's window edits at `send`. Call when plugins
/// appear; edits must be applied off the plugin's own call, e.g. by a
/// thread reading the other end.
pub fn connect_edits<H: Host>(host: &H, send: &std::sync::mpsc::Sender<(Id, Edit)>) {
    let Some(engine) = host.engine() else {
        return;
    };
    let ids = match host.session() {
        Ok(s) => plugin_ids(s.project()),
        Err(_) => return,
    };
    for id in ids {
        if let Some(Ok(instance)) = engine.plugin(id) {
            let tx = std::sync::Mutex::new(send.clone());
            instance.on_edit(move |e| {
                if let Ok(tx) = tx.lock() {
                    let _ = tx.send((id, e));
                }
            });
        }
    }
}
