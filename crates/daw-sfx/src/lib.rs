//! Renders game sound effect recipes ([`SoundRecipe`]) to audio.
//!
//! Adapted from sfxr's synthesizer (`sfxr.cpp`, `SynthSample`, by Tomas
//! Pettersson; MIT licence; see THIRD_PARTY.md): the same envelope, pitch
//! slide, vibrato, pitch jump, square-width sweep, repeat, filters, and
//! phaser, so recipes sound the way sfxr users expect. Added here: a
//! triangle wave, bit crush, a volume in dB, and other sample rates.
//! Rendering is offline and deterministic (noise uses a fixed seed).

use daw_model::sound::{MAX_SOUND_SECONDS, SoundRecipe, Wave};

/// sfxr's own rate; recipes are timed in its samples.
const BASE_RATE_HZ: u32 = 44_100;
/// Oversampling per output sample (sfxr's).
const OVERSAMPLE: usize = 8;
const PHASER_LEN: usize = 1024;
const NOISE_LEN: usize = 32;
/// Output level: sfxr's raw output often peaks near 2 (punch, filters,
/// phaser), so this leaves most sounds around -6 dBFS with room to spare.
const BASE_GAIN: f32 = 0.25;

/// The loudest sample, as a positive number (0 for silence).
pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// Renders `recipe` as mono samples at `sample_rate_hz`, at most
/// [`MAX_SOUND_SECONDS`] long. Not clamped: a loud recipe can peak above 1.
pub fn render(recipe: &SoundRecipe, sample_rate_hz: u32) -> Vec<f32> {
    let base = Synth::new(recipe).run();
    if sample_rate_hz == BASE_RATE_HZ || sample_rate_hz == 0 || base.is_empty() {
        return base;
    }
    resample(&base, sample_rate_hz)
}

/// Linear-interpolation resampling: plenty for short effects.
fn resample(input: &[f32], to_hz: u32) -> Vec<f32> {
    let ratio = f64::from(BASE_RATE_HZ) / f64::from(to_hz);
    let n = ((input.len() as f64) / ratio).round() as usize;
    (0..n)
        .map(|i| {
            let pos = i as f64 * ratio;
            let j = pos.floor() as usize;
            let frac = (pos - j as f64) as f32;
            let a = input.get(j).copied().unwrap_or(0.0);
            let b = input.get(j + 1).copied().unwrap_or(a);
            a + (b - a) * frac
        })
        .collect()
}

/// xorshift32, for the noise wave.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// sfxr's synth state.
struct Synth<'a> {
    r: &'a SoundRecipe,
    phase: usize,
    fperiod: f64,
    fmaxperiod: f64,
    fslide: f64,
    fdslide: f64,
    square_duty: f32,
    square_slide: f32,
    arp_mod: f64,
    arp_time: usize,
    arp_limit: usize,
    // Filters.
    fltp: f32,
    fltdp: f32,
    fltw: f32,
    fltw_d: f32,
    fltdmp: f32,
    fltphp: f32,
    flthp: f32,
    flthp_d: f32,
    // Vibrato.
    vib_phase: f64,
    vib_speed: f64,
    vib_amp: f64,
    // Envelope.
    env_stage: usize,
    env_time: usize,
    env_length: [usize; 3],
    // Phaser.
    fphase: f32,
    fdphase: f32,
    ipp: usize,
    phaser: Box<[f32; PHASER_LEN]>,
    noise_buffer: [f32; NOISE_LEN],
    noise: Noise,
    rep_time: usize,
    rep_limit: usize,
}

