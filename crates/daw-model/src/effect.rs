//! Built-in audio effects as data: kinds, parameters, and defaults.
//! The DSP lives in `daw-effects`.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Id;
use crate::instrument::{ParamSpec, Unit, choice, p};

/// Which built-in effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    /// Three-band equalizer with a low cut: shape the tone.
    Eq,
    /// Evens out loud and quiet parts; adds punch.
    Compressor,
    /// Room and hall ambience.
    Reverb,
    /// Echoes, optionally synced to the tempo and bounced left/right.
    Delay,
    /// Thickens and widens a sound with gently moving copies.
    Chorus,
    /// Warm saturation to hard fuzz.
    Distortion,
    /// Stops the signal from going over a ceiling. Best last on the master.
    Limiter,
}

impl EffectKind {
    pub const ALL: [EffectKind; 7] = [
        EffectKind::Eq,
        EffectKind::Compressor,
        EffectKind::Reverb,
        EffectKind::Delay,
        EffectKind::Chorus,
        EffectKind::Distortion,
        EffectKind::Limiter,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            EffectKind::Eq => "EQ",
            EffectKind::Compressor => "Compressor",
            EffectKind::Reverb => "Reverb",
            EffectKind::Delay => "Delay",
            EffectKind::Chorus => "Chorus",
            EffectKind::Distortion => "Distortion",
            EffectKind::Limiter => "Limiter",
        }
    }
}

/// One effect in a track's or the master's chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Effect {
    pub id: Id,
    pub kind: EffectKind,
    /// Bypassed effects pass sound through unchanged.
    pub enabled: bool,
    /// Parameter values keyed by parameter id (see `describe_effects`).
    pub params: BTreeMap<String, f64>,
}

impl Effect {
    /// A new effect with default settings.
    pub fn new(id: Id, kind: EffectKind) -> Self {
        Self {
            id,
            kind,
            enabled: true,
            params: effect_params(kind)
                .iter()
                .map(|s| (s.id.to_owned(), s.default))
                .collect(),
        }
    }

    pub fn value(&self, id: &str) -> Option<f64> {
        self.params
            .get(id)
            .copied()
            .or_else(|| effect_spec(self.kind, id).map(|s| s.default))
    }
}

pub const DELAY_SYNC_CHOICES: &[&str] = &[
    "Free (use Time)",
    "1/2",
    "1/4",
    "Dotted 1/8",
    "1/8",
    "1/8 triplet",
    "1/16",
];

/// Delay length in beats for each sync choice (index 0 = free).
pub const DELAY_SYNC_BEATS: [f64; 7] = [0.0, 2.0, 1.0, 0.75, 0.5, 1.0 / 3.0, 0.25];

const OFF_ON: &[&str] = &["Off", "On"];

/// Parameters for each effect. Order is the engine's parameter index:
/// append only.
pub const EQ_PARAMS: &[ParamSpec] = &[
    p(
        "low_cut_hz",
        "Low cut",
        "EQ",
        20.0,
        1_000.0,
        20.0,
        Unit::Hertz,
        true,
    ),
    p(
        "low.gain_db",
        "Low",
        "EQ",
        -18.0,
        18.0,
        0.0,
        Unit::Decibels,
        false,
    ),
    p(
        "low.freq_hz",
        "Low freq",
        "EQ",
        40.0,
        800.0,
        150.0,
        Unit::Hertz,
        true,
    ),
    p(
        "mid.gain_db",
        "Mid",
        "EQ",
        -18.0,
        18.0,
        0.0,
        Unit::Decibels,
        false,
    ),
    p(
        "mid.freq_hz",
        "Mid freq",
        "EQ",
        200.0,
        8_000.0,
        1_000.0,
        Unit::Hertz,
        true,
    ),
    p("mid.q", "Mid width", "EQ", 0.3, 8.0, 1.0, Unit::None, true),
    p(
        "high.gain_db",
        "High",
        "EQ",
        -18.0,
        18.0,
        0.0,
        Unit::Decibels,
        false,
    ),
    p(
        "high.freq_hz",
        "High freq",
        "EQ",
        1_500.0,
        16_000.0,
        6_000.0,
        Unit::Hertz,
        true,
    ),
];

pub const COMPRESSOR_PARAMS: &[ParamSpec] = &[
    p(
        "threshold_db",
        "Threshold",
        "Compressor",
        -60.0,
        0.0,
        -18.0,
        Unit::Decibels,
        false,
    ),
    p(
        "ratio",
        "Ratio",
        "Compressor",
        1.0,
        20.0,
        4.0,
        Unit::Ratio,
        true,
    ),
    p(
        "attack_s",
        "Attack",
        "Compressor",
        0.0005,
        0.2,
        0.01,
        Unit::Seconds,
        true,
    ),
    p(
        "release_s",
        "Release",
        "Compressor",
        0.01,
        2.0,
        0.15,
        Unit::Seconds,
        true,
    ),
    p(
        "knee_db",
        "Knee",
        "Compressor",
        0.0,
        24.0,
        6.0,
        Unit::Decibels,
        false,
    ),
    p(
        "makeup_db",
        "Makeup",
        "Compressor",
        0.0,
        24.0,
        0.0,
        Unit::Decibels,
        false,
    ),
    p(
        "mix",
        "Mix",
        "Compressor",
        0.0,
        1.0,
        1.0,
        Unit::Percent,
        false,
    ),
];

