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
    /// Guitar pedal: a warm, mid-boosted tube-style push.
    Overdrive,
    /// Guitar pedal: hard, buzzy, square-ish clipping.
    Fuzz,
    /// A guitar amp and speaker cabinet: gain, tone stack, cabinet.
    Amp,
    /// Guitar pedal: a swirling all-pass sweep.
    Phaser,
    /// Guitar pedal: a jet-plane comb sweep.
    Flanger,
    /// Guitar pedal: volume pulsing.
    Tremolo,
    /// Guitar pedal: a sweeping band-pass "wow", automatic or fixed.
    Wah,
    /// Silences quiet hiss and hum between phrases.
    NoiseGate,
    /// Guitar pedal: adds an octave below (and a fuzzy octave above).
    Octaver,
    /// A third-party VST3 effect installed on this computer. Add one with
    /// add_plugin_effect; its controls are in plugin_params.
    Plugin,
}

impl EffectKind {
    pub const ALL: [EffectKind; 16] = [
        EffectKind::Eq,
        EffectKind::Compressor,
        EffectKind::Reverb,
        EffectKind::Delay,
        EffectKind::Chorus,
        EffectKind::Distortion,
        EffectKind::Limiter,
        EffectKind::Overdrive,
        EffectKind::Fuzz,
        EffectKind::Amp,
        EffectKind::Phaser,
        EffectKind::Flanger,
        EffectKind::Tremolo,
        EffectKind::Wah,
        EffectKind::NoiseGate,
        EffectKind::Octaver,
    ];

    /// Guitar pedals (shown on the pedalboard's + Pedal list first).
    pub fn is_pedal(self) -> bool {
        matches!(
            self,
            EffectKind::Overdrive
                | EffectKind::Fuzz
                | EffectKind::Amp
                | EffectKind::Phaser
                | EffectKind::Flanger
                | EffectKind::Tremolo
                | EffectKind::Wah
                | EffectKind::NoiseGate
                | EffectKind::Octaver
        )
    }

