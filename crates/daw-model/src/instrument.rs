//! Built-in instruments as data: which parameters each has, their ranges,
//! and the factory presets. The DSP lives in `daw-instruments`; this module
//! is what gets saved, validated, shown in the UI, and described to Claude.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Which built-in instrument a track plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentKind {
    /// Polyphonic subtractive synthesizer: keys, pads, leads, and basses.
    Synth,
    /// 16-pad synthesized drum machine on General MIDI notes 36–51.
    Drums,
    /// Plays recorded and imported audio (voice, guitar, phone recordings)
    /// instead of notes. Its clips hold audio, not MIDI.
    Audio,
    /// Plays a sampled instrument from an SFZ sample pack on disk (a real
    /// piano, bass, strings...). Set the pack with load_sample_pack.
    Sampler,
}

impl InstrumentKind {
    pub const ALL: [InstrumentKind; 4] = [
        InstrumentKind::Synth,
        InstrumentKind::Drums,
        InstrumentKind::Audio,
        InstrumentKind::Sampler,
    ];

    /// Audio tracks hold audio clips; every other kind holds note clips.
    pub fn is_audio(self) -> bool {
        self == InstrumentKind::Audio
    }
}

/// A track's instrument settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Instrument {
    pub kind: InstrumentKind,
    /// Name of the preset these settings started from.
    pub preset: String,
    /// Parameter values keyed by parameter id (see `describe_instrument`).
    pub params: BTreeMap<String, f64>,
    /// For samplers: the SFZ file of the sample pack, as an absolute path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_pack: Option<String>,
}

impl Instrument {
    /// An instrument loaded with one of its factory presets.
    pub fn from_preset(kind: InstrumentKind, preset: &str) -> Option<Self> {
        let preset_def = presets(kind).iter().find(|p| p.name == preset)?;
        let mut params: BTreeMap<String, f64> = param_specs(kind)
            .iter()
            .map(|s| (s.id.to_owned(), s.default))
            .collect();
        for (id, value) in preset_def.values {
            params.insert((*id).to_owned(), *value);
        }
        Some(Self {
            kind,
            preset: preset_def.name.to_owned(),
            params,
            sample_pack: None,
        })
    }

    /// The current value of a parameter, falling back to its default.
    pub fn value(&self, id: &str) -> Option<f64> {
        self.params
            .get(id)
            .copied()
            .or_else(|| spec(self.kind, id).map(|s| s.default))
    }
}

/// How a parameter's number should be shown and edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    None,
    Hertz,
    Seconds,
    Decibels,
    Semitones,
    Cents,
    /// Stored 0.0–1.0, shown as 0–100%.
    Percent,
    /// Compression ratio, shown as "4:1".
    Ratio,
}

/// Static description of one instrument parameter.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ParamSpec {
    /// Stable identifier used in project files and Commands.
    pub id: &'static str,
    /// Human-readable label.
    pub name: &'static str,
    /// UI section, such as "Filter" or "Amp envelope".
    pub group: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub unit: Unit,
    /// True when the slider should be logarithmic (frequencies, times).
    pub log_scale: bool,
    /// For choice parameters, the option names; the value is the index.
    pub choices: &'static [&'static str],
}

impl ParamSpec {
    /// Checks a value against the range (and integer index for choices).
    pub fn validate(&self, value: f64) -> bool {
        let in_range = value.is_finite() && value >= self.min && value <= self.max;
        let integral = self.choices.is_empty() || value.fract() == 0.0;
        in_range && integral
    }
}

#[allow(clippy::too_many_arguments)] // A table row; named args would be noisier.
pub(crate) const fn p(
    id: &'static str,
    name: &'static str,
    group: &'static str,
    min: f64,
    max: f64,
    default: f64,
    unit: Unit,
    log_scale: bool,
) -> ParamSpec {
    ParamSpec {
        id,
        name,
        group,
        min,
        max,
        default,
        unit,
        log_scale,
        choices: &[],
    }
}

pub(crate) const fn choice(
    id: &'static str,
    name: &'static str,
    group: &'static str,
    choices: &'static [&'static str],
    default: f64,
) -> ParamSpec {
    ParamSpec {
        id,
        name,
        group,
        min: 0.0,
        max: (choices.len() - 1) as f64,
        default,
        unit: Unit::None,
        log_scale: false,
        choices,
    }
}

const WAVES: &[&str] = &["Sine", "Saw", "Square", "Triangle"];