pub const REVERB_PARAMS: &[ParamSpec] = &[
    p(
        "size",
        "Size",
        "Reverb",
        0.0,
        1.0,
        0.6,
        Unit::Percent,
        false,
    ),
    p(
        "damping",
        "Damping",
        "Reverb",
        0.0,
        1.0,
        0.4,
        Unit::Percent,
        false,
    ),
    p(
        "width",
        "Width",
        "Reverb",
        0.0,
        1.0,
        1.0,
        Unit::Percent,
        false,
    ),
    p(
        "predelay_s",
        "Pre-delay",
        "Reverb",
        0.0,
        0.2,
        0.01,
        Unit::Seconds,
        false,
    ),
    p("mix", "Mix", "Reverb", 0.0, 1.0, 0.25, Unit::Percent, false),
];

pub const DELAY_PARAMS: &[ParamSpec] = &[
    choice("sync", "Sync", "Delay", DELAY_SYNC_CHOICES, 4.0),
    p(
        "time_s",
        "Time",
        "Delay",
        0.01,
        2.0,
        0.3,
        Unit::Seconds,
        true,
    ),
    p(
        "feedback",
        "Feedback",
        "Delay",
        0.0,
        0.95,
        0.35,
        Unit::Percent,
        false,
    ),
    p(
        "tone_hz",
        "Tone",
        "Delay",
        500.0,
        20_000.0,
        6_000.0,
        Unit::Hertz,
        true,
    ),
    choice("ping_pong", "Ping-pong", "Delay", OFF_ON, 0.0),
    p("mix", "Mix", "Delay", 0.0, 1.0, 0.25, Unit::Percent, false),
];

pub const CHORUS_PARAMS: &[ParamSpec] = &[
    p(
        "rate_hz",
        "Rate",
        "Chorus",
        0.05,
        5.0,
        0.8,
        Unit::Hertz,
        true,
    ),
    p(
        "depth",
        "Depth",
        "Chorus",
        0.0,
        1.0,
        0.5,
        Unit::Percent,
        false,
    ),
    p("mix", "Mix", "Chorus", 0.0, 1.0, 0.5, Unit::Percent, false),
];

pub const DISTORTION_PARAMS: &[ParamSpec] = &[
    p(
        "drive_db",
        "Drive",
        "Distortion",
        0.0,
        40.0,
        12.0,
        Unit::Decibels,
        false,
    ),
    p(
        "tone_hz",
        "Tone",
        "Distortion",
        500.0,
        20_000.0,
        8_000.0,
        Unit::Hertz,
        true,
    ),
    p(
        "output_db",
        "Output",
        "Distortion",
        -24.0,
        6.0,
        -6.0,
        Unit::Decibels,
        false,
    ),
    p(
        "mix",
        "Mix",
        "Distortion",
        0.0,
        1.0,
        1.0,
        Unit::Percent,
        false,
    ),
];

pub const LIMITER_PARAMS: &[ParamSpec] = &[
    p(
        "input_db",
        "Input gain",
        "Limiter",
        0.0,
        24.0,
        0.0,
        Unit::Decibels,
        false,
    ),
    p(
        "ceiling_db",
        "Ceiling",
        "Limiter",
        -12.0,
        0.0,
        -1.0,
        Unit::Decibels,
        false,
    ),
    p(
        "release_s",
        "Release",
        "Limiter",
        0.01,
        1.0,
        0.1,
        Unit::Seconds,
        true,
    ),
];

pub fn effect_params(kind: EffectKind) -> &'static [ParamSpec] {
    match kind {
        EffectKind::Eq => EQ_PARAMS,
        EffectKind::Compressor => COMPRESSOR_PARAMS,
        EffectKind::Reverb => REVERB_PARAMS,
        EffectKind::Delay => DELAY_PARAMS,
        EffectKind::Chorus => CHORUS_PARAMS,
        EffectKind::Distortion => DISTORTION_PARAMS,
        EffectKind::Limiter => LIMITER_PARAMS,
    }
}

pub fn effect_spec(kind: EffectKind, id: &str) -> Option<&'static ParamSpec> {
    effect_params(kind).iter().find(|s| s.id == id)
}

/// Everything the UI and Claude need to know about an effect kind.
#[derive(Debug, Clone, Serialize)]
pub struct EffectDescription {
    pub kind: EffectKind,
    pub name: &'static str,
    pub description: &'static str,
    pub params: &'static [ParamSpec],
}

pub fn describe_effects() -> Vec<EffectDescription> {
    EffectKind::ALL
        .iter()
        .map(|&kind| EffectDescription {
            kind,
            name: kind.display_name(),
            description: match kind {
                EffectKind::Eq => "Three-band equalizer with a low cut: shape the tone.",
                EffectKind::Compressor => "Evens out loud and quiet parts; adds punch.",
                EffectKind::Reverb => "Room and hall ambience.",
                EffectKind::Delay => "Echoes, optionally synced to the tempo.",
                EffectKind::Chorus => "Thickens and widens with moving copies.",
                EffectKind::Distortion => "Warm saturation to hard fuzz.",
                EffectKind::Limiter => "Keeps peaks under a ceiling. Use last on the master.",
            },
            params: effect_params(kind),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_params_are_unique_and_defaults_valid() {
        for kind in EffectKind::ALL {
            let specs = effect_params(kind);
            for (i, s) in specs.iter().enumerate() {
                assert!(s.validate(s.default), "{kind:?} {}", s.id);
                assert!(specs[..i].iter().all(|o| o.id != s.id), "dup {}", s.id);
            }
            let e = Effect::new(1, kind);
            assert_eq!(e.params.len(), specs.len());
        }
        assert_eq!(DELAY_SYNC_CHOICES.len(), DELAY_SYNC_BEATS.len());
    }
}
