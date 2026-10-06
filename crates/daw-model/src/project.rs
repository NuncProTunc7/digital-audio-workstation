use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Slowest tempo a project accepts, in beats per minute.
pub const MIN_TEMPO_BPM: f64 = 20.0;
/// Fastest tempo a project accepts, in beats per minute.
pub const MAX_TEMPO_BPM: f64 = 999.0;

/// A song: everything saved in a project file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Project {
    /// Display name shown in the title bar and used as the default export name.
    pub name: String,
    /// Tempo in beats per minute.
    pub tempo_bpm: f64,
    /// Meter, such as 4/4 or 6/8.
    pub time_signature: TimeSignature,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            name: "Untitled".to_owned(),
            tempo_bpm: 120.0,
            time_signature: TimeSignature::default(),
        }
    }
}

/// Musical meter: `numerator` beats per bar, each a 1/`denominator` note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TimeSignature {
    /// Beats per bar (1–32).
    pub numerator: u8,
    /// Note value of one beat: 1, 2, 4, 8, 16, or 32.
    pub denominator: u8,
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self {
            numerator: 4,
            denominator: 4,
        }
    }
}
