//! Game sound effects (coin, jump, laser...): small recipes in the style of
//! sfxr, kept in the project and rendered by `daw-sfx`.
//!
//! The category generators and `vary` follow sfxr's (`sfxr.cpp` by Tomas
//! Pettersson, MIT; see THIRD_PARTY.md), so recipes behave the way sfxr
//! users expect. The fields keep sfxr's normalised ranges for the same
//! reason.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::command::{CommandError, invalid};
use crate::project::Id;

/// The basic tone a sound effect is made from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum Wave {
    /// Hollow, retro (width set by `duty`).
    #[default]
    Square,
    /// Bright and buzzy.
    Saw,
    /// Pure and soft.
    Sine,
    /// Between sine and square: soft but present.
    Triangle,
    /// Hiss: explosions, hits, wind.
    Noise,
}

/// A kind of sound to generate a new recipe for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SoundCategory {
    Coin,
    Jump,
    Laser,
    Explosion,
    Hit,
    PowerUp,
    Blip,
    /// Anything at all.
    Random,
}

impl SoundCategory {
    pub const ALL: [SoundCategory; 8] = [
        SoundCategory::Coin,
        SoundCategory::Jump,
        SoundCategory::Laser,
        SoundCategory::Explosion,
        SoundCategory::Hit,
        SoundCategory::PowerUp,
        SoundCategory::Blip,
        SoundCategory::Random,
    ];

    /// The name a new sound of this kind gets.
    pub fn default_name(self) -> &'static str {
        match self {
            SoundCategory::Coin => "coin",
            SoundCategory::Jump => "jump",
            SoundCategory::Laser => "laser",
            SoundCategory::Explosion => "explosion",
            SoundCategory::Hit => "hit",
            SoundCategory::PowerUp => "power_up",
            SoundCategory::Blip => "blip",
            SoundCategory::Random => "random",
        }
    }
}

/// How to make one sound effect. Fields are 0 to 1 unless marked -1 to 1;
/// values outside are clamped. Any field left out takes the default (a
/// short square blip), so a partial recipe is fine.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SoundRecipe {
    /// The basic tone.
    pub wave: Wave,
    /// Fade-in time (0 = instant; length grows with the square).
    pub attack: f32,
    /// How long it holds at full volume.
    pub sustain: f32,
    /// Extra loudness at the start of the hold, for a "ping" (0 = none).
    pub punch: f32,
    /// Fade-out time.
    pub decay: f32,
    /// Starting pitch (0 = very low, 1 = very high; about 0.3 is middle).
    pub base_freq: f32,
    /// Lowest pitch: if the pitch slides below it the sound stops (0 = no
    /// limit).
    pub freq_limit: f32,
    /// Pitch slide, -1 to 1: negative falls (lasers), positive rises (jumps).
    pub freq_ramp: f32,
    /// Change of the slide over time, -1 to 1 (curves the slide).
    pub freq_dramp: f32,
    /// Vibrato depth (pitch wobble).
    pub vib_strength: f32,
    /// Vibrato speed.
    pub vib_speed: f32,
    /// Pitch jump, -1 to 1: positive jumps up (coin "ding-ding"), negative
    /// down.
    pub arp_mod: f32,
    /// When the pitch jump happens (higher = sooner; 0 = never... use
    /// arp_mod 0 for no jump).
    pub arp_speed: f32,
    /// Square wave width (0 = even and hollow, 1 = thin and nasal).
    pub duty: f32,
    /// Square width sweep, -1 to 1.
    pub duty_ramp: f32,
    /// Restarts the pitch slide and jump this often (0 = off): trills,
    /// alarms, rolling explosions.
    pub repeat_speed: f32,
    /// Phaser (a swirling comb filter) offset, -1 to 1 (0 = off).
    pub phaser_offset: f32,
    /// Phaser sweep, -1 to 1.
    pub phaser_ramp: f32,
    /// Low-pass filter: lower is duller (1 = off).
    pub lpf_freq: f32,
    /// Low-pass sweep, -1 to 1.
    pub lpf_ramp: f32,
    /// Low-pass resonance (a whistling peak at the cut-off).
    pub lpf_resonance: f32,
    /// High-pass filter: higher is thinner (0 = off).
    pub hpf_freq: f32,
    /// High-pass sweep, -1 to 1.
    pub hpf_ramp: f32,
    /// Bit crush: lo-fi grit, fewer bits and a lower sample rate (0 = off).
    pub crush: f32,
    /// Overall level in dB, -24 to +6.
    pub volume_db: f32,
}

