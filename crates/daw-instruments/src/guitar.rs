//! Six-string guitar: one plucked-string model (Karplus-Strong: a noise
//! burst circulating through a delay line with a damping filter) per
//! string.
//!
//! Each note goes to the highest string that can reach it in the chosen
//! tuning, so a chord shape lands one note per string and a string that is
//! plucked again cuts its old note, as on a real guitar. That is what makes
//! the tuning audible: a drop or open tuning changes which strings ring
//! together and how low the guitar goes.

use daw_dsp::{Biquad, BiquadShape, Rng, db_to_gain, midi_to_hz};
use daw_model::instrument::{guitar_string_for, strings_for};

use crate::InstrumentProcessor;

const STRINGS: usize = 6;
/// Lowest pitch the delay lines can hold; lower notes play at this.
const LOWEST_HZ: f32 = 20.0;
/// How long a string rings after its key is let go (a lifted finger).
const RELEASE_T60_S: f32 = 0.12;
/// Notes this close together count as one strummed chord.
const STRUM_WINDOW_S: f32 = 0.03;
/// Below this a string is treated as silent and skipped.
const SILENT: f32 = 1e-5;
/// Keeps six strings ringing together below full scale.
const STRING_GAIN: f32 = 0.5;
/// Strings spread a little across the stereo field, low on the left.
const PANS: [f32; STRINGS] = [-0.25, -0.15, -0.05, 0.05, 0.15, 0.25];
const PITCH_BEND_RANGE_SEMITONES: f32 = 12.0;

/// Parameter indices, matching `GUITAR_PARAMS`.
mod param {
    pub const TUNING: usize = 0;
    pub const CAPO: usize = 1;
    pub const TONE: usize = 2;
    pub const SUSTAIN: usize = 3;
    pub const PICK: usize = 4;
    pub const MUTE: usize = 5;
    pub const STRUM: usize = 6;
    pub const BODY: usize = 7;
    pub const VELOCITY: usize = 8;
    pub const GAIN: usize = 9;
}

#[derive(Debug, Clone, Copy)]
struct Params {
    tuning: f32,
    capo: f32,
    tone: f32,
    sustain_s: f32,
    pick_position: f32,
    mute: f32,
    strum_s: f32,
    body: f32,
    velocity_sensitivity: f32,
    gain: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            tuning: 0.0,
            capo: 0.0,
            tone: 0.5,
            sustain_s: 3.0,
            pick_position: 0.15,
            mute: 0.0,
            strum_s: 0.0,
            body: 0.5,
            velocity_sensitivity: 0.7,
            gain: db_to_gain(-6.0),
        }
    }
}

/// A pluck waiting for its turn in a strum.
#[derive(Debug, Clone, Copy)]
struct Pending {
    note: u8,
    velocity: f32,
    samples_left: u32,
    /// The key was let go before the pluck happened.
    released: bool,
}

#[derive(Debug, Clone)]
struct GuitarString {
    buf: Vec<f32>,
    write: usize,
    note: u8,
    /// The key is down (or the sustain pedal holds it).
    held: bool,
    held_by_pedal: bool,
    active: bool,
    /// Loop damping filter state and coefficient (0 = bright).
    lp: f32,
    lp_coef: f32,
    /// Loss per trip round the loop.
    loss: f32,
    pending: Option<Pending>,
    pan_l: f32,
    pan_r: f32,
}

impl GuitarString {
    fn new(len: usize, pan: f32) -> Self {
        let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
        Self {
            buf: vec![0.0; len],
            write: 0,
            note: 0,
            held: false,
            held_by_pedal: false,
            active: false,
            lp: 0.0,
            lp_coef: 0.0,
            loss: 0.0,
            pending: None,
            pan_l: angle.cos(),
            pan_r: angle.sin(),
        }
    }
}

/// The built-in guitar.
pub struct Guitar {
    sample_rate_hz: f32,
    params: Params,
    strings: [GuitarString; STRINGS],
    rng: Rng,
    sustain_pedal: bool,
    pitch_bend_semitones: f32,
    /// Samples since the current strum started, and its lowest string.
    since_strum: u32,
    strum_base: usize,
    body: [[Biquad; 2]; 2],
    dc: [(f32, f32); 2],
}

