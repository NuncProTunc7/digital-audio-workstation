use ebur128::{EbuR128, Mode};
use rustfft::FftPlanner;
use rustfft::num_complex::Complex;
use serde::Serialize;

/// A frequency band and its share of the total energy.
#[derive(Debug, Clone, Serialize)]
pub struct Band {
    pub name: &'static str,
    pub from_hz: f64,
    pub to_hz: f64,
    /// 0–100.
    pub share_percent: f64,
}

const BANDS: [(&str, f64, f64); 6] = [
    ("sub", 20.0, 60.0),
    ("bass", 60.0, 250.0),
    ("low_mids", 250.0, 500.0),
    ("mids", 500.0, 2_000.0),
    ("high_mids", 2_000.0, 6_000.0),
    ("highs", 6_000.0, 20_000.0),
];

/// Loudness, peak, stereo, and tonal measurements of a stereo signal.
#[derive(Debug, Clone, Serialize)]
pub struct Measurements {
    pub duration_s: f64,
    /// EBU R128 integrated loudness; None when silent.
    pub integrated_lufs: Option<f64>,
    /// Loudest 3-second stretch.
    pub max_short_term_lufs: Option<f64>,
    /// Inter-sample ("true") peak, in dB relative to full scale.
    pub true_peak_dbtp: f64,
    pub sample_peak_dbfs: f64,
    pub rms_dbfs: f64,
    /// True peak minus integrated loudness: higher = punchier, lower = squashed.
    pub peak_to_loudness_db: Option<f64>,
    /// +1 mono, 0 wide/unrelated, negative = out of phase.
    pub stereo_correlation: f64,
    pub bands: Vec<Band>,
    /// Sum of squared samples (both channels), for comparing tracks.
    #[serde(skip)]
    pub energy: f64,
}

fn to_db(linear: f64) -> f64 {
    if linear > 0.0 {
        20.0 * linear.log10()
    } else {
        -120.0
    }
}

fn loudness(value: Result<f64, ebur128::Error>) -> Option<f64> {
    value.ok().filter(|v| v.is_finite() && *v > -70.0)
}

/// Measures interleaved stereo audio.
pub fn measure(stereo: &[f32], sample_rate_hz: u32) -> Measurements {
    let frames = stereo.len() / 2;
    let mut meter = EbuR128::new(
        2,
        sample_rate_hz,
        Mode::I | Mode::S | Mode::TRUE_PEAK | Mode::SAMPLE_PEAK,
    )
    .ok();
    let mut max_short_term: Option<f64> = None;
    if let Some(m) = meter.as_mut() {
        // Feed 100 ms at a time so the loudest short-term window is seen.
        let chunk = (sample_rate_hz as usize / 10) * 2;
        for block in stereo.chunks(chunk) {
            if m.add_frames_f32(block).is_err() {
                break;
            }
            if let Some(st) = loudness(m.loudness_shortterm()) {
                max_short_term = Some(max_short_term.map_or(st, |cur: f64| cur.max(st)));
            }
        }
    }
    let integrated = meter.as_ref().and_then(|m| loudness(m.loudness_global()));
    let peak_of = |f: &dyn Fn(&EbuR128, u32) -> Result<f64, ebur128::Error>| {
        meter
            .as_ref()
            .map(|m| (0..2).filter_map(|c| f(m, c).ok()).fold(0.0f64, f64::max))
            .unwrap_or(0.0)
    };
    let true_peak = peak_of(&|m, c| m.true_peak(c));
    let sample_peak = peak_of(&|m, c| m.sample_peak(c));

    let (mut sum_sq, mut sum_l2, mut sum_r2, mut sum_lr) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for f in stereo.chunks_exact(2) {
        let (l, r) = (f64::from(f[0]), f64::from(f[1]));
        sum_sq += l * l + r * r;
        sum_l2 += l * l;
        sum_r2 += r * r;
        sum_lr += l * r;
    }
    let rms = if frames > 0 {
        (sum_sq / (2 * frames) as f64).sqrt()
    } else {
        0.0
    };
    let correlation = if sum_l2 > 0.0 && sum_r2 > 0.0 {
        sum_lr / (sum_l2 * sum_r2).sqrt()
    } else {
        1.0
    };

    Measurements {
        duration_s: frames as f64 / f64::from(sample_rate_hz),
        integrated_lufs: integrated,
        max_short_term_lufs: max_short_term,
        true_peak_dbtp: to_db(true_peak),
        sample_peak_dbfs: to_db(sample_peak),
        rms_dbfs: to_db(rms),
        peak_to_loudness_db: integrated.map(|l| to_db(true_peak) - l),
        stereo_correlation: correlation,
        bands: band_shares(stereo, sample_rate_hz),
        energy: sum_sq,
    }
}

