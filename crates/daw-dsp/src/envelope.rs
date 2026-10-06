/// Times in seconds and sustain level (0.0–1.0) for an [`Adsr`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdsrParams {
    pub attack_s: f32,
    pub decay_s: f32,
    pub sustain: f32,
    pub release_s: f32,
}

impl Default for AdsrParams {
    fn default() -> Self {
        Self {
            attack_s: 0.005,
            decay_s: 0.2,
            sustain: 0.7,
            release_s: 0.3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Shortest attack and release, so notes never start or stop with a click.
const MIN_ATTACK_S: f32 = 0.001;
const MIN_RELEASE_S: f32 = 0.005;
/// Level treated as silence (-100 dB).
const SILENCE: f32 = 1e-5;

/// Attack-decay-sustain-release envelope.
///
/// Linear attack; exponential decay and release, which sound natural.
/// Retriggering starts from the current level, so repeated notes don't click.
#[derive(Debug, Clone)]
pub struct Adsr {
    stage: Stage,
    level: f32,
    attack_step: f32,
    decay_coef: f32,
    release_coef: f32,
    sustain: f32,
}

impl Adsr {
    pub fn new(params: AdsrParams, sample_rate_hz: f32) -> Self {
        let mut env = Self {
            stage: Stage::Idle,
            level: 0.0,
            attack_step: 1.0,
            decay_coef: 0.0,
            release_coef: 0.0,
            sustain: 0.0,
        };
        env.set_params(params, sample_rate_hz);
        env
    }

    // RT-SAFE
    pub fn set_params(&mut self, p: AdsrParams, sample_rate_hz: f32) {
        self.attack_step = 1.0 / (p.attack_s.max(MIN_ATTACK_S) * sample_rate_hz);
        self.decay_coef = exp_coef(p.decay_s.max(MIN_RELEASE_S), sample_rate_hz);
        self.release_coef = exp_coef(p.release_s.max(MIN_RELEASE_S), sample_rate_hz);
        self.sustain = p.sustain.clamp(0.0, 1.0);
    }

    // RT-SAFE
    pub fn note_on(&mut self) {
        self.stage = Stage::Attack;
    }

    // RT-SAFE
    pub fn note_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    /// Hard reset to silence. Only for voices that are not sounding.
    pub fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
    }

    pub fn is_active(&self) -> bool {
        self.stage != Stage::Idle
    }

    pub fn is_released(&self) -> bool {
        matches!(self.stage, Stage::Release | Stage::Idle)
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    // RT-SAFE
    pub fn next_sample(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += self.attack_step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level = self.sustain + (self.level - self.sustain) * self.decay_coef;
                if (self.level - self.sustain).abs() < SILENCE {
                    self.level = self.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => self.level = self.sustain,
            Stage::Release => {
                self.level *= self.release_coef;
                if self.level < SILENCE {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

/// Per-sample multiplier that decays to -60 dB in `time_s`.
fn exp_coef(time_s: f32, sample_rate_hz: f32) -> f32 {
    (-(1000f32.ln()) / (time_s * sample_rate_hz)).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn params() -> AdsrParams {
        AdsrParams {
            attack_s: 0.01,
            decay_s: 0.1,
            sustain: 0.5,
            release_s: 0.1,
        }
    }

    #[test]
    fn attack_reaches_full_level_on_time() {
        let mut env = Adsr::new(params(), SR);
        env.note_on();
        let samples_to_peak = (0..10_000).position(|_| env.next_sample() >= 1.0);
        assert_eq!(samples_to_peak, Some(479));
    }

    #[test]
    fn settles_at_sustain_then_releases_to_silence() {
        let mut env = Adsr::new(params(), SR);
        env.note_on();
        for _ in 0..(SR as usize) {
            env.next_sample();
        }
        assert!((env.level() - 0.5).abs() < 1e-3);
        env.note_off();
        // -60 dB after release time, fully idle not long after.
        for _ in 0..(SR as usize / 10) {
            env.next_sample();
        }
        assert!(env.level() < 0.5 * 0.0011, "{}", env.level());
        for _ in 0..(SR as usize) {
            env.next_sample();
        }
        assert!(!env.is_active());
    }

    #[test]
    fn retrigger_continues_from_current_level() {
        let mut env = Adsr::new(params(), SR);
        env.note_on();
        for _ in 0..(SR as usize / 2) {
            env.next_sample();
        }
        env.note_off();
        for _ in 0..100 {
            env.next_sample();
        }
        let before = env.level();
        env.note_on();
        let after = env.next_sample();
        assert!((after - before).abs() < 0.01, "jump {before} -> {after}");
    }
}