impl Guitar {
    pub fn new(sample_rate_hz: f32) -> Self {
        let len = (sample_rate_hz / LOWEST_HZ) as usize + 4;
        let mut g = Self {
            sample_rate_hz,
            params: Params::default(),
            strings: std::array::from_fn(|i| GuitarString::new(len, PANS[i])),
            rng: Rng::new(0x6u32.wrapping_mul(0x9E37_79B9)),
            sustain_pedal: false,
            pitch_bend_semitones: 0.0,
            since_strum: u32::MAX,
            strum_base: 0,
            body: Default::default(),
            dc: [(0.0, 0.0); 2],
        };
        g.update_body();
        g
    }

    /// Strings currently making sound. For tests.
    pub fn active_strings(&self) -> usize {
        self.strings.iter().filter(|s| s.active).count()
    }

    /// The open strings for the current tuning and capo.
    // RT-SAFE
    fn open_strings(&self) -> [u8; STRINGS] {
        strings_for(f64::from(self.params.tuning), f64::from(self.params.capo))
    }

    /// The low string without the capo, to tell how far up the capo sits.
    // RT-SAFE
    fn uncapoed_low(&self) -> u8 {
        strings_for(f64::from(self.params.tuning), 0.0)[0]
    }

    /// Which string plays `note` (the rule is shared with the tab view).
    // RT-SAFE
    fn pick_string(&self, note: u8) -> usize {
        let busy = self
            .strings
            .each_ref()
            .map(|s| (s.held || s.pending.is_some()).then(|| Self::sounding(s)));
        guitar_string_for(note, self.open_strings(), self.uncapoed_low(), busy)
    }

    // RT-SAFE
    fn sounding(s: &GuitarString) -> u8 {
        s.pending.map_or(s.note, |p| p.note)
    }

    /// Loop delay in samples for `note`, allowing for the damping filter's
    /// own delay.
    // RT-SAFE
    fn delay_samples(&self, note: u8, lp_coef: f32, len: usize) -> f32 {
        let hz = midi_to_hz(f32::from(note) + self.pitch_bend_semitones).max(LOWEST_HZ);
        let filter_delay = lp_coef / (1.0 - lp_coef);
        (self.sample_rate_hz / hz - filter_delay).clamp(2.0, (len - 3) as f32)
    }

    /// Per-trip loss so the note dies away by 60 dB in `t60_s`.
    // RT-SAFE
    fn loss_for(note: u8, t60_s: f32) -> f32 {
        let hz = midi_to_hz(f32::from(note)).max(LOWEST_HZ);
        10f32.powf(-3.0 / (hz * t60_s.max(0.01)))
    }

    /// How long a held note rings: higher notes die sooner, palm muting
    /// cuts it short.
    // RT-SAFE
    fn held_t60(&self, note: u8) -> f32 {
        let hz = midi_to_hz(f32::from(note)).max(LOWEST_HZ);
        let by_pitch = (82.4 / hz).powf(0.3);
        self.params.sustain_s * by_pitch * (1.0 - 0.92 * self.params.mute)
    }

    /// Starts string `i` sounding `note`: fills its loop with a noise burst
    /// shaped by the tone and pick position.
    // RT-SAFE
    fn pluck(&mut self, i: usize, note: u8, velocity: f32) {
        let p = self.params;
        let lp_coef = (0.55 - 0.5 * p.tone + 0.3 * p.mute).clamp(0.02, 0.85);
        let t60 = self.held_t60(note);
        let len = self.strings[i].buf.len();
        let delay = self.delay_samples(note, lp_coef, len);
        let amp = (1.0 - p.velocity_sensitivity) + p.velocity_sensitivity * velocity;
        let period = (delay.ceil() as usize + 1).min(len);
        let bright = (0.15 + 0.85 * p.tone) * (0.4 + 0.6 * velocity) * (1.0 - 0.6 * p.mute);
        let pick = ((p.pick_position * period as f32) as usize).clamp(1, period - 1);
        let s = &mut self.strings[i];
        let start = (s.write + len - period) % len;
        let mut smooth = 0.0;
        for k in 0..period {
            smooth += (self.rng.next_bipolar() - smooth) * bright;
            s.buf[(start + k) % len] = smooth * amp;
        }
        // Plucking a point along the string cancels the harmonics that
        // have a node there.
        for k in (pick..period).rev() {
            let earlier = s.buf[(start + k - pick) % len];
            s.buf[(start + k) % len] -= earlier;
        }
        s.note = note;
        s.held = true;
        s.held_by_pedal = false;
        s.active = true;
        s.lp = 0.0;
        s.lp_coef = lp_coef;
        s.loss = Self::loss_for(note, t60);
    }