    pub fn display_name(self) -> &'static str {
        match self {
            EffectKind::Eq => "EQ",
            EffectKind::Compressor => "Compressor",
            EffectKind::Reverb => "Reverb",
            EffectKind::Delay => "Delay",
            EffectKind::Chorus => "Chorus",
            EffectKind::Distortion => "Distortion",
            EffectKind::Limiter => "Limiter",
            EffectKind::Overdrive => "Overdrive",
            EffectKind::Fuzz => "Fuzz",
            EffectKind::Amp => "Amp",
            EffectKind::Phaser => "Phaser",
            EffectKind::Flanger => "Flanger",
            EffectKind::Tremolo => "Tremolo",
            EffectKind::Wah => "Wah",
            EffectKind::NoiseGate => "Noise gate",
            EffectKind::Octaver => "Octaver",
            EffectKind::Plugin => "Plugin",
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
    /// Compressors only: the track whose sound drives the compressor
    /// (sidechain), e.g. the kick ducking a bass. None = its own input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidechain: Option<crate::project::TrackId>,
    /// Plugin effects: which plugin, and its settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<Box<crate::plugin::PluginRef>>,
}

impl Effect {
    /// A new effect with default settings.
    pub fn new(id: Id, kind: EffectKind) -> Self {
        Self {
            sidechain: None,
            plugin: None,
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

pub const AMP_CHANNELS: &[&str] = &["Clean", "Crunch", "High gain"];
pub const AMP_CABINETS: &[&str] = &["Off", "1x12 open back", "2x12", "4x12 closed"];
pub const TREMOLO_SHAPES: &[&str] = &["Smooth (sine)", "Choppy (square)"];
pub const WAH_MODES: &[&str] = &["Auto (follows playing)", "Sweep", "Fixed"];

pub const OVERDRIVE_PARAMS: &[ParamSpec] = &[
    p(
        "drive",
        "Drive",
        "Overdrive",
        0.0,
        1.0,
        0.5,
        Unit::Percent,
        false,
    ),
    p(
        "tone",
        "Tone",
        "Overdrive",
        0.0,
        1.0,
        0.5,
        Unit::Percent,
        false,
    ),
    p(
        "level_db",
        "Level",
        "Overdrive",
        -24.0,
        6.0,
        -3.0,
        Unit::Decibels,
        false,
    ),
];

pub const FUZZ_PARAMS: &[ParamSpec] = &[
    p("fuzz", "Fuzz", "Fuzz", 0.0, 1.0, 0.7, Unit::Percent, false),
    p("tone", "Tone", "Fuzz", 0.0, 1.0, 0.5, Unit::Percent, false),
    p(
        "level_db",
        "Level",
        "Fuzz",
        -24.0,
        6.0,
        -8.0,
        Unit::Decibels,
        false,
    ),
];

pub const AMP_PARAMS: &[ParamSpec] = &[
    choice("channel", "Channel", "Amp", AMP_CHANNELS, 1.0),
    p("gain", "Gain", "Amp", 0.0, 10.0, 5.0, Unit::None, false),
    p("bass", "Bass", "Amp", 0.0, 10.0, 5.0, Unit::None, false),
    p("mid", "Mid", "Amp", 0.0, 10.0, 5.0, Unit::None, false),
    p("treble", "Treble", "Amp", 0.0, 10.0, 5.0, Unit::None, false),
    p(
        "master_db",
        "Master",
        "Amp",
        -24.0,
        6.0,
        -6.0,
        Unit::Decibels,
        false,
    ),
    choice("cabinet", "Cabinet", "Amp", AMP_CABINETS, 2.0),
];

pub const PHASER_PARAMS: &[ParamSpec] = &[
    p(
        "rate_hz",
        "Rate",
        "Phaser",
        0.05,
        5.0,
        0.5,
        Unit::Hertz,
        true,
    ),
    p(
        "depth",
        "Depth",
        "Phaser",
        0.0,
        1.0,
        0.7,
        Unit::Percent,
        false,
    ),
    p(
        "feedback",
        "Feedback",
        "Phaser",
        0.0,
        0.9,
        0.4,
        Unit::Percent,
        false,
    ),
    p("mix", "Mix", "Phaser", 0.0, 1.0, 0.5, Unit::Percent, false),
];

pub const FLANGER_PARAMS: &[ParamSpec] = &[
    p(
        "rate_hz",
        "Rate",
        "Flanger",
        0.05,
        5.0,
        0.25,
        Unit::Hertz,
        true,
    ),
    p(
        "depth",
        "Depth",
        "Flanger",
        0.0,
        1.0,
        0.7,
        Unit::Percent,
        false,
    ),
    p(
        "feedback",
        "Feedback",
        "Flanger",
        -0.9,
        0.9,
        0.5,
        Unit::Percent,
        false,
    ),
    p("mix", "Mix", "Flanger", 0.0, 1.0, 0.5, Unit::Percent, false),
];

pub const TREMOLO_PARAMS: &[ParamSpec] = &[
    p(
        "rate_hz",
        "Rate",
        "Tremolo",
        0.5,
        15.0,
        5.0,
        Unit::Hertz,
        true,
    ),
    p(
        "depth",
        "Depth",
        "Tremolo",
        0.0,
        1.0,
        0.6,
        Unit::Percent,
        false,
    ),
    choice("shape", "Shape", "Tremolo", TREMOLO_SHAPES, 0.0),
];

pub const WAH_PARAMS: &[ParamSpec] = &[
    choice("mode", "Mode", "Wah", WAH_MODES, 0.0),
    p(
        "position",
        "Position",
        "Wah",
        0.0,
        1.0,
        0.5,
        Unit::Percent,
        false,
    ),
    p(
        "sensitivity",
        "Sensitivity",
        "Wah",
        0.0,
        1.0,
        0.6,
        Unit::Percent,
        false,
    ),
    p("rate_hz", "Rate", "Wah", 0.1, 8.0, 1.5, Unit::Hertz, true),
    p(
        "resonance",
        "Resonance",
        "Wah",
        0.0,
        1.0,
        0.7,
        Unit::Percent,
        false,
    ),
    p("mix", "Mix", "Wah", 0.0, 1.0, 1.0, Unit::Percent, false),
];

pub const NOISE_GATE_PARAMS: &[ParamSpec] = &[
    p(
        "threshold_db",
        "Threshold",
        "Noise gate",
        -90.0,
        -10.0,
        -55.0,
        Unit::Decibels,
        false,
    ),
    p(
        "release_s",
        "Release",
        "Noise gate",
        0.01,
        1.0,
        0.12,
        Unit::Seconds,
        true,
    ),
];

pub const OCTAVER_PARAMS: &[ParamSpec] = &[
    p(
        "down",
        "Octave down",
        "Octaver",
        0.0,
        1.0,
        0.6,
        Unit::Percent,
        false,
    ),
    p(
        "up",
        "Octave up",
        "Octaver",
        0.0,
        1.0,
        0.0,
        Unit::Percent,
        false,
    ),
    p("dry", "Dry", "Octaver", 0.0, 1.0, 0.8, Unit::Percent, false),
];

/// One effect of a rig: its kind and the settings that differ from the
/// defaults.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RigPedal {
    pub kind: EffectKind,
    pub params: &'static [(&'static str, f64)],
}

/// A ready-made guitar setup: a chain of pedals, an amp, and room effects.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Rig {
    pub name: &'static str,
    pub description: &'static str,
    pub chain: &'static [RigPedal],
}