impl<'a> Synth<'a> {
    fn new(r: &'a SoundRecipe) -> Self {
        let mut noise = Noise(0x1234_5678);
        let mut noise_buffer = [0.0; NOISE_LEN];
        for n in &mut noise_buffer {
            *n = noise.next();
        }
        let env_len = |x: f32| (x * x * 100_000.0) as usize;
        let mut s = Synth {
            r,
            phase: 0,
            fperiod: 0.0,
            fmaxperiod: 0.0,
            fslide: 0.0,
            fdslide: 0.0,
            square_duty: 0.0,
            square_slide: 0.0,
            arp_mod: 0.0,
            arp_time: 0,
            arp_limit: 0,
            fltp: 0.0,
            fltdp: 0.0,
            fltw: r.lpf_freq.powi(3) * 0.1,
            fltw_d: 1.0 + r.lpf_ramp * 0.0001,
            fltdmp: (5.0 / (1.0 + r.lpf_resonance.powi(2) * 20.0)
                * (0.01 + r.lpf_freq.powi(3) * 0.1))
                .min(0.8),
            fltphp: 0.0,
            flthp: r.hpf_freq.powi(2) * 0.1,
            flthp_d: 1.0 + r.hpf_ramp * 0.0003,
            vib_phase: 0.0,
            vib_speed: f64::from(r.vib_speed).powi(2) * 0.01,
            vib_amp: f64::from(r.vib_strength) * 0.5,
            env_stage: 0,
            env_time: 0,
            env_length: [env_len(r.attack), env_len(r.sustain), env_len(r.decay)],
            fphase: r.phaser_offset.powi(2) * 1020.0 * r.phaser_offset.signum(),
            fdphase: r.phaser_ramp.powi(2) * r.phaser_ramp.signum(),
            ipp: 0,
            phaser: Box::new([0.0; PHASER_LEN]),
            noise_buffer,
            noise,
            rep_time: 0,
            rep_limit: if r.repeat_speed == 0.0 {
                0
            } else {
                ((1.0 - r.repeat_speed).powi(2) * 20_000.0 + 32.0) as usize
            },
        };
        s.reset_pitch();
        s
    }

    /// The part of sfxr's `ResetSample` that "repeat" restarts.
    fn reset_pitch(&mut self) {
        let r = self.r;
        self.fperiod = 100.0 / (f64::from(r.base_freq).powi(2) + 0.001);
        self.fmaxperiod = 100.0 / (f64::from(r.freq_limit).powi(2) + 0.001);
        self.fslide = 1.0 - f64::from(r.freq_ramp).powi(3) * 0.01;
        self.fdslide = -f64::from(r.freq_dramp).powi(3) * 0.000_001;
        self.square_duty = 0.5 - r.duty * 0.5;
        self.square_slide = -r.duty_ramp * 0.000_05;
        let m = f64::from(r.arp_mod);
        self.arp_mod = if m >= 0.0 {
            1.0 - m * m * 0.9
        } else {
            1.0 + m * m * 10.0
        };
        self.arp_time = 0;
        self.arp_limit = if r.arp_speed >= 1.0 {
            0
        } else {
            ((1.0 - r.arp_speed).powi(2) * 20_000.0 + 32.0) as usize
        };
    }

    fn run(mut self) -> Vec<f32> {
        let max_samples = (MAX_SOUND_SECONDS * BASE_RATE_HZ as f32) as usize;
        let total: usize = self.env_length.iter().sum::<usize>() + 4;
        let mut out = Vec::with_capacity(total.min(max_samples));
        let gain = BASE_GAIN * 10f32.powf(self.r.volume_db / 20.0);
        let mut crush = Crusher::new(self.r.crush);
        while out.len() < max_samples {
            match self.sample() {
                Some(s) => out.push(crush.process(s * gain)),
                None => break,
            }
        }
        out
    }