/// Synth parameters. The order is the engine's parameter index: append only.
pub const SYNTH_PARAMS: &[ParamSpec] = &[
    choice("osc1.wave", "Wave", "Oscillator 1", WAVES, 1.0),
    p(
        "osc1.level",
        "Level",
        "Oscillator 1",
        0.0,
        1.0,
        0.8,
        Unit::Percent,
        false,
    ),
    choice("osc2.wave", "Wave", "Oscillator 2", WAVES, 1.0),
    p(
        "osc2.level",
        "Level",
        "Oscillator 2",
        0.0,
        1.0,
        0.5,
        Unit::Percent,
        false,
    ),
    p(
        "osc2.semitones",
        "Pitch",
        "Oscillator 2",
        -24.0,
        24.0,
        0.0,
        Unit::Semitones,
        false,
    ),
    p(
        "osc2.detune_cents",
        "Detune",
        "Oscillator 2",
        -50.0,
        50.0,
        7.0,
        Unit::Cents,
        false,
    ),
    p(
        "osc.pulse_width",
        "Pulse width",
        "Oscillator 1",
        0.05,
        0.95,
        0.5,
        Unit::Percent,
        false,
    ),
    p(
        "sub.level",
        "Sub",
        "Mixer",
        0.0,
        1.0,
        0.0,
        Unit::Percent,
        false,
    ),
    p(
        "noise.level",
        "Noise",
        "Mixer",
        0.0,
        1.0,
        0.0,
        Unit::Percent,
        false,
    ),
    choice(
        "filter.mode",
        "Mode",
        "Filter",
        &["Low-pass", "Band-pass", "High-pass"],
        0.0,
    ),
    p(
        "filter.cutoff_hz",
        "Cutoff",
        "Filter",
        20.0,
        20_000.0,
        2_000.0,
        Unit::Hertz,
        true,
    ),
    p(
        "filter.resonance",
        "Resonance",
        "Filter",
        0.0,
        1.0,
        0.2,
        Unit::Percent,
        false,
    ),
    p(
        "filter.env_amount",
        "Env amount",
        "Filter",
        -1.0,
        1.0,
        0.3,
        Unit::Percent,
        false,
    ),
    p(
        "filter.key_track",
        "Key track",
        "Filter",
        0.0,
        1.0,
        0.5,
        Unit::Percent,
        false,
    ),
    p(
        "amp.attack_s",
        "Attack",
        "Amp envelope",
        0.001,
        10.0,
        0.005,
        Unit::Seconds,
        true,
    ),
    p(
        "amp.decay_s",
        "Decay",
        "Amp envelope",
        0.001,
        10.0,
        0.3,
        Unit::Seconds,
        true,
    ),
    p(
        "amp.sustain",
        "Sustain",
        "Amp envelope",
        0.0,
        1.0,
        0.7,
        Unit::Percent,
        false,
    ),
    p(
        "amp.release_s",
        "Release",
        "Amp envelope",
        0.001,
        10.0,
        0.3,
        Unit::Seconds,
        true,
    ),
    p(
        "fenv.attack_s",
        "Attack",
        "Filter envelope",
        0.001,
        10.0,
        0.005,
        Unit::Seconds,
        true,
    ),
    p(
        "fenv.decay_s",
        "Decay",
        "Filter envelope",
        0.001,
        10.0,
        0.4,
        Unit::Seconds,
        true,
    ),
    p(
        "fenv.sustain",
        "Sustain",
        "Filter envelope",
        0.0,
        1.0,
        0.3,
        Unit::Percent,
        false,
    ),
    p(
        "fenv.release_s",
        "Release",
        "Filter envelope",
        0.001,
        10.0,
        0.3,
        Unit::Seconds,
        true,
    ),
    p(
        "lfo.rate_hz",
        "Rate",
        "LFO",
        0.05,
        20.0,
        5.0,
        Unit::Hertz,
        true,
    ),
    p(
        "lfo.to_pitch_cents",
        "Vibrato",
        "LFO",
        0.0,
        100.0,
        0.0,
        Unit::Cents,
        false,
    ),
    p(
        "lfo.to_cutoff",
        "Filter wobble",
        "LFO",
        0.0,
        1.0,
        0.0,
        Unit::Percent,
        false,
    ),
    choice("voice.mode", "Voices", "Voice", &["Poly", "Mono"], 0.0),
    p(
        "voice.glide_s",
        "Glide",
        "Voice",
        0.0,
        1.0,
        0.0,
        Unit::Seconds,
        false,
    ),
    p(
        "velocity.sensitivity",
        "Velocity",
        "Voice",
        0.0,
        1.0,
        0.7,
        Unit::Percent,
        false,
    ),
    p(
        "master.gain_db",
        "Volume",
        "Output",
        -36.0,
        6.0,
        -6.0,
        Unit::Decibels,
        false,
    ),
];