    /// The string's key is let go: it is damped quickly.
    // RT-SAFE
    fn damp(&mut self, i: usize) {
        let s = &mut self.strings[i];
        s.held = false;
        s.held_by_pedal = false;
        s.loss = Self::loss_for(s.note, RELEASE_T60_S);
    }

    // RT-SAFE
    fn update_body(&mut self) {
        let gain = self.params.body * 9.0;
        for ch in &mut self.body {
            ch[0].set(
                BiquadShape::Peak { gain_db: gain },
                105.0,
                1.4,
                self.sample_rate_hz,
            );
            ch[1].set(
                BiquadShape::Peak {
                    gain_db: gain * 0.6,
                },
                230.0,
                2.0,
                self.sample_rate_hz,
            );
        }
    }

    // RT-SAFE
    fn release_pedal_notes(&mut self) {
        for i in 0..STRINGS {
            if self.strings[i].held_by_pedal {
                self.damp(i);
            }
        }
    }
}

impl InstrumentProcessor for Guitar {
    // RT-SAFE
    fn note_on(&mut self, note: u8, velocity: f32) {
        if velocity <= 0.0 {
            self.note_off(note);
            return;
        }
        let i = self.pick_string(note);
        let window = (STRUM_WINDOW_S * self.sample_rate_hz) as u32;
        if self.since_strum > window {
            self.since_strum = 0;
            self.strum_base = i;
        }
        // Each string a little later than the one below it.
        let step = self.params.strum_s / (STRINGS - 1) as f32;
        let wait = i.saturating_sub(self.strum_base) as f32 * step * self.sample_rate_hz;
        if wait >= 1.0 {
            self.strings[i].pending = Some(Pending {
                note,
                velocity: velocity.clamp(0.0, 1.0),
                samples_left: wait as u32,
                released: false,
            });
        } else {
            self.strings[i].pending = None;
            self.pluck(i, note, velocity.clamp(0.0, 1.0));
        }
    }

    // RT-SAFE
    fn note_off(&mut self, note: u8) {
        let pedal = self.sustain_pedal;
        for i in 0..STRINGS {
            let s = &mut self.strings[i];
            if let Some(p) = s.pending.as_mut().filter(|p| p.note == note) {
                p.released = true;
            } else if s.held && s.note == note {
                if pedal {
                    s.held_by_pedal = true;
                } else {
                    self.damp(i);
                }
            }
        }
    }

    // RT-SAFE
    fn all_notes_off(&mut self) {
        self.sustain_pedal = false;
        for i in 0..STRINGS {
            self.strings[i].pending = None;
            if self.strings[i].active {
                self.damp(i);
            }
        }
    }

    // RT-SAFE
    fn set_param(&mut self, index: usize, value: f32) {
        let p = &mut self.params;
        match index {
            param::TUNING => p.tuning = value,
            param::CAPO => p.capo = value,
            param::TONE => p.tone = value.clamp(0.0, 1.0),
            param::SUSTAIN => p.sustain_s = value.clamp(0.05, 30.0),
            param::PICK => p.pick_position = value.clamp(0.02, 0.5),
            param::MUTE => p.mute = value.clamp(0.0, 1.0),
            param::STRUM => p.strum_s = value.clamp(0.0, 0.5),
            param::BODY => {
                p.body = value.clamp(0.0, 1.0);
                self.update_body();
            }
            param::VELOCITY => p.velocity_sensitivity = value.clamp(0.0, 1.0),
            param::GAIN => p.gain = db_to_gain(value),
            _ => {}
        }
    }

    // RT-SAFE
    fn control_change(&mut self, controller: u8, value: u8) {
        match controller {
            64 => {
                let down = value >= 64;
                if self.sustain_pedal && !down {
                    self.sustain_pedal = false;
                    self.release_pedal_notes();
                } else {
                    self.sustain_pedal = down;
                }
            }
            120 | 123 => self.all_notes_off(),
            _ => {}
        }
    }

