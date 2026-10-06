//! Changing a recording's speed without changing its pitch (WSOLA:
//! waveform-similarity overlap-add). Short windows are copied from the
//! input at the new speed and cross-faded, each one nudged to wherever it
//! lines up best with what came before, so there's no phasing or flutter.
//! Works well for voice, guitar, and most single instruments.

use crate::AudioData;

/// Window length, seconds: long enough for low notes, short enough to keep
/// drum hits tight.
const WINDOW_S: f64 = 0.040;
/// How far a window may be nudged to line up, seconds.
const TOLERANCE_S: f64 = 0.006;
/// Similarity is measured on every Nth sample (plenty for alignment).
const DECIMATE: usize = 4;

/// Stretches `data` to `ratio` times its length (2.0 = half speed, same
/// pitch). Ratios very close to 1 return a copy.
pub fn time_stretch(data: &AudioData, ratio: f64) -> AudioData {
    let ratio = ratio.clamp(0.25, 4.0);
    if (ratio - 1.0).abs() < 1e-4 || data.frames() == 0 {
        return data.clone();
    }
    let sr = f64::from(data.sample_rate_hz.max(1));
    let n = ((WINDOW_S * sr) as usize).max(64) & !1;
    let hs = n / 2; // synthesis hop: 50% overlap
    let ha = hs as f64 / ratio; // analysis hop
    let tol = (TOLERANCE_S * sr) as isize;
    let frames = data.frames();
    let out_frames = (frames as f64 * ratio).ceil() as usize;
    let channels = data.channels.len().max(1);

    // Mono guide for alignment.
    let mono: Vec<f32> = (0..frames)
        .map(|i| data.channels.iter().map(|c| c[i]).sum::<f32>() / channels as f32)
        .collect();
    let window: Vec<f32> = (0..n)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos())
        .collect();

    let mut out = vec![vec![0.0f32; out_frames + n]; channels];
    let mut weight = vec![0.0f32; out_frames + n];
    let mut prev: isize = 0;
    let mut k = 0usize;
    loop {
        let out_pos = k * hs;
        if out_pos >= out_frames {
            break;
        }
        let ideal = (k as f64 * ha).round() as isize;
        let chosen = if k == 0 {
            0
        } else {
            // The input that would naturally follow the last window...
            let natural = prev + hs as isize;
            let mut best = (ideal, f32::MIN);
            let mut d = -tol;
            while d <= tol {
                let cand = ideal + d;
                // ...compared with each candidate over the overlap.
                let mut corr = 0.0f32;
                let mut i = 0usize;
                while i < hs {
                    let a = sample(&mono, natural + i as isize);
                    let b = sample(&mono, cand + i as isize);
                    corr += a * b;
                    i += DECIMATE;
                }
                if corr > best.1 {
                    best = (cand, corr);
                }
                d += 2;
            }
            best.0
        };
        for (c, ch) in data.channels.iter().enumerate() {
            for (i, w) in window.iter().enumerate() {
                out[c][out_pos + i] += w * sample(ch, chosen + i as isize);
            }
        }
        for (i, w) in window.iter().enumerate() {
            weight[out_pos + i] += w;
        }
        prev = chosen;
        k += 1;
    }
    for ch in &mut out {
        for (s, w) in ch.iter_mut().zip(&weight) {
            if *w > 1e-3 {
                *s /= w;
            }
        }
        ch.truncate(out_frames);
    }
    AudioData {
        sample_rate_hz: data.sample_rate_hz,
        channels: out,
    }
}

fn sample(x: &[f32], i: isize) -> f32 {
    usize::try_from(i)
        .ok()
        .and_then(|i| x.get(i))
        .copied()
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, seconds: f32, sr: u32) -> AudioData {
        let n = (seconds * sr as f32) as usize;
        AudioData {
            sample_rate_hz: sr,
            channels: vec![
                (0..n)
                    .map(|i| 0.5 * (std::f32::consts::TAU * freq * i as f32 / sr as f32).sin())
                    .collect(),
            ],
        }
    }

    fn pitch_hz(x: &[f32], sr: u32) -> f32 {
        let c = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        c as f32 * sr as f32 / x.len() as f32
    }

    #[test]
    fn slower_and_faster_keep_pitch_and_level() {
        let d = tone(330.0, 2.0, 48_000);
        for ratio in [1.5, 0.75] {
            let s = time_stretch(&d, ratio);
            let expected = (d.frames() as f64 * ratio).ceil() as usize;
            assert_eq!(s.frames(), expected);
            let body = &s.channels[0][4_800..s.frames() - 4_800];
            assert!(
                (pitch_hz(body, 48_000) - 330.0).abs() < 3.0,
                "{ratio}: {}",
                pitch_hz(body, 48_000)
            );
            // Level stays steady: no dips where windows overlap.
            let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
            for chunk in body.chunks(2_400) {
                let r = rms(chunk);
                assert!((r - 0.3535).abs() < 0.04, "{ratio}: rms {r}");
            }
        }
    }

    #[test]
    fn ratio_one_is_a_copy() {
        let d = tone(200.0, 0.1, 44_100);
        assert_eq!(time_stretch(&d, 1.0), d);
    }
}
