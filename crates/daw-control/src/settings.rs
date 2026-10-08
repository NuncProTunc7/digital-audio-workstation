//! App settings that belong to this computer rather than to a song, such as
//! how late each microphone's recordings arrive. Kept in `settings.json` in
//! the app-data folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::discovery::APP_ID;

/// Largest recording delay correction, either way (ms).
pub const MAX_RECORDING_OFFSET_MS: f64 = 500.0;
/// Longest count-in before recording, in bars.
pub const MAX_COUNT_IN_BARS: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Per input device: how much later than the beat its recordings arrive
    /// (ms). Takes are moved this much earlier.
    #[serde(default)]
    pub recording_offsets_ms: BTreeMap<String, f64>,
    /// Bars of metronome clicks before recording starts (0 = none).
    #[serde(default = "default_count_in_bars")]
    pub count_in_bars: u32,
    /// Sound card buffer size in frames (None = the device's default).
    #[serde(default)]
    pub buffer_frames: Option<u32>,
}

fn default_count_in_bars() -> u32 {
    1
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            recording_offsets_ms: BTreeMap::new(),
            count_in_bars: default_count_in_bars(),
            buffer_frames: None,
        }
    }
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

    /// Sets the count-in (clamped to 0..=2 bars); returns what was stored.
    pub fn set_count_in_bars(&mut self, bars: u32) -> u32 {
        self.count_in_bars = bars.min(MAX_COUNT_IN_BARS);
        self.count_in_bars
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
    fn count_in_defaults_to_one_bar_and_round_trips() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("settings.json");
        // Settings saved before the count-in existed get the default.
        std::fs::write(&path, r#"{ "recording_offsets_ms": {} }"#).expect("write");
        assert_eq!(Settings::load(&path).count_in_bars, 1);
        let mut s = Settings::default();
        assert_eq!(s.set_count_in_bars(9), 2);
        assert_eq!(s.set_count_in_bars(0), 0);
        s.save(&path).expect("save");
        assert_eq!(Settings::load(&path).count_in_bars, 0);
    }

    #[test]
    fn recognizes_bluetooth_names() {
        assert!(looks_bluetooth("Headset (WH-1000XM4 Hands-Free AG Audio)"));
        assert!(looks_bluetooth("Microphone (Galaxy Buds2 Pro)"));
        assert!(!looks_bluetooth("Microphone Array (Realtek(R) Audio)"));
    }
}
