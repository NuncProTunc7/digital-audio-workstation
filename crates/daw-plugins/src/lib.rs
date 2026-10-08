//! Third-party plugins (VST3): finding them, loading them, and running them
//! as instruments and effects.
//!
//! Plugins run in the app's process. Their real-time `process` call is made
//! from the audio thread with preallocated buffers, events and parameter
//! queues (our side never allocates there); everything else (creating,
//! saving state, windows) happens off the audio thread.

// VST3 enum constants are i32 on Windows and u32 elsewhere, so casts that
// are no-ops here are needed on Linux.
#![allow(clippy::unnecessary_cast)]

pub mod com;
pub mod instance;
pub mod main_thread;
pub mod module;
pub mod scan;

pub use com::Edit;
pub use instance::{Instance, ParamInfo, PluginProcessor};
pub use module::Module;

use serde::{Deserialize, Serialize};
use vst3::Steinberg::TUID;

/// Whether a plugin makes sound from notes or changes sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    Instrument,
    Effect,
}

impl PluginKind {
    /// From VST3 sub-categories such as `"Instrument|Synth"` or `"Fx|Reverb"`.
    pub fn from_subcategories(sub: &str) -> PluginKind {
        if sub.split('|').any(|s| s.eq_ignore_ascii_case("Instrument")) {
            PluginKind::Instrument
        } else {
            PluginKind::Effect
        }
    }
}

/// One plugin installed on this computer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginInfo {
    /// The plugin's class id: 32 hex digits, as in `moduleinfo.json`.
    pub uid: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub kind: PluginKind,
    /// VST3 sub-categories, e.g. `"Fx|Reverb"`.
    pub categories: String,
    /// The `.vst3` file or folder.
    pub path: String,
}

/// A class id as text, the way the VST3 SDK prints it (and `moduleinfo.json`
/// stores it), so ids from either source match.
pub fn uid_string(tuid: &TUID) -> String {
    let b = tuid.map(|x| x as u8);
    #[cfg(windows)]
    {
        // COM-compatible layout: the first three fields are little-endian.
        let d1 = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let d2 = u16::from_le_bytes([b[4], b[5]]);
        let d3 = u16::from_le_bytes([b[6], b[7]]);
        let rest: String = b[8..].iter().map(|x| format!("{x:02X}")).collect();
        format!("{d1:08X}{d2:04X}{d3:04X}{rest}")
    }
    #[cfg(not(windows))]
    {
        b.iter().map(|x| format!("{x:02X}")).collect()
    }
}

/// The inverse of [`uid_string`].
pub fn parse_uid(s: &str) -> Option<TUID> {
    let s = s.trim();
    if s.len() != 32 || !s.is_ascii() {
        return None;
    }
    let mut b = [0u8; 16];
    for (i, byte) in b.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    #[cfg(windows)]
    {
        b[0..4].reverse();
        b[4..6].reverse();
        b[6..8].reverse();
    }
    Some(b.map(|x| x as i8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_ids_print_like_the_sdk() {
        let id = vst3::uid(0x4E505431, 0x53594E54, 0x48000000, 0x00000001);
        let s = uid_string(&id);
        assert_eq!(s, "4E50543153594E544800000000000001");
        assert_eq!(parse_uid(&s), Some(id));
        assert_eq!(parse_uid("nope"), None);
        assert_eq!(
            PluginKind::from_subcategories("Instrument|Synth"),
            PluginKind::Instrument
        );
        assert_eq!(
            PluginKind::from_subcategories("Fx|Reverb"),
            PluginKind::Effect
        );
    }
}