const fn pedal(kind: EffectKind, params: &'static [(&'static str, f64)]) -> RigPedal {
    RigPedal { kind, params }
}

/// The rigs `load_rig` knows, in menu order.
pub const RIGS: &[Rig] = &[
    Rig {
        name: "Clean",
        description: "A clean amp with a touch of room: chords and picked parts.",
        chain: &[
            pedal(
                EffectKind::Compressor,
                &[("threshold_db", -24.0), ("ratio", 3.0), ("makeup_db", 4.0)],
            ),
            pedal(
                EffectKind::Amp,
                &[
                    ("channel", 0.0),
                    ("gain", 3.0),
                    ("treble", 6.0),
                    ("master_db", -4.0),
                ],
            ),
            pedal(EffectKind::Reverb, &[("size", 0.5), ("mix", 0.15)]),
        ],
    },
    Rig {
        name: "Clean shimmer",
        description: "Sparkling clean with chorus, dotted-eighth echoes and a big hall: dreamy towns and menus.",
        chain: &[
            pedal(
                EffectKind::Compressor,
                &[("threshold_db", -24.0), ("ratio", 3.0), ("makeup_db", 4.0)],
            ),
            pedal(
                EffectKind::Amp,
                &[
                    ("channel", 0.0),
                    ("gain", 3.0),
                    ("treble", 7.0),
                    ("master_db", -4.0),
                ],
            ),
            pedal(EffectKind::Chorus, &[("depth", 0.5), ("mix", 0.4)]),
            pedal(
                EffectKind::Delay,
                &[
                    ("sync", 3.0),
                    ("feedback", 0.35),
                    ("ping_pong", 1.0),
                    ("mix", 0.25),
                ],
            ),
            pedal(EffectKind::Reverb, &[("size", 0.85), ("mix", 0.35)]),
        ],
    },
    Rig {
        name: "Crunch",
        description: "An amp just breaking up, nudged by an overdrive: rhythm parts and riffs.",
        chain: &[
            pedal(
                EffectKind::Overdrive,
                &[("drive", 0.3), ("tone", 0.55), ("level_db", 0.0)],
            ),
            pedal(
                EffectKind::Amp,
                &[("channel", 1.0), ("gain", 5.0), ("mid", 6.0)],
            ),
            pedal(EffectKind::Reverb, &[("size", 0.45), ("mix", 0.12)]),
        ],
    },
    Rig {
        name: "Blues",
        description: "Warm, singing overdrive through an open-back combo: bends and slow leads.",
        chain: &[
            pedal(
                EffectKind::Overdrive,
                &[("drive", 0.45), ("tone", 0.45), ("level_db", 0.0)],
            ),
            pedal(
                EffectKind::Amp,
                &[
                    ("channel", 1.0),
                    ("gain", 4.0),
                    ("bass", 6.0),
                    ("mid", 6.0),
                    ("cabinet", 1.0),
                ],
            ),
            pedal(EffectKind::Reverb, &[("size", 0.55), ("mix", 0.2)]),
        ],
    },
    Rig {
        name: "Classic rock",
        description: "A cranked amp through a 4x12 with a short echo: power chords and anthems.",
        chain: &[
            pedal(
                EffectKind::Amp,
                &[
                    ("channel", 1.0),
                    ("gain", 8.0),
                    ("mid", 7.0),
                    ("cabinet", 3.0),
                ],
            ),
            pedal(
                EffectKind::Delay,
                &[("sync", 4.0), ("feedback", 0.25), ("mix", 0.15)],
            ),
            pedal(EffectKind::Reverb, &[("size", 0.5), ("mix", 0.15)]),
        ],
    },
    Rig {
        name: "Metal",
        description: "Gated high gain with a tight boost and scooped mids: chugs and boss fights.",
        chain: &[
            pedal(
                EffectKind::NoiseGate,
                &[("threshold_db", -50.0), ("release_s", 0.06)],
            ),
            pedal(
                EffectKind::Overdrive,
                &[("drive", 0.05), ("tone", 0.6), ("level_db", 0.0)],
            ),
            pedal(
                EffectKind::Amp,
                &[
                    ("channel", 2.0),
                    ("gain", 7.0),
                    ("bass", 6.0),
                    ("mid", 3.5),
                    ("treble", 6.5),
                    ("cabinet", 3.0),
                ],
            ),
            pedal(EffectKind::Reverb, &[("size", 0.4), ("mix", 0.08)]),
        ],
    },
    Rig {
        name: "Fuzz lead",
        description: "Thick fuzz into a clean amp with quarter-note echoes: soaring leads.",
        chain: &[
            pedal(
                EffectKind::Fuzz,
                &[("fuzz", 0.8), ("tone", 0.55), ("level_db", -8.0)],
            ),
            pedal(EffectKind::Amp, &[("channel", 0.0), ("gain", 5.0)]),
            pedal(
                EffectKind::Delay,
                &[("sync", 2.0), ("feedback", 0.4), ("mix", 0.25)],
            ),
            pedal(EffectKind::Reverb, &[("size", 0.6), ("mix", 0.2)]),
        ],
    },
    Rig {
        name: "Funk",
        description: "Squeezed and auto-wah'd clean: choppy sixteenth-note rhythm.",
        chain: &[
            pedal(
                EffectKind::Compressor,
                &[
                    ("threshold_db", -28.0),
                    ("ratio", 6.0),
                    ("attack_s", 0.003),
                    ("makeup_db", 6.0),
                ],
            ),
            pedal(
                EffectKind::Wah,
                &[("mode", 0.0), ("sensitivity", 0.7), ("resonance", 0.75)],
            ),
            pedal(
                EffectKind::Amp,
                &[("channel", 0.0), ("gain", 3.0), ("treble", 7.0)],
            ),
        ],
    },
    Rig {
        name: "Surf",
        description: "Twangy clean with tremolo and a splashy spring-style reverb: beaches and westerns.",
        chain: &[
            pedal(
                EffectKind::Amp,
                &[
                    ("channel", 0.0),
                    ("gain", 4.0),
                    ("treble", 7.0),
                    ("cabinet", 1.0),
                ],
            ),
            pedal(EffectKind::Tremolo, &[("rate_hz", 6.0), ("depth", 0.5)]),
            pedal(
                EffectKind::Reverb,
                &[("size", 0.7), ("damping", 0.2), ("mix", 0.45)],
            ),
        ],
    },
    Rig {
        name: "Ambient",
        description: "Clean guitar washed in long echoes and a huge reverb: pads, title screens.",
        chain: &[
            pedal(
                EffectKind::Amp,
                &[("channel", 0.0), ("gain", 3.0), ("master_db", -4.0)],
            ),
            pedal(EffectKind::Chorus, &[("depth", 0.4), ("mix", 0.3)]),
            pedal(
                EffectKind::Delay,
                &[
                    ("sync", 2.0),
                    ("feedback", 0.6),
                    ("ping_pong", 1.0),
                    ("mix", 0.4),
                ],
            ),
            pedal(EffectKind::Reverb, &[("size", 1.0), ("mix", 0.6)]),
        ],
    },
];

