use daw_model::EffectKind;
use daw_model::effect::DELAY_SYNC_BEATS;

use crate::{DelayLine, EffectProcessor, one_pole_coef, param_id};

const MAX_DELAY_S: f32 = 4.0;

/// Stereo echo with tempo sync, filtered feedback, and ping-pong.
pub struct Delay {
    sr: f32,
    lines: [DelayLine; 2],
    sync_index: usize,
    time_s: f32,
    bpm: f32,
    feedback: f32,
    tone_coef: f32,
    ping_pong: bool,
    mix: f32,
    /// Smoothed delay in samples, so time changes glide instead of click.
    current_delay: f32,
    smooth_coef: f32,
    tone_state: [f32; 2],
    // Jump straight to the target time on the next block (after creation
    // or reset), instead of gliding from a stale value.
    snap: bool,
}

impl Delay {
    pub fn new(sr: f32) -> Self {
        let max = (MAX_DELAY_S * sr) as usize;
        let mut d = Self {
            sr,
            lines: [DelayLine::new(max), DelayLine::new(max)],
            sync_index: 4,
            time_s: 0.3,
            bpm: 120.0,
            feedback: 0.35,
            tone_coef: 0.0,
            ping_pong: false,
            mix: 0.25,
            current_delay: 0.0,
            smooth_coef: one_pole_coef(0.05, sr),
            tone_state: [0.0; 2],
            snap: true,
        };
        d.set_tone(6_000.0);
        d
    }

    fn set_tone(&mut self, hz: f32) {
        self.tone_coef = (-std::f32::consts::TAU * hz.min(self.sr * 0.45) / self.sr).exp();
    }

    /// Delay length in samples from sync choice or free time.
    // RT-SAFE
    fn target_delay(&self) -> f32 {
        let beats = DELAY_SYNC_BEATS
            .get(self.sync_index)
            .copied()
            .unwrap_or(0.0) as f32;
        let seconds = if beats > 0.0 {
            beats * 60.0 / self.bpm.max(1.0)
        } else {
            self.time_s
        };
        (seconds * self.sr).clamp(1.0, MAX_DELAY_S * self.sr - 2.0)
    }
}

impl EffectProcessor for Delay {
    fn set_param(&mut self, index: usize, value: f32) {
        match param_id(EffectKind::Delay, index) {
            "sync" => self.sync_index = value.round().max(0.0) as usize,
            "time_s" => self.time_s = value,
            "feedback" => self.feedback = value.clamp(0.0, 0.95),
            "tone_hz" => self.set_tone(value),
            "ping_pong" => self.ping_pong = value >= 0.5,
            "mix" => self.mix = value.clamp(0.0, 1.0),
            _ => {}
        }
    }

    fn set_tempo(&mut self, bpm: f32) {
        self.bpm = bpm;
    }

    fn reset(&mut self) {
        self.lines.iter_mut().for_each(DelayLine::clear);
        self.tone_state = [0.0; 2];
        self.snap = true;
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let target = self.target_delay();
        if self.snap {
            self.current_delay = target;
            self.snap = false;
        }
        let (fb, tc, mix) = (self.feedback, self.tone_coef, self.mix);
        let [ll, lr] = &mut self.lines;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            self.current_delay = target + (self.current_delay - target) * self.smooth_coef;
            let echo_l = ll.read(self.current_delay);
            let echo_r = lr.read(self.current_delay);
            // Darken each repeat a little, like tape.
            self.tone_state[0] = echo_l + (self.tone_state[0] - echo_l) * tc;
            self.tone_state[1] = echo_r + (self.tone_state[1] - echo_r) * tc;
            let (fl, fr) = (self.tone_state[0] * fb, self.tone_state[1] * fb);
            if self.ping_pong {
                // Mono input enters on the left; repeats bounce across.
                ll.push((*l + *r) * 0.5 + fr);
                lr.push(fl);
            } else {
                ll.push(*l + fl);
                lr.push(*r + fr);
            }
            *l = *l * (1.0 - mix) + echo_l * mix;
            *r = *r * (1.0 - mix) + echo_r * mix;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;
    use daw_model::EffectKind;

    #[test]
    fn quarter_note_echo_at_120_bpm_lands_half_a_second_later() {
        // Sync index 2 = 1/4.
        let mut fx = with(
            EffectKind::Delay,
            &[("sync", 2.0), ("mix", 1.0), ("feedback", 0.0)],
        );
        fx.set_tempo(120.0);
        let mut input = vec![0.0; SR as usize];
        input[0] = 1.0;
        let (l, _) = run(fx.as_mut(), &input);
        let echo = l.iter().position(|s| s.abs() > 0.1).expect("echo");
        assert!(echo.abs_diff(24_000) <= 2, "echo at {echo}");
    }

    #[test]
    fn feedback_repeats_decay() {
        let mut fx = with(
            EffectKind::Delay,
            &[
                ("sync", 0.0),
                ("time_s", 0.1),
                ("feedback", 0.5),
                ("mix", 1.0),
                ("tone_hz", 20_000.0),
            ],
        );
        let mut input = vec![0.0; SR as usize];
        input[0] = 1.0;
        let (l, _) = run(fx.as_mut(), &input);
        let first = peak(&l[4_700..4_900]);
        let second = peak(&l[9_500..9_700]);
        assert!(
            first > 0.5 && second < first * 0.7 && second > 0.1,
            "{first} {second}"
        );
    }
}