impl Default for SoundRecipe {
    fn default() -> Self {
        Self {
            wave: Wave::Square,
            attack: 0.0,
            sustain: 0.1,
            punch: 0.0,
            decay: 0.2,
            base_freq: 0.5,
            freq_limit: 0.0,
            freq_ramp: 0.0,
            freq_dramp: 0.0,
            vib_strength: 0.0,
            vib_speed: 0.0,
            arp_mod: 0.0,
            arp_speed: 0.0,
            duty: 0.0,
            duty_ramp: 0.0,
            repeat_speed: 0.0,
            phaser_offset: 0.0,
            phaser_ramp: 0.0,
            lpf_freq: 1.0,
            lpf_ramp: 0.0,
            lpf_resonance: 0.0,
            hpf_freq: 0.0,
            hpf_ramp: 0.0,
            crush: 0.0,
            volume_db: 0.0,
        }
    }
}

/// Quietest and loudest `volume_db`.
pub const MIN_SOUND_VOLUME_DB: f32 = -24.0;
pub const MAX_SOUND_VOLUME_DB: f32 = 6.0;

/// Longest sound, in seconds; longer recipes are cut off.
pub const MAX_SOUND_SECONDS: f32 = 10.0;

/// Visits every number field with its name and range, mutably.
macro_rules! for_each_field {
    ($r:expr, $f:expr) => {{
        let r = $r;
        let mut f = $f;
        f("attack", &mut r.attack, 0.0, 1.0);
        f("sustain", &mut r.sustain, 0.0, 1.0);
        f("punch", &mut r.punch, 0.0, 1.0);
        f("decay", &mut r.decay, 0.0, 1.0);
        f("base_freq", &mut r.base_freq, 0.0, 1.0);
        f("freq_limit", &mut r.freq_limit, 0.0, 1.0);
        f("freq_ramp", &mut r.freq_ramp, -1.0, 1.0);
        f("freq_dramp", &mut r.freq_dramp, -1.0, 1.0);
        f("vib_strength", &mut r.vib_strength, 0.0, 1.0);
        f("vib_speed", &mut r.vib_speed, 0.0, 1.0);
        f("arp_mod", &mut r.arp_mod, -1.0, 1.0);
        f("arp_speed", &mut r.arp_speed, 0.0, 1.0);
        f("duty", &mut r.duty, 0.0, 1.0);
        f("duty_ramp", &mut r.duty_ramp, -1.0, 1.0);
        f("repeat_speed", &mut r.repeat_speed, 0.0, 1.0);
        f("phaser_offset", &mut r.phaser_offset, -1.0, 1.0);
        f("phaser_ramp", &mut r.phaser_ramp, -1.0, 1.0);
        f("lpf_freq", &mut r.lpf_freq, 0.0, 1.0);
        f("lpf_ramp", &mut r.lpf_ramp, -1.0, 1.0);
        f("lpf_resonance", &mut r.lpf_resonance, 0.0, 1.0);
        f("hpf_freq", &mut r.hpf_freq, 0.0, 1.0);
        f("hpf_ramp", &mut r.hpf_ramp, -1.0, 1.0);
        f("crush", &mut r.crush, 0.0, 1.0);
        f(
            "volume_db",
            &mut r.volume_db,
            MIN_SOUND_VOLUME_DB,
            MAX_SOUND_VOLUME_DB,
        );
    }};
}

impl SoundRecipe {
    /// The recipe with every field in its range; NaN or infinity is an
    /// error naming the field.
    pub fn clamped(mut self) -> Result<SoundRecipe, CommandError> {
        let mut bad = None;
        for_each_field!(&mut self, |name: &'static str,
                                    v: &mut f32,
                                    lo: f32,
                                    hi: f32| {
            if !v.is_finite() {
                bad.get_or_insert(name);
            } else {
                *v = v.clamp(lo, hi);
            }
        });
        match bad {
            Some(name) => Err(invalid(
                "sound",
                format!("{name} must be a number, not NaN or infinity"),
            )),
            None => Ok(self),
        }
    }

    /// Clamps without checking for NaN (generators never make one).
    fn clamp_all(self) -> SoundRecipe {
        self.clamped().unwrap_or_default()
    }