pub fn rig(name: &str) -> Option<&'static Rig> {
    RIGS.iter()
        .find(|r| r.name.eq_ignore_ascii_case(name.trim()))
}

pub fn effect_params(kind: EffectKind) -> &'static [ParamSpec] {
    match kind {
        EffectKind::Eq => EQ_PARAMS,
        EffectKind::Compressor => COMPRESSOR_PARAMS,
        EffectKind::Reverb => REVERB_PARAMS,
        EffectKind::Delay => DELAY_PARAMS,
        EffectKind::Chorus => CHORUS_PARAMS,
        EffectKind::Distortion => DISTORTION_PARAMS,
        EffectKind::Limiter => LIMITER_PARAMS,
        EffectKind::Overdrive => OVERDRIVE_PARAMS,
        EffectKind::Fuzz => FUZZ_PARAMS,
        EffectKind::Amp => AMP_PARAMS,
        EffectKind::Phaser => PHASER_PARAMS,
        EffectKind::Flanger => FLANGER_PARAMS,
        EffectKind::Tremolo => TREMOLO_PARAMS,
        EffectKind::Wah => WAH_PARAMS,
        EffectKind::NoiseGate => NOISE_GATE_PARAMS,
        EffectKind::Octaver => OCTAVER_PARAMS,
        EffectKind::Plugin => &[],
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
                EffectKind::Overdrive => "Guitar pedal: a warm, mid-boosted push. Before an Amp for crunch, or alone for a gritty edge.",
                EffectKind::Fuzz => "Guitar pedal: hard, buzzy, square-ish fuzz for 60s and stoner tones.",
                EffectKind::Amp => "A guitar amp and speaker cabinet. Channel Clean/Crunch/High gain, gain, bass/mid/treble, master, cabinet. Put it after drive pedals and before time effects.",
                EffectKind::Phaser => "Guitar pedal: a swirling sweep.",
                EffectKind::Flanger => "Guitar pedal: a jet-plane whoosh.",
                EffectKind::Tremolo => "Guitar pedal: volume pulsing, for surf and western.",
                EffectKind::Wah => "Guitar pedal: a band-pass 'wow'. Auto follows how hard notes are played; Sweep moves by itself; Fixed holds Position (automate it to rock the pedal).",
                EffectKind::NoiseGate => "Silences hiss and hum between phrases; tightens high-gain chugs. Put it first.",
                EffectKind::Octaver => "Guitar pedal: adds a note an octave below (and a fuzzy octave above). Works best on single notes.",
                EffectKind::Plugin => "A VST3 effect you installed.",
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

    #[test]
    fn every_rig_uses_real_settings() {
        for r in RIGS {
            assert!(!r.chain.is_empty(), "{}", r.name);
            for pedal in r.chain {
                assert_ne!(pedal.kind, EffectKind::Plugin);
                for (id, value) in pedal.params {
                    let spec = effect_spec(pedal.kind, id)
                        .unwrap_or_else(|| panic!("{}: {:?} has no {id}", r.name, pedal.kind));
                    assert!(spec.validate(*value), "{}: {id} = {value}", r.name);
                }
            }
        }
        assert_eq!(rig("metal").map(|r| r.name), Some("Metal"));
    }
}