    /// One output sample, or None when the sound has finished.
    fn sample(&mut self) -> Option<f32> {
        let r = self.r;
        self.rep_time += 1;
        if self.rep_limit != 0 && self.rep_time >= self.rep_limit {
            self.rep_time = 0;
            self.reset_pitch();
        }
        self.arp_time += 1;
        if self.arp_limit != 0 && self.arp_time >= self.arp_limit {
            self.arp_limit = 0;
            self.fperiod *= self.arp_mod;
        }
        self.fslide += self.fdslide;
        self.fperiod *= self.fslide;
        if self.fperiod > self.fmaxperiod {
            self.fperiod = self.fmaxperiod;
            if r.freq_limit > 0.0 {
                return None;
            }
        }
        let mut rfperiod = self.fperiod;
        if self.vib_amp > 0.0 {
            self.vib_phase += self.vib_speed;
            rfperiod = self.fperiod * (1.0 + self.vib_phase.sin() * self.vib_amp);
        }
        // Clamped both ways: a slide can't make the period absurd.
        let period = (rfperiod as usize).clamp(8, 10_000_000);
        self.square_duty = (self.square_duty + self.square_slide).clamp(0.0, 0.5);

        self.env_time += 1;
        if self.env_time > self.env_length[self.env_stage] {
            self.env_time = 0;
            self.env_stage += 1;
            if self.env_stage == 3 {
                return None;
            }
        }
        let t = self.env_time as f32 / self.env_length[self.env_stage].max(1) as f32;
        let env_vol = match self.env_stage {
            0 => t,
            1 => 1.0 + (1.0 - t) * 2.0 * r.punch,
            _ => 1.0 - t,
        };

        self.fphase += self.fdphase;
        let iphase = (self.fphase.abs() as usize).min(PHASER_LEN - 1);
        if self.flthp_d != 0.0 {
            self.flthp = (self.flthp * self.flthp_d).clamp(0.000_01, 0.1);
        }

        let mut total = 0.0f32;
        for _ in 0..OVERSAMPLE {
            self.phase += 1;
            if self.phase >= period {
                self.phase %= period;
                if r.wave == Wave::Noise {
                    for n in &mut self.noise_buffer {
                        *n = self.noise.next();
                    }
                }
            }
            let fp = self.phase as f32 / period as f32;
            let mut sample = match r.wave {
                Wave::Square => {
                    if fp < self.square_duty {
                        0.5
                    } else {
                        -0.5
                    }
                }
                Wave::Saw => 1.0 - fp * 2.0,
                Wave::Sine => (fp * std::f32::consts::TAU).sin(),
                Wave::Triangle => 4.0 * (fp - 0.5).abs() - 1.0,
                Wave::Noise => {
                    self.noise_buffer[(self.phase * NOISE_LEN / period).min(NOISE_LEN - 1)]
                }
            };
            // Low-pass.
            let pp = self.fltp;
            self.fltw = (self.fltw * self.fltw_d).clamp(0.0, 0.1);
            if r.lpf_freq < 1.0 {
                self.fltdp += (sample - self.fltp) * self.fltw;
                self.fltdp -= self.fltdp * self.fltdmp;
            } else {
                self.fltp = sample;
                self.fltdp = 0.0;
            }
            self.fltp += self.fltdp;
            // High-pass.
            self.fltphp += self.fltp - pp;
            self.fltphp -= self.fltphp * self.flthp;
            sample = self.fltphp;
            // Phaser.
            self.phaser[self.ipp & (PHASER_LEN - 1)] = sample;
            sample += self.phaser[(self.ipp + PHASER_LEN - iphase) & (PHASER_LEN - 1)];
            self.ipp = (self.ipp + 1) & (PHASER_LEN - 1);
            total += sample * env_vol;
        }
        Some(total / OVERSAMPLE as f32)
    }
}

/// Bit crush: holds each sample for a few samples and rounds it to fewer
/// bits. Off at 0.
struct Crusher {
    hold: usize,
    steps: f32,
    count: usize,
    held: f32,
}

impl Crusher {
    fn new(crush: f32) -> Self {
        Crusher {
            hold: 1 + (crush * 15.0).round() as usize,
            // 16 bits at 0, 4 bits at 1.
            steps: 2f32.powf(15.0 - crush * 12.0),
            count: 0,
            held: 0.0,
        }
    }

    fn process(&mut self, s: f32) -> f32 {
        if self.hold == 1 && self.steps >= 32_768.0 {
            return s;
        }
        if self.count == 0 {
            self.held = (s * self.steps).round() / self.steps;
        }
        self.count = (self.count + 1) % self.hold;
        self.held
    }
}