    /// Length in seconds: attack, hold, and fade (at most
    /// [`MAX_SOUND_SECONDS`]).
    pub fn seconds(&self) -> f32 {
        let samples = (self.attack.powi(2) + self.sustain.powi(2) + self.decay.powi(2)) * 100_000.0;
        (samples / 44_100.0).min(MAX_SOUND_SECONDS)
    }
}

/// One game sound effect in the project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SoundEffect {
    pub id: Id,
    /// Also its file name when exported (`coin` -> `coin.wav`).
    pub name: String,
    pub recipe: SoundRecipe,
}

/// Small deterministic random numbers (xorshift64*), so a seed always
/// makes the same sound on every computer.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // Mix the seed so nearby seeds differ, and never start at zero.
        let s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
        Rng(if s == 0 { 1 } else { s })
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A float in `0..range`.
    fn frnd(&mut self, range: f32) -> f32 {
        ((self.next() >> 40) as f32 / (1u64 << 24) as f32) * range
    }

    /// Whole number `0..=n`.
    fn rnd(&mut self, n: u64) -> u64 {
        self.next() % (n + 1)
    }

    fn coin(&mut self) -> bool {
        self.rnd(1) == 1
    }
}

fn wave_from_sfxr(i: u64) -> Wave {
    match i {
        0 => Wave::Square,
        1 => Wave::Saw,
        2 => Wave::Sine,
        _ => Wave::Noise,
    }
}

