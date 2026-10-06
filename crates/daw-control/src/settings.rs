//! App settings that belong to this computer rather than to a song, such as
//! how late each microphone's recordings arrive. Kept in `settings.json` in
//! the app-data folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::discovery::APP_ID;

/// Largest recording delay correction, either way (ms).
pub const MAX_RECORDING_OFFSET_MS: f64 = 500.0;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Per input device: how much later than the beat its recordings arrive
    /// (ms). Takes are moved this much earlier.
    #[serde(default)]
    pub recording_offsets_ms: BTreeMap<String, f64>,
}

/// `%APPDATA%\io.github.nuncprotunc7.nuncprotune\settings.json` on Windows.
pub fn settings_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_ID)
        .join("settings.json")
}

impl Settings {
    /// The saved settings; defaults if there are none or they're unreadable.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    pub fn recording_offset_ms(&self, device: &str) -> f64 {
        self.recording_offsets_ms
            .get(device)
            .copied()
            .unwrap_or(0.0)
    }

    /// Sets a device's offset (clamped to ±500 ms); returns what was stored.
    pub fn set_recording_offset_ms(&mut self, device: &str, ms: f64) -> f64 {
        let ms = if ms.is_finite() {
            ms.clamp(-MAX_RECORDING_OFFSET_MS, MAX_RECORDING_OFFSET_MS)
        } else {
            0.0
        };
        if ms == 0.0 {
            self.recording_offsets_ms.remove(device);
        } else {
            self.recording_offsets_ms.insert(device.to_owned(), ms);
        }
        ms
    }
}

/// Whether a device name looks like a Bluetooth headset or earbuds. Windows
/// names a Bluetooth headset's microphone "Headset" or "Hands-Free".
pub fn looks_bluetooth(device: &str) -> bool {
    let d = device.to_lowercase();
    [
        "bluetooth",
        "hands-free",
        "handsfree",
        "headset",
        "ag audio",
        "airpods",
        "buds",
    ]
    .iter()
    .any(|k| d.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_round_trip_and_clamp() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("settings.json");
        assert_eq!(Settings::load(&path), Settings::default());
        let mut s = Settings::default();
        assert_eq!(
            s.set_recording_offset_ms("Headset (WH-1000XM4)", 183.0),
            183.0
        );
        assert_eq!(s.set_recording_offset_ms("Far away", 9_000.0), 500.0);
        s.save(&path).expect("save");
        let back = Settings::load(&path);
        assert_eq!(back.recording_offset_ms("Headset (WH-1000XM4)"), 183.0);
        assert_eq!(back.recording_offset_ms("Laptop mic"), 0.0);
    }

    #[test]
    fn recognizes_bluetooth_names() {
        assert!(looks_bluetooth("Headset (WH-1000XM4 Hands-Free AG Audio)"));
        assert!(looks_bluetooth("Microphone (Galaxy Buds2 Pro)"));
        assert!(!looks_bluetooth("Microphone Array (Realtek(R) Audio)"));
    }
}
