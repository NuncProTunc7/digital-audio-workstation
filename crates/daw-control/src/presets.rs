//! The user's own instrument presets: a track's sound saved under a name,
//! kept on this computer (`presets.json` in the app-data folder) so every
//! song can use it. Applying one is an ordinary `SetInstrument` Command.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use daw_model::instrument::{Instrument, InstrumentKind, presets};
use serde::{Deserialize, Serialize};

use crate::discovery::APP_ID;

/// Longest preset name, in characters.
const MAX_NAME_CHARS: usize = 40;

/// A saved sound.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserPreset {
    pub name: String,
    pub kind: InstrumentKind,
    pub params: BTreeMap<String, f64>,
    /// For samplers: the SFZ sample pack it plays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_pack: Option<String>,
}

impl UserPreset {
    /// The instrument settings this preset gives a track.
    pub fn instrument(&self) -> Instrument {
        Instrument {
            kind: self.kind,
            preset: self.name.clone(),
            params: self.params.clone(),
            sample_pack: self.sample_pack.clone(),
            plugin: None,
        }
    }
}

/// `%APPDATA%\io.github.nuncprotunc7.nuncprotune\presets.json` on Windows.
pub fn presets_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_ID)
        .join("presets.json")
}

/// The saved presets, by kind then name; empty if there are none or the
/// file is unreadable.
pub fn load(path: &Path) -> Vec<UserPreset> {
    let mut list: Vec<UserPreset> = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    sort(&mut list);
    list
}

fn sort(list: &mut [UserPreset]) {
    list.sort_by(|a, b| {
        format!("{:?}", a.kind)
            .cmp(&format!("{:?}", b.kind))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

fn write(path: &Path, list: &[UserPreset]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Saves `instrument` as preset `name` (replacing a saved one of the same
/// name and kind). Returns the updated list.
pub fn save(path: &Path, name: &str, instrument: &Instrument) -> Result<Vec<UserPreset>, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("give the preset a name".into());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!(
            "preset names are at most {MAX_NAME_CHARS} characters"
        ));
    }
    if instrument.kind.is_audio() {
        return Err("audio tracks have no sound settings to save".into());
    }
    if presets(instrument.kind)
        .iter()
        .any(|p| p.name.eq_ignore_ascii_case(name))
    {
        return Err(format!(
            "\"{name}\" is a built-in preset; choose another name"
        ));
    }
    let mut list = load(path);
    list.retain(|p| !(p.kind == instrument.kind && p.name.eq_ignore_ascii_case(name)));
    list.push(UserPreset {
        name: name.to_owned(),
        kind: instrument.kind,
        params: instrument.params.clone(),
        sample_pack: instrument.sample_pack.clone(),
    });
    sort(&mut list);
    write(path, &list)?;
    Ok(list)
}

/// Deletes saved preset `name` of `kind`. Returns the updated list.
pub fn delete(path: &Path, kind: InstrumentKind, name: &str) -> Result<Vec<UserPreset>, String> {
    let mut list = load(path);
    let before = list.len();
    list.retain(|p| !(p.kind == kind && p.name == name));
    if list.len() == before {
        return Err(format!("there is no saved preset called \"{name}\""));
    }
    write(path, &list)?;
    Ok(list)
}

/// Finds saved preset `name` for an instrument of `kind`.
pub fn find(path: &Path, kind: InstrumentKind, name: &str) -> Result<UserPreset, String> {
    load(path)
        .into_iter()
        .find(|p| p.kind == kind && p.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| {
            format!("there is no saved {kind:?} preset called \"{name}\"").to_lowercase()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_save_replace_list_and_delete() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("presets.json");
        assert!(load(&path).is_empty());
        let mut keys = Instrument::from_preset(InstrumentKind::Synth, "Warm Keys").expect("keys");
        keys.params.insert("filter.cutoff_hz".into(), 900.0);
        save(&path, "My Dark Keys", &keys).expect("save");
        keys.params.insert("filter.cutoff_hz".into(), 700.0);
        // Same name again replaces it.
        let list = save(&path, " my dark keys ", &keys).expect("replace");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].params["filter.cutoff_hz"], 700.0);
        let found = find(&path, InstrumentKind::Synth, "MY DARK KEYS").expect("find");
        assert_eq!(found.instrument().params, keys.params);
        assert_eq!(found.instrument().preset, "my dark keys");
        // Not for another kind; not over a factory name.
        assert!(find(&path, InstrumentKind::Drums, "my dark keys").is_err());
        assert!(save(&path, "Warm Keys", &keys).is_err());
        assert!(save(&path, "  ", &keys).is_err());
        assert!(
            delete(&path, InstrumentKind::Synth, "my dark keys")
                .expect("delete")
                .is_empty()
        );
        assert!(delete(&path, InstrumentKind::Synth, "my dark keys").is_err());
    }
}