    // RT-SAFE
    fn pitch_bend(&mut self, semitones: f32) {
        self.pitch_bend_semitones =
            semitones.clamp(-PITCH_BEND_RANGE_SEMITONES, PITCH_BEND_RANGE_SEMITONES);
    }

    // RT-SAFE
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let frames = left.len().min(right.len());
        self.since_strum = self.since_strum.saturating_add(frames as u32);
        let gain = self.params.gain * STRING_GAIN;
        for i in 0..STRINGS {
            let mut k = 0;
            while k < frames {
                // Run up to the next pluck in a strum, if there is one.
                let mut end = frames;
                if let Some(p) = self.strings[i].pending {
                    let at = k + p.samples_left as usize;
                    if at < frames {
                        end = at;
                    } else if let Some(p) = self.strings[i].pending.as_mut() {
                        p.samples_left -= (frames - k) as u32;
                    }
                }
                if self.strings[i].active {
                    let len = self.strings[i].buf.len();
                    let delay =
                        self.delay_samples(self.strings[i].note, self.strings[i].lp_coef, len);
                    let whole = delay.floor();
                    let frac = delay - whole;
                    let whole = whole as usize;
                    let s = &mut self.strings[i];
                    let mut peak = 0.0f32;
                    for n in k..end {
                        let a = s.buf[(s.write + len - whole) % len];
                        let b = s.buf[(s.write + len - whole - 1) % len];
                        let y = a + (b - a) * frac;
                        s.lp = s.lp_coef * s.lp + (1.0 - s.lp_coef) * y;
                        let out = s.lp * s.loss;
                        s.buf[s.write] = out;
                        s.write = (s.write + 1) % len;
                        peak = peak.max(out.abs());
                        if let (Some(l), Some(r)) = (left.get_mut(n), right.get_mut(n)) {
                            *l += out * s.pan_l * gain;
                            *r += out * s.pan_r * gain;
                        }
                    }
                    if peak < SILENT && end == frames && !s.held {
                        s.active = false;
                    }
                }
                if end < frames
                    && let Some(p) = self.strings[i].pending.take()
                {
                    self.pluck(i, p.note, p.velocity);
                    if p.released {
                        self.damp(i);
                    }
                }
                k = end;
            }
        }
        // Body resonance and a DC blocker on the summed output.
        let r = 1.0 - 20.0 * std::f32::consts::TAU / self.sample_rate_hz;
        for (ch, buf) in [&mut *left, &mut *right].into_iter().enumerate() {
            let [low, mid] = &mut self.body[ch];
            let (x1, y1) = &mut self.dc[ch];
            for x in buf.iter_mut().take(frames) {
                let v = mid.process(low.process(*x));
                let y = v - *x1 + r * *y1;
                *x1 = v;
                *y1 = y;
                *x = y;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{assert_clean, peak, render};
    use daw_model::{Instrument, InstrumentKind, instrument::GUITAR_PRESETS};

    const SR: f32 = 48_000.0;

    fn guitar_with(preset: &str, tuning: f64) -> Box<dyn InstrumentProcessor> {
        let mut inst = Instrument::from_preset(InstrumentKind::Guitar, preset).expect("preset");
        inst.params.insert("string.tuning".into(), tuning);
        crate::create(&inst, SR)
    }

    /// Fundamental by autocorrelation, searching 40-1000 Hz.
    fn pitch_hz(x: &[f32]) -> f32 {
        let (lo, hi) = ((SR / 1000.0) as usize, (SR / 40.0) as usize);
        let n = x.len() - hi;
        let corr = |lag: usize| -> f32 { (0..n).map(|i| x[i] * x[i + lag]).sum() };
        let best = (lo..hi)
            .max_by(|&a, &b| corr(a).total_cmp(&corr(b)))
            .expect("lags");
        SR / best as f32
    }

    #[test]
    fn every_preset_plays_cleanly_and_falls_silent() {
        for preset in GUITAR_PRESETS {
            let mut g = guitar_with(preset.name, 0.0);
            // An open E major chord.
            for note in [40, 47, 52, 56, 59, 64] {
                g.note_on(note, 1.0);
            }
            let (on, _) = render(g.as_mut(), SR, 1.0);
            assert_clean(&on, 1.0);
            assert!(peak(&on) > 0.01, "{} is silent", preset.name);
            for note in [40, 47, 52, 56, 59, 64] {
                g.note_off(note);
            }
            render(g.as_mut(), SR, 3.0);
            let (tail, _) = render(g.as_mut(), SR, 0.1);
            assert!(peak(&tail) < 1e-3, "{} still sounding", preset.name);
        }
    }

    #[test]
    fn plays_the_right_pitch() {
        for note in [40u8, 45, 57, 69] {
            let mut g = guitar_with("Acoustic Guitar", 0.0);
            g.note_on(note, 0.8);
            render(g.as_mut(), SR, 0.1);
            let (l, _) = render(g.as_mut(), SR, 0.2);
            let want = midi_to_hz(f32::from(note));
            let got = pitch_hz(&l);
            assert!(
                (got / want - 1.0).abs() < 0.02,
                "note {note}: {got} Hz, want {want} Hz"
            );
        }
    }

    #[test]
    fn a_chord_lands_one_note_per_string() {
        let mut g = Guitar::new(SR);
        for note in [40, 47, 52, 56, 59, 64] {
            g.note_on(note, 1.0);
        }
        assert_eq!(g.active_strings(), 6);
        let notes: Vec<u8> = g.strings.iter().map(|s| s.note).collect();
        assert_eq!(notes, [40, 47, 52, 56, 59, 64]);
    }

    #[test]
    fn the_tuning_decides_which_string_plays_a_note() {
        // A3: in standard tuning the B string (B3) is too high, so the G
        // string plays it; in DADGAD it is the open 2nd string.
        let mut standard = Guitar::new(SR);
        standard.note_on(57, 1.0);
        assert!(standard.strings[3].active && standard.strings[3].note == 57);
        let mut dadgad = Guitar::new(SR);
        dadgad.set_param(param::TUNING, 5.0);
        dadgad.note_on(57, 1.0);
        assert!(dadgad.strings[4].active && dadgad.strings[4].note == 57);
        // D4: the B string in standard, the open top string in Open G.
        standard.note_on(62, 1.0);
        assert!(standard.strings[4].active && standard.strings[4].note == 62);
        let mut open_g = Guitar::new(SR);
        open_g.set_param(param::TUNING, 6.0);
        open_g.note_on(62, 1.0);
        assert!(open_g.strings[5].active && open_g.strings[5].note == 62);
    }

    #[test]
    fn a_capo_raises_the_lowest_note() {
        let mut g = Guitar::new(SR);
        g.set_param(param::CAPO, 5.0);
        // A2 is now the open low string.
        g.note_on(45, 1.0);
        assert_eq!(g.strings[0].note, 45);
        // E2 is out of reach and goes to the nearest string, still played.
        g.note_on(40, 1.0);
        let (l, _) = render(&mut g, SR, 0.2);
        assert_clean(&l, 1.0);
    }

    #[test]
    fn strumming_spreads_a_chord_out() {
        // 50 ms from the low string to the top one: 10 ms apart.
        let mut g = Guitar::new(SR);
        g.set_param(param::STRUM, 0.05);
        for note in [40, 47, 52, 56, 59, 64] {
            g.note_on(note, 1.0);
        }
        render(&mut g, SR, 0.005);
        assert_eq!(g.active_strings(), 1, "only the low string after 5 ms");
        render(&mut g, SR, 0.1);
        assert_eq!(g.active_strings(), 6);
    }

    #[test]
    fn replaying_a_note_reuses_its_string() {
        let mut g = Guitar::new(SR);
        g.note_on(64, 1.0);
        g.note_on(64, 1.0);
        assert_eq!(g.active_strings(), 1);
    }

    #[test]
    fn output_is_deterministic() {
        let run = || {
            let mut g = guitar_with("Strummed Acoustic", 6.0);
            for note in [43, 50, 55, 59, 62] {
                g.note_on(note, 0.9);
            }
            render(g.as_mut(), SR, 0.5).0
        };
        assert_eq!(run(), run());
    }
}