/// A new recipe of `category`. The same seed always gives the same recipe.
pub fn generate(category: SoundCategory, seed: u64) -> SoundRecipe {
    let mut g = Rng::new(seed);
    // sfxr's starting point for every category.
    let mut r = SoundRecipe {
        sustain: 0.3,
        decay: 0.4,
        base_freq: 0.3,
        ..SoundRecipe::default()
    };
    match category {
        SoundCategory::Coin => {
            r.base_freq = 0.4 + g.frnd(0.5);
            r.sustain = g.frnd(0.1);
            r.decay = 0.1 + g.frnd(0.4);
            r.punch = 0.3 + g.frnd(0.3);
            if g.coin() {
                r.arp_speed = 0.5 + g.frnd(0.2);
                r.arp_mod = 0.2 + g.frnd(0.4);
            }
        }
        SoundCategory::Laser => {
            r.wave = wave_from_sfxr(g.rnd(2));
            if r.wave == Wave::Sine && g.coin() {
                r.wave = wave_from_sfxr(g.rnd(1));
            }
            r.base_freq = 0.5 + g.frnd(0.5);
            r.freq_limit = (r.base_freq - 0.2 - g.frnd(0.6)).max(0.2);
            r.freq_ramp = -0.15 - g.frnd(0.2);
            if g.rnd(2) == 0 {
                r.base_freq = 0.3 + g.frnd(0.6);
                r.freq_limit = g.frnd(0.1);
                r.freq_ramp = -0.35 - g.frnd(0.3);
            }
            if g.coin() {
                r.duty = g.frnd(0.5);
                r.duty_ramp = g.frnd(0.2);
            } else {
                r.duty = 0.4 + g.frnd(0.5);
                r.duty_ramp = -g.frnd(0.7);
            }
            r.sustain = 0.1 + g.frnd(0.2);
            r.decay = g.frnd(0.4);
            if g.coin() {
                r.punch = g.frnd(0.3);
            }
            if g.rnd(2) == 0 {
                r.phaser_offset = g.frnd(0.2);
                r.phaser_ramp = -g.frnd(0.2);
            }
            if g.coin() {
                r.hpf_freq = g.frnd(0.3);
            }
        }
        SoundCategory::Explosion => {
            r.wave = Wave::Noise;
            if g.coin() {
                r.base_freq = 0.1 + g.frnd(0.4);
                r.freq_ramp = -0.1 + g.frnd(0.4);
            } else {
                r.base_freq = 0.2 + g.frnd(0.7);
                r.freq_ramp = -0.2 - g.frnd(0.2);
            }
            r.base_freq *= r.base_freq;
            if g.rnd(4) == 0 {
                r.freq_ramp = 0.0;
            }
            if g.rnd(2) == 0 {
                r.repeat_speed = 0.3 + g.frnd(0.5);
            }
            r.sustain = 0.1 + g.frnd(0.3);
            r.decay = g.frnd(0.5);
            if g.rnd(1) == 0 {
                r.phaser_offset = -0.3 + g.frnd(0.9);
                r.phaser_ramp = -g.frnd(0.3);
            }
            r.punch = 0.2 + g.frnd(0.6);
            if g.coin() {
                r.vib_strength = g.frnd(0.7);
                r.vib_speed = g.frnd(0.6);
            }
            if g.rnd(2) == 0 {
                r.arp_speed = 0.6 + g.frnd(0.3);
                r.arp_mod = 0.8 - g.frnd(1.6);
            }
        }
        SoundCategory::PowerUp => {
            if g.coin() {
                r.wave = Wave::Saw;
            } else {
                r.duty = g.frnd(0.6);
            }
            r.base_freq = 0.2 + g.frnd(0.3);
            if g.coin() {
                r.freq_ramp = 0.1 + g.frnd(0.4);
                r.repeat_speed = 0.4 + g.frnd(0.4);
            } else {
                r.freq_ramp = 0.05 + g.frnd(0.2);
                if g.coin() {
                    r.vib_strength = g.frnd(0.7);
                    r.vib_speed = g.frnd(0.6);
                }
            }
            r.sustain = g.frnd(0.4);
            r.decay = 0.1 + g.frnd(0.4);
        }
        SoundCategory::Hit => {
            r.wave = match g.rnd(2) {
                0 => Wave::Square,
                1 => Wave::Saw,
                _ => Wave::Noise,
            };
            if r.wave == Wave::Square {
                r.duty = g.frnd(0.6);
            }
            r.base_freq = 0.2 + g.frnd(0.6);
            r.freq_ramp = -0.3 - g.frnd(0.4);
            r.sustain = g.frnd(0.1);
            r.decay = 0.1 + g.frnd(0.2);
            if g.coin() {
                r.hpf_freq = g.frnd(0.3);
            }
        }
        SoundCategory::Jump => {
            r.duty = g.frnd(0.6);
            r.base_freq = 0.3 + g.frnd(0.3);
            r.freq_ramp = 0.1 + g.frnd(0.2);
            r.sustain = 0.1 + g.frnd(0.3);
            r.decay = 0.1 + g.frnd(0.2);
            if g.coin() {
                r.hpf_freq = g.frnd(0.3);
            }
            if g.coin() {
                r.lpf_freq = 1.0 - g.frnd(0.6);
            }
        }
        SoundCategory::Blip => {
            r.wave = wave_from_sfxr(g.rnd(1));
            if r.wave == Wave::Square {
                r.duty = g.frnd(0.6);
            }
            r.base_freq = 0.2 + g.frnd(0.4);
            r.sustain = 0.1 + g.frnd(0.1);
            r.decay = g.frnd(0.2);
            r.hpf_freq = 0.1;
        }
        SoundCategory::Random => r = randomize(&mut g),
    }
    r.clamp_all()
}

/// sfxr's "Randomize": anything at all, nudged away from silent or
/// ear-splitting extremes.
fn randomize(g: &mut Rng) -> SoundRecipe {
    fn sym(g: &mut Rng, p: i32) -> f32 {
        (g.frnd(2.0) - 1.0).powi(p)
    }
    let mut r = SoundRecipe {
        wave: wave_from_sfxr(g.rnd(3)),
        ..SoundRecipe::default()
    };
    r.base_freq = sym(g, 2);
    if g.coin() {
        r.base_freq = sym(g, 3) + 0.5;
    }
    r.freq_ramp = sym(g, 5);
    if r.base_freq > 0.7 && r.freq_ramp > 0.2 {
        r.freq_ramp = -r.freq_ramp;
    }
    if r.base_freq < 0.2 && r.freq_ramp < -0.05 {
        r.freq_ramp = -r.freq_ramp;
    }
    r.freq_dramp = sym(g, 3);
    r.duty = g.frnd(2.0) - 1.0;
    r.duty_ramp = sym(g, 3);
    r.vib_strength = sym(g, 3);
    r.vib_speed = g.frnd(2.0) - 1.0;
    r.attack = sym(g, 3);
    r.sustain = sym(g, 2);
    r.decay = g.frnd(2.0) - 1.0;
    r.punch = g.frnd(0.8).powi(2);
    if r.attack + r.sustain + r.decay < 0.2 {
        r.sustain += 0.2 + g.frnd(0.3);
        r.decay += 0.2 + g.frnd(0.3);
    }
    r.lpf_resonance = g.frnd(2.0) - 1.0;
    r.lpf_freq = 1.0 - g.frnd(1.0).powi(3);
    r.lpf_ramp = sym(g, 3);
    if r.lpf_freq < 0.1 && r.lpf_ramp < -0.05 {
        r.lpf_ramp = -r.lpf_ramp;
    }
    r.hpf_freq = g.frnd(1.0).powi(5);
    r.hpf_ramp = sym(g, 5);
    r.phaser_offset = sym(g, 3);
    r.phaser_ramp = sym(g, 3);
    r.repeat_speed = g.frnd(2.0) - 1.0;
    r.arp_speed = g.frnd(2.0) - 1.0;
    r.arp_mod = g.frnd(2.0) - 1.0;
    r
}

