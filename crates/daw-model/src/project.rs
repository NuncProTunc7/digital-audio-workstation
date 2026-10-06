use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::instrument::{Instrument, InstrumentKind};

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
    /// Instrument tracks, top to bottom.
    pub tracks: Vec<Track>,
}

impl Project {
    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn track_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }
}

impl Default for Project {
    /// A new song with the three instruments most game tracks start from.
    fn default() -> Self {
        let track = |id, name: &str, kind, preset| Track {
            id,
            name: name.to_owned(),
            instrument: Instrument::from_preset(kind, preset)
                .unwrap_or_else(|| unreachable!("factory preset {preset} exists")),
        };
        Self {
            name: "Untitled".to_owned(),
            tempo_bpm: 120.0,
            time_signature: TimeSignature::default(),
            tracks: vec![
                track(1, "Keys", InstrumentKind::Synth, "Warm Keys"),
                track(2, "Bass", InstrumentKind::Synth, "Fat Bass"),
                track(3, "Drums", InstrumentKind::Drums, "Classic Kit"),
            ],
        }
    }
}

/// Stable identifier of a track within a project.
pub type TrackId = u32;

/// One instrument lane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Track {
    pub id: TrackId,
    pub name: String,
    pub instrument: Instrument,
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
