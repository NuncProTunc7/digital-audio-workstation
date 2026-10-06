/// Click sound: a short sine "tick" with a fast exponential decay.
/// Downbeats are higher and louder so the bar is easy to hear.
#[derive(Debug, Clone)]
pub(crate) struct Metronome {
    sample_rate_hz: f32,
    phase: f32,
    freq_hz: f32,
    amp: f32,
    decay_coef: f32,
}

const DOWNBEAT_HZ: f32 = 1_760.0;
const BEAT_HZ: f32 = 1_320.0;
const DOWNBEAT_GAIN: f32 = 0.5;
const BEAT_GAIN: f32 = 0.3;
const DECAY_S: f32 = 0.03;

impl Metronome {
    pub fn new(sample_rate_hz: f32) -> Self {
        Self {
            sample_rate_hz,
            phase: 0.0,
            freq_hz: BEAT_HZ,
            amp: 0.0,
            decay_coef: (-(1000f32.ln()) / (DECAY_S * sample_rate_hz)).exp(),
        }
    }

    // RT-SAFE
    pub fn trigger(&mut self, downbeat: bool) {
        self.phase = 0.0;
        (self.freq_hz, self.amp) = if downbeat {
            (DOWNBEAT_HZ, DOWNBEAT_GAIN)
        } else {
            (BEAT_HZ, BEAT_GAIN)
        };
    }

    // RT-SAFE
    pub fn next(&mut self) -> f32 {
        if self.amp < 1e-5 {
            return 0.0;
        }
        let s = (self.phase * std::f32::consts::TAU).sin() * self.amp;
        self.phase = (self.phase + self.freq_hz / self.sample_rate_hz).fract();
        self.amp *= self.decay_coef;
        s
    }
}
