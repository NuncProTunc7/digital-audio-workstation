use daw_dsp::{Biquad, BiquadShape};
use daw_model::EffectKind;

use crate::{EffectProcessor, one_pole_coef, param_id};

/// An analog-style octaver: the note's fundamental is filtered out and
/// tracked; a flip-flop that changes sign on every other cycle makes a
/// square wave an octave down, shaped by how loud the note is and softened.
/// Rectifying the note doubles it, an octave up (fuzzy, like the pedals).
/// Tracks single notes; chords make it warble, as on the real thing.
pub struct Octaver {
    down: f32,
    up: f32,
    dry: f32,
    sides: [Side; 2],
    attack: f32,
    release: f32,
}

#[derive(Clone)]
struct Side {
    /// Isolates the fundamental the flip-flop listens to.
    track: [Biquad; 2],
    /// Smooths the octave-down square into something like a bass note.
    soften: [Biquad; 2],
    /// Removes the DC rectifying leaves.
    up_dc: Biquad,
    high: bool,
    flip: f32,
    envelope: f32,
}

impl Octaver {
    pub fn new(sr: f32) -> Self {
        let lp = |hz: f32| {
            let mut b = Biquad::new();
            b.set(BiquadShape::LowPass, hz, 0.7, sr);
            b
        };
        let mut up_dc = Biquad::new();
        up_dc.set(BiquadShape::HighPass, 40.0, 0.7, sr);
        let side = Side {
            track: [lp(500.0), lp(500.0)],
            soften: [lp(900.0), lp(1_400.0)],
            up_dc,
            high: false,
            flip: 1.0,
            envelope: 0.0,
        };
        Self {
            down: 0.6,
            up: 0.0,
            dry: 0.8,
            sides: [side.clone(), side],
            attack: one_pole_coef(0.003, sr),
            release: one_pole_coef(0.08, sr),
        }
    }
}

impl EffectProcessor for Octaver {
    fn set_param(&mut self, index: usize, value: f32) {
        match param_id(EffectKind::Octaver, index) {
            "down" => self.down = value.clamp(0.0, 1.0),
            "up" => self.up = value.clamp(0.0, 1.0),
            "dry" => self.dry = value.clamp(0.0, 1.0),
            _ => {}
        }
    }

    fn reset(&mut self) {
        for s in &mut self.sides {
            s.track
                .iter_mut()
                .chain(s.soften.iter_mut())
                .for_each(Biquad::reset);
            s.up_dc.reset();
            s.high = false;
            s.flip = 1.0;
            s.envelope = 0.0;
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let (down, up, dry) = (self.down, self.up, self.dry);
        let (attack, release) = (self.attack, self.release);
        let [sl, sr] = &mut self.sides;
        for (buf, s) in [(left, sl), (right, sr)] {
            for x in buf.iter_mut() {
                let level = x.abs();
                let coef = if level > s.envelope { attack } else { release };
                s.envelope = level + (s.envelope - level) * coef;
                let first = s.track[0].process(*x);
                let f = s.track[1].process(first);
                // Rising through a small window (hysteresis) = a new cycle;
                // the flip-flop changes every other one.
                let window = 0.05 * s.envelope;
                if !s.high && f > window {
                    s.high = true;
                    s.flip = -s.flip;
                } else if s.high && f < -window {
                    s.high = false;
                }
                let mut sub = s.flip * s.envelope;
                for b in &mut s.soften {
                    sub = b.process(sub);
                }
                let octave_up = s.up_dc.process(x.abs());
                *x = *x * dry + sub * down * 1.5 + octave_up * up * 1.5;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;
    use daw_model::EffectKind;

    #[test]
    fn adds_the_octave_below() {
        let mut fx = with(EffectKind::Octaver, &[("down", 1.0), ("dry", 0.0)]);
        let (l, _) = run(fx.as_mut(), &sine(220.0, 0.4, 0.6));
        let tail = &l[9_600..];
        assert!(level_at(tail, 110.0) > 0.1, "sub {}", level_at(tail, 110.0));
        assert!(level_at(tail, 110.0) > level_at(tail, 220.0) * 2.0);
        assert!(peak(tail) < 2.0);
    }

    #[test]
    fn octave_up_doubles_the_note() {
        let mut fx = with(
            EffectKind::Octaver,
            &[("down", 0.0), ("up", 1.0), ("dry", 0.0)],
        );
        let (l, _) = run(fx.as_mut(), &sine(220.0, 0.4, 0.6));
        let tail = &l[9_600..];
        assert!(level_at(tail, 440.0) > level_at(tail, 220.0) * 5.0);
    }
}