/// Drum machine parameters. The order is the engine's parameter index: append only.
pub const DRUM_PARAMS: &[ParamSpec] = &[
    p(
        "kick.level",
        "Kick",
        "Levels",
        0.0,
        1.0,
        0.9,
        Unit::Percent,
        false,
    ),
    p(
        "snare.level",
        "Snare",
        "Levels",
        0.0,
        1.0,
        0.75,
        Unit::Percent,
        false,
    ),
    p(
        "hats.level",
        "Hi-hats",
        "Levels",
        0.0,
        1.0,
        0.5,
        Unit::Percent,
        false,
    ),
    p(
        "toms.level",
        "Toms",
        "Levels",
        0.0,
        1.0,
        0.7,
        Unit::Percent,
        false,
    ),
    p(
        "perc.level",
        "Clap & perc",
        "Levels",
        0.0,
        1.0,
        0.65,
        Unit::Percent,
        false,
    ),
    p(
        "cymbals.level",
        "Cymbals",
        "Levels",
        0.0,
        1.0,
        0.45,
        Unit::Percent,
        false,
    ),
    p(
        "kick.tune_semitones",
        "Kick tune",
        "Kick",
        -12.0,
        12.0,
        0.0,
        Unit::Semitones,
        false,
    ),
    p(
        "kick.decay",
        "Kick decay",
        "Kick",
        0.25,
        2.0,
        1.0,
        Unit::None,
        false,
    ),
    p(
        "snare.tone",
        "Snare tone",
        "Snare",
        0.0,
        1.0,
        0.5,
        Unit::Percent,
        false,
    ),
    p(
        "hats.decay",
        "Hat decay",
        "Hi-hats",
        0.25,
        2.0,
        1.0,
        Unit::None,
        false,
    ),
    p(
        "kit.tune_semitones",
        "Kit tune",
        "Kit",
        -12.0,
        12.0,
        0.0,
        Unit::Semitones,
        false,
    ),
    p(
        "kit.decay",
        "Kit decay",
        "Kit",
        0.25,
        2.0,
        1.0,
        Unit::None,
        false,
    ),
    p(
        "master.gain_db",
        "Volume",
        "Output",
        -36.0,
        6.0,
        -6.0,
        Unit::Decibels,
        false,
    ),
];

/// Parameter list for an instrument kind, in engine index order.
pub fn param_specs(kind: InstrumentKind) -> &'static [ParamSpec] {
    match kind {
        InstrumentKind::Synth => SYNTH_PARAMS,
        InstrumentKind::Drums => DRUM_PARAMS,
        InstrumentKind::Audio => &[],
        InstrumentKind::Sampler => SAMPLER_PARAMS,
    }
}

/// Sampler parameters. The order is the engine's parameter index: append only.
pub const SAMPLER_PARAMS: &[ParamSpec] = &[
    p(
        "master.gain_db",
        "Volume",
        "Output",
        -36.0,
        12.0,
        0.0,
        Unit::Decibels,
        false,
    ),
    p(
        "tune.cents",
        "Fine tune",
        "Pitch",
        -100.0,
        100.0,
        0.0,
        Unit::Cents,
        false,
    ),
    p(
        "tune.semitones",
        "Transpose",
        "Pitch",
        -24.0,
        24.0,
        0.0,
        Unit::Semitones,
        false,
    ),
    p(
        "amp.attack_s",
        "Attack",
        "Envelope",
        0.0,
        2.0,
        0.0,
        Unit::Seconds,
        false,
    ),
    p(
        "amp.release_s",
        "Release",
        "Envelope",
        0.01,
        5.0,
        0.4,
        Unit::Seconds,
        true,
    ),
    p(
        "velocity.sensitivity",
        "Velocity",
        "Envelope",
        0.0,
        1.0,
        1.0,
        Unit::Percent,
        false,
    ),
];

pub fn spec(kind: InstrumentKind, id: &str) -> Option<&'static ParamSpec> {
    param_specs(kind).iter().find(|s| s.id == id)
}

/// Engine index of a parameter.
pub fn param_index(kind: InstrumentKind, id: &str) -> Option<usize> {
    param_specs(kind).iter().position(|s| s.id == id)
}

/// A factory preset: a name plus the parameters that differ from defaults.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Preset {
    pub name: &'static str,
    pub description: &'static str,
    #[serde(skip)]
    pub values: &'static [(&'static str, f64)],
}