/// A new take on `recipe`: each shaping setting moves a little (at most
/// 0.05) or stays. The wave, crush, and volume are kept.
pub fn vary(recipe: &SoundRecipe, seed: u64) -> SoundRecipe {
    let mut g = Rng::new(seed);
    let mut r = *recipe;
    for v in [
        &mut r.base_freq,
        &mut r.freq_ramp,
        &mut r.freq_dramp,
        &mut r.duty,
        &mut r.duty_ramp,
        &mut r.vib_strength,
        &mut r.vib_speed,
        &mut r.attack,
        &mut r.sustain,
        &mut r.decay,
        &mut r.punch,
        &mut r.lpf_resonance,
        &mut r.lpf_freq,
        &mut r.lpf_ramp,
        &mut r.hpf_freq,
        &mut r.hpf_ramp,
        &mut r.phaser_offset,
        &mut r.phaser_ramp,
        &mut r.repeat_speed,
        &mut r.arp_speed,
        &mut r.arp_mod,
    ] {
        if g.coin() {
            *v += g.frnd(0.1) - 0.05;
        }
    }
    r.clamp_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_is_deterministic_per_seed() {
        for c in SoundCategory::ALL {
            assert_eq!(generate(c, 5), generate(c, 5), "{c:?}");
            assert_ne!(generate(c, 5), generate(c, 6), "{c:?}");
        }
    }

    #[test]
    fn every_generated_recipe_is_in_range() {
        for c in SoundCategory::ALL {
            for seed in 0..100 {
                let r = generate(c, seed);
                assert_eq!(r.clamped().expect("finite"), r, "{c:?} seed {seed}");
            }
        }
    }

    #[test]
    fn vary_stays_close() {
        let original = generate(SoundCategory::Laser, 3);
        let varied = vary(&original, 9);
        assert_ne!(varied, original);
        let (mut a, mut b) = (original, varied);
        let mut before = Vec::new();
        for_each_field!(&mut a, |_: &str, v: &mut f32, _: f32, _: f32| before
            .push(*v));
        let mut after = Vec::new();
        for_each_field!(&mut b, |_: &str, v: &mut f32, _: f32, _: f32| after
            .push(*v));
        for (x, y) in before.iter().zip(&after) {
            assert!((x - y).abs() <= 0.05 + 1e-6, "{x} -> {y}");
        }
        assert_eq!(vary(&original, 9), varied);
    }

    #[test]
    fn clamped_limits_and_rejects_nan() {
        let r = SoundRecipe {
            base_freq: 3.0,
            freq_ramp: -4.0,
            volume_db: 40.0,
            ..SoundRecipe::default()
        };
        let c = r.clamped().expect("finite");
        assert_eq!((c.base_freq, c.freq_ramp, c.volume_db), (1.0, -1.0, 6.0));
        let bad = SoundRecipe {
            attack: f32::NAN,
            ..SoundRecipe::default()
        };
        assert!(matches!(bad.clamped(), Err(e) if e.to_string().contains("attack")));
    }

    #[test]
    fn a_partial_recipe_fills_in_the_defaults() {
        let r: SoundRecipe =
            serde_json::from_str(r#"{"wave":"noise","decay":0.5}"#).expect("parse");
        assert_eq!(r.wave, Wave::Noise);
        assert_eq!(r.decay, 0.5);
        assert_eq!(r.base_freq, SoundRecipe::default().base_freq);
    }
}
