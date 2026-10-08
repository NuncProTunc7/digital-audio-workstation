//! Remembering a plugin that crashed the app, so the next start doesn't
//! crash again the same way.
//!
//! While a plugin starts (or opens its window) a marker file names it; it
//! is removed when that finishes. A marker left behind at start-up means
//! that plugin took the app down: it is switched off until the user looks
//! for plugins again (after updating or reinstalling it).
//!
//! Without [`init`] (tests, the command line) nothing is written.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::PluginInfo;

static DIR: OnceLock<PathBuf> = OnceLock::new();

fn blocked() -> &'static Mutex<BTreeSet<String>> {
    static B: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    B.get_or_init(|| Mutex::new(BTreeSet::new()))
}

#[derive(Serialize, Deserialize)]
struct Marker {
    uid: String,
    name: String,
}

const MARKER: &str = "plugin-starting.json";
const BLOCKED: &str = "plugins-switched-off.json";

/// Starts guarding with files in `dir`. Returns the name of a plugin that
/// crashed the app last time (it is now switched off).
pub fn init(dir: &Path) -> Option<String> {
    let _ = std::fs::create_dir_all(dir);
    let _ = DIR.set(dir.to_path_buf());
    let mut list: BTreeSet<String> = std::fs::read_to_string(dir.join(BLOCKED))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let crashed = std::fs::read_to_string(dir.join(MARKER))
        .ok()
        .and_then(|t| serde_json::from_str::<Marker>(&t).ok());
    let _ = std::fs::remove_file(dir.join(MARKER));
    if let Some(m) = &crashed {
        list.insert(m.uid.to_ascii_uppercase());
        save(dir, &list);
    }
    if let Ok(mut b) = blocked().lock() {
        *b = list;
    }
    crashed.map(|m| m.name)
}

fn save(dir: &Path, list: &BTreeSet<String>) {
    if let Ok(text) = serde_json::to_string(list) {
        let _ = std::fs::write(dir.join(BLOCKED), text);
    }
}

/// Whether this plugin is switched off for crashing the app.
pub fn is_blocked(uid: &str) -> bool {
    blocked()
        .lock()
        .is_ok_and(|b| b.contains(&uid.to_ascii_uppercase()))
}

/// Gives every switched-off plugin another chance.
pub fn unblock_all() {
    if let Ok(mut b) = blocked().lock() {
        b.clear();
        if let Some(dir) = DIR.get() {
            save(dir, &b);
        }
    }
}

/// Runs `f` (starting `info`, or opening its window) with a marker naming
/// it, so a crash inside is remembered.
pub fn watch<R>(info: &PluginInfo, f: impl FnOnce() -> R) -> R {
    let marker = DIR.get().map(|d| d.join(MARKER));
    if let Some(path) = &marker {
        let m = Marker {
            uid: info.uid.clone(),
            name: info.name.clone(),
        };
        if let Ok(text) = serde_json::to_string(&m) {
            let _ = std::fs::write(path, text);
        }
    }
    let result = f();
    if let Some(path) = &marker {
        let _ = std::fs::remove_file(path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PluginKind;

    #[test]
    fn a_plugin_that_crashed_is_switched_off_until_plugins_are_looked_for_again() {
        let dir = tempfile::tempdir().expect("tmp");
        // A previous run died while "Big Synth" was starting.
        std::fs::write(
            dir.path().join(MARKER),
            r#"{ "uid": "0123456789abcdef0123456789abcdef", "name": "Big Synth" }"#,
        )
        .expect("marker");
        assert_eq!(init(dir.path()).as_deref(), Some("Big Synth"));
        assert!(is_blocked("0123456789ABCDEF0123456789ABCDEF"));
        assert!(!dir.path().join(MARKER).exists());
        // A clean start of another plugin leaves no marker.
        let info = PluginInfo {
            uid: "FEDCBA9876543210FEDCBA9876543210".into(),
            name: "Fine".into(),
            vendor: String::new(),
            version: String::new(),
            kind: PluginKind::Effect,
            categories: String::new(),
            path: String::new(),
        };
        let seen = watch(&info, || dir.path().join(MARKER).exists());
        assert!(seen, "the marker exists while it starts");
        assert!(!dir.path().join(MARKER).exists());
        // Still blocked after a restart, until unblocked.
        assert_eq!(init(dir.path()), None);
        assert!(is_blocked("0123456789ABCDEF0123456789ABCDEF"));
        unblock_all();
        assert!(!is_blocked("0123456789ABCDEF0123456789ABCDEF"));
    }
}