pub const SYNTH_PRESETS: &[Preset] = &[
    Preset {
        name: "Init",
        description: "Plain two-saw starting point.",
        values: &[],
    },
    Preset {
        name: "Warm Keys",
        description: "Soft, slightly detuned keys for chords.",
        values: &[
            ("osc1.wave", 3.0),
            ("osc2.wave", 1.0),
            ("osc2.level", 0.35),
            ("filter.cutoff_hz", 1_800.0),
            ("filter.env_amount", 0.25),
            ("amp.attack_s", 0.004),
            ("amp.decay_s", 1.2),
            ("amp.sustain", 0.45),
            ("amp.release_s", 0.4),
            ("fenv.decay_s", 0.8),
        ],
    },
    Preset {
        name: "Soft Pad",
        description: "Slow, wide pad for backgrounds and ambience.",
        values: &[
            ("osc2.detune_cents", 14.0),
            ("osc2.level", 0.8),
            ("filter.cutoff_hz", 1_200.0),
            ("filter.env_amount", 0.15),
            ("amp.attack_s", 0.8),
            ("amp.decay_s", 1.5),
            ("amp.sustain", 0.8),
            ("amp.release_s", 1.8),
            ("fenv.attack_s", 1.0),
            ("lfo.rate_hz", 0.3),
            ("lfo.to_cutoff", 0.15),
            ("master.gain_db", -9.0),
        ],
    },
    Preset {
        name: "Bright Lead",
        description: "Cutting mono lead with vibrato and a little glide.",
        values: &[
            ("osc2.semitones", 12.0),
            ("osc2.level", 0.4),
            ("filter.cutoff_hz", 4_500.0),
            ("filter.resonance", 0.35),
            ("amp.sustain", 0.85),
            ("amp.release_s", 0.15),
            ("lfo.rate_hz", 5.5),
            ("lfo.to_pitch_cents", 12.0),
            ("voice.mode", 1.0),
            ("voice.glide_s", 0.04),
        ],
    },
    Preset {
        name: "Pluck",
        description: "Short, percussive pluck for arpeggios.",
        values: &[
            ("osc1.wave", 2.0),
            ("osc2.level", 0.3),
            ("filter.cutoff_hz", 600.0),
            ("filter.resonance", 0.4),
            ("filter.env_amount", 0.7),
            ("amp.decay_s", 0.35),
            ("amp.sustain", 0.0),
            ("amp.release_s", 0.25),
            ("fenv.decay_s", 0.18),
            ("fenv.sustain", 0.0),
        ],
    },
    Preset {
        name: "Chip Square",
        description: "8-bit style square lead for retro game music.",
        values: &[
            ("osc1.wave", 2.0),
            ("osc.pulse_width", 0.25),
            ("osc2.level", 0.0),
            ("filter.cutoff_hz", 20_000.0),
            ("filter.resonance", 0.0),
            ("filter.env_amount", 0.0),
            ("amp.attack_s", 0.001),
            ("amp.sustain", 0.8),
            ("amp.release_s", 0.05),
            ("velocity.sensitivity", 0.2),
        ],
    },
    Preset {
        name: "Brass Stab",
        description: "Punchy brass-like stab with a filter swell.",
        values: &[
            ("osc2.detune_cents", 10.0),
            ("osc2.level", 0.7),
            ("filter.cutoff_hz", 700.0),
            ("filter.resonance", 0.15),
            ("filter.env_amount", 0.6),
            ("amp.attack_s", 0.02),
            ("amp.sustain", 0.75),
            ("amp.release_s", 0.2),
            ("fenv.attack_s", 0.05),
            ("fenv.decay_s", 0.5),
            ("fenv.sustain", 0.4),
        ],
    },
    Preset {
        name: "Sub Bass",
        description: "Clean, deep sine-like bass that sits under everything.",
        values: &[
            ("osc1.wave", 0.0),
            ("osc2.wave", 3.0),
            ("osc2.semitones", 12.0),
            ("osc2.level", 0.15),
            ("sub.level", 0.4),
            ("filter.cutoff_hz", 400.0),
            ("filter.env_amount", 0.0),
            ("filter.key_track", 0.0),
            ("amp.attack_s", 0.003),
            ("amp.sustain", 0.9),
            ("amp.release_s", 0.08),
            ("voice.mode", 1.0),
            ("master.gain_db", -3.0),
        ],
    },
    Preset {
        name: "Fat Bass",
        description: "Thick detuned saw bass with a little glide.",
        values: &[
            ("osc2.detune_cents", 12.0),
            ("osc2.level", 0.8),
            ("sub.level", 0.5),
            ("filter.cutoff_hz", 500.0),
            ("filter.resonance", 0.25),
            ("filter.env_amount", 0.45),
            ("filter.key_track", 0.3),
            ("amp.attack_s", 0.003),
            ("amp.sustain", 0.8),
            ("amp.release_s", 0.1),
            ("fenv.decay_s", 0.25),
            ("fenv.sustain", 0.2),
            ("voice.mode", 1.0),
            ("voice.glide_s", 0.05),
        ],
    },
    Preset {
        name: "Acid Bass",
        description: "Squelchy resonant bass for driving loops.",
        values: &[
            ("osc1.wave", 1.0),
            ("osc2.level", 0.0),
            ("filter.cutoff_hz", 300.0),
            ("filter.resonance", 0.8),
            ("filter.env_amount", 0.8),
            ("filter.key_track", 0.2),
            ("amp.attack_s", 0.002),
            ("amp.sustain", 0.7),
            ("amp.release_s", 0.06),
            ("fenv.decay_s", 0.2),
            ("fenv.sustain", 0.0),
            ("voice.mode", 1.0),
            ("voice.glide_s", 0.06),
        ],
    },
];