/// Share of spectral energy per band, from an averaged FFT of the mono mix.
fn band_shares(stereo: &[f32], sample_rate_hz: u32) -> Vec<Band> {
    const N: usize = 4096;
    let mono: Vec<f32> = stereo
        .chunks_exact(2)
        .map(|f| 0.5 * (f[0] + f[1]))
        .collect();
    let mut totals = [0.0f64; BANDS.len()];
    if mono.len() >= N {
        let fft = FftPlanner::<f32>::new().plan_fft_forward(N);
        let window: Vec<f32> = (0..N)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos())
            .collect();
        let bin_hz = f64::from(sample_rate_hz) / N as f64;
        let mut buf = vec![Complex::new(0.0f32, 0.0); N];
        for start in (0..=mono.len() - N).step_by(N / 2) {
            for (i, b) in buf.iter_mut().enumerate() {
                *b = Complex::new(mono[start + i] * window[i], 0.0);
            }
            fft.process(&mut buf);
            for (bin, c) in buf.iter().enumerate().take(N / 2).skip(1) {
                let hz = bin as f64 * bin_hz;
                if let Some(i) = BANDS.iter().position(|(_, lo, hi)| hz >= *lo && hz < *hi) {
                    totals[i] += f64::from(c.norm_sqr());
                }
            }
        }
    }
    let sum: f64 = totals.iter().sum();
    BANDS
        .iter()
        .zip(totals)
        .map(|(&(name, from_hz, to_hz), e)| Band {
            name,
            from_hz,
            to_hz,
            share_percent: if sum > 0.0 { 100.0 * e / sum } else { 0.0 },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn sine(freq: f32, amp: f32, seconds: f32, right_sign: f32) -> Vec<f32> {
        (0..(seconds * SR as f32) as usize)
            .flat_map(|i| {
                let s = (std::f32::consts::TAU * freq * i as f32 / SR as f32).sin() * amp;
                [s, s * right_sign]
            })
            .collect()
    }

    #[test]
    fn full_scale_1khz_sine_measures_as_expected() {
        // A 1 kHz sine peaking at -20 dBFS in both channels reads -20 LUFS:
        // -23 dB RMS per channel, +3 dB for two channels, +0.7 dB K-weighting
        // at 1 kHz, -0.691 dB calibration offset (ITU-R BS.1770).
        let m = measure(&sine(1_000.0, 0.1, 5.0, 1.0), SR);
        let lufs = m.integrated_lufs.expect("loud enough");
        assert!((lufs - -20.0).abs() < 0.3, "{lufs}");
        assert!(
            (m.sample_peak_dbfs - -20.0).abs() < 0.1,
            "{}",
            m.sample_peak_dbfs
        );
        assert!((m.stereo_correlation - 1.0).abs() < 1e-6);
        let mids = m.bands.iter().find(|b| b.name == "mids").expect("band");
        assert!(mids.share_percent > 95.0, "{}", mids.share_percent);
    }

    #[test]
    fn silence_has_no_loudness() {
        let m = measure(&vec![0.0; SR as usize * 2], SR);
        assert!(m.integrated_lufs.is_none());
        assert!(m.sample_peak_dbfs <= -120.0);
    }

    #[test]
    fn inverted_channels_have_negative_correlation() {
        let m = measure(&sine(200.0, 0.3, 1.0, -1.0), SR);
        assert!(m.stereo_correlation < -0.99);
    }

    #[test]
    fn bass_tone_lands_in_bass_band() {
        let m = measure(&sine(100.0, 0.3, 2.0, 1.0), SR);
        let bass = m.bands.iter().find(|b| b.name == "bass").expect("band");
        assert!(bass.share_percent > 95.0);
    }
}