pub const DRUM_PRESETS: &[Preset] = &[
    Preset {
        name: "Classic Kit",
        description: "Balanced analog-style kit.",
        values: &[],
    },
    Preset {
        name: "Tight Kit",
        description: "Short, dry hits for fast patterns.",
        values: &[
            ("kick.decay", 0.6),
            ("hats.decay", 0.6),
            ("kit.decay", 0.7),
            ("snare.tone", 0.35),
        ],
    },
    Preset {
        name: "Boomy Kit",
        description: "Long, deep kick and big toms for epic moments.",
        values: &[
            ("kick.tune_semitones", -3.0),
            ("kick.decay", 1.8),
            ("kit.decay", 1.4),
            ("toms.level", 0.85),
            ("snare.tone", 0.7),
        ],
    },
];

pub fn presets(kind: InstrumentKind) -> &'static [Preset] {
    match kind {
        InstrumentKind::Synth => SYNTH_PRESETS,
        InstrumentKind::Drums => DRUM_PRESETS,
        InstrumentKind::Audio => AUDIO_PRESETS,
        InstrumentKind::Sampler => SAMPLER_PRESETS,
    }
}

/// Samplers sound like their sample pack; this preset just resets the
/// controls.
pub const SAMPLER_PRESETS: &[Preset] = &[Preset {
    name: "Sample pack",
    description: "Plays the loaded SFZ sample pack as recorded.",
    values: &[],
}];

/// Audio tracks have no sound settings of their own; this single "preset"
/// keeps them uniform with instrument tracks.
pub const AUDIO_PRESETS: &[Preset] = &[Preset {
    name: "Audio",
    description: "Recorded or imported audio.",
    values: &[],
}];

/// Everything the UI and Claude need to know about an instrument kind.
#[derive(Debug, Clone, Serialize)]
pub struct InstrumentDescription {
    pub kind: InstrumentKind,
    pub params: &'static [ParamSpec],
    pub presets: &'static [Preset],
}

pub fn describe_instruments() -> Vec<InstrumentDescription> {
    InstrumentKind::ALL
        .iter()
        .map(|&kind| InstrumentDescription {
            kind,
            params: param_specs(kind),
            presets: presets(kind),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_ids_are_unique_and_defaults_valid() {
        for kind in InstrumentKind::ALL {
            let specs = param_specs(kind);
            for (i, s) in specs.iter().enumerate() {
                assert!(s.validate(s.default), "{kind:?} {}", s.id);
                assert!(s.min < s.max, "{kind:?} {}", s.id);
                assert!(
                    specs[..i].iter().all(|o| o.id != s.id),
                    "duplicate {}",
                    s.id
                );
            }
        }
    }

    #[test]
    fn every_preset_loads_with_valid_values() {
        for kind in InstrumentKind::ALL {
            for preset in presets(kind) {
                for (id, value) in preset.values {
                    let s = spec(kind, id)
                        .unwrap_or_else(|| panic!("{} uses unknown param {id}", preset.name));
                    assert!(s.validate(*value), "{} {id}={value}", preset.name);
                }
                let inst = Instrument::from_preset(kind, preset.name).expect("loads");
                assert_eq!(inst.params.len(), param_specs(kind).len());
            }
        }
    }

    #[test]
    fn choice_params_reject_fractions() {
        let s = spec(InstrumentKind::Synth, "osc1.wave").expect("exists");
        assert!(s.validate(2.0));
        assert!(!s.validate(1.5));
        assert!(!s.validate(4.0));
    }
}
