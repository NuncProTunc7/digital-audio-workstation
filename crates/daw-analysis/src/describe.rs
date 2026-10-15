//! The spectrogram in words, for AIs that can't look at pictures (small
//! local models, `npt-mcp --core`). Says how loud and how bright each
//! stretch of the song is, where it changes, and any narrow frequency
//! build-ups, using bar numbers the AI can act on.

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

const FFT_SIZE: usize = 2048;
/// Long enough to resolve third-octave bands down to 31.5 Hz (2.9 Hz bins).
const PEAK_FFT_SIZE: usize = 16_384;
/// At most this many lines of per-stretch detail.
const MAX_STRETCHES: usize = 16;
/// Stretches quieter than this (dB below the loudest) count as silent.
const SILENT_DB: f64 = -50.0;
/// Third-octave centres scanned for narrow build-ups.
const THIRD_OCTAVES_HZ: [f64; 28] = [
    31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0, 500.0, 630.0,
    800.0, 1_000.0, 1_250.0, 1_600.0, 2_000.0, 2_500.0, 3_150.0, 4_000.0, 5_000.0, 6_300.0,
    8_000.0, 10_000.0, 12_500.0, 16_000.0,
];

/// Where the render sits in the song, so stretches can be named by bar.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// Song position of the first sample, in beats.
    pub start_beats: f64,
    /// Where the requested region ends (anything after is reverb tail).
    pub end_beats: f64,
    pub tempo_bpm: f64,
    pub beats_per_bar: f64,
}

/// One stretch of the song.
#[derive(Debug, Clone, Copy)]
struct Stretch {
    from_beats: f64,
    to_beats: f64,
    /// Mean power of the mono mix in this stretch.
    power: f64,
    /// Share of spectral energy below 250 Hz / 250 Hz–2 kHz / above 2 kHz.
    lows: f64,
    mids: f64,
    highs: f64,
    /// Spectral centroid in Hz: higher = brighter.
    brightness_hz: f64,
}

/// Describes interleaved stereo audio in plain sentences: an overview, one
/// line per stretch (a few bars each), notable changes, and frequency
/// build-ups. Returns a single line when the audio is silent.
pub fn describe_spectrogram(stereo: &[f32], sample_rate_hz: u32, timing: Timing) -> Vec<String> {
    let mono: Vec<f32> = stereo
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[l, r]| 0.5 * (l + r))
        .collect();
    let sr = f64::from(sample_rate_hz);
    let samples_per_beat = sr * 60.0 / timing.tempo_bpm.max(1.0);
    let render_end_beats = timing.start_beats + mono.len() as f64 / samples_per_beat;

    // Whole bars per stretch, doubling until the song fits in the limit.
    let bar = timing.beats_per_bar.max(1.0);
    let music_beats = (timing.end_beats - timing.start_beats).max(0.0);
    let mut stretch_beats = bar;
    while music_beats / stretch_beats > MAX_STRETCHES as f64 {
        stretch_beats *= 2.0;
    }

    let mut bounds = Vec::new();
    let mut from = timing.start_beats;
    while from < timing.end_beats - 1e-9 {
        let next_line = ((from / stretch_beats).floor() + 1.0) * stretch_beats;
        let to = next_line.min(timing.end_beats);
        bounds.push((from, to));
        from = to;
    }
    // The reverb tail is its own stretch, if long enough to measure.
    if render_end_beats - timing.end_beats > 0.25 {
        bounds.push((timing.end_beats, render_end_beats));
    }

    let fft = FftPlanner::<f64>::new().plan_fft_forward(FFT_SIZE);
    let window: Vec<f64> = (0..FFT_SIZE)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / FFT_SIZE as f64).cos())
        .collect();
    let bin_hz = sr / FFT_SIZE as f64;
    let mut buf = vec![Complex::new(0.0f64, 0.0); FFT_SIZE];

    let to_sample = |beats: f64| {
        (((beats - timing.start_beats) * samples_per_beat)
            .round()
            .max(0.0) as usize)
            .min(mono.len())
    };
    let stretches: Vec<Stretch> = bounds
        .iter()
        .map(|&(from_beats, to_beats)| {
            let part = &mono[to_sample(from_beats)..to_sample(to_beats)];
            let power = if part.is_empty() {
                0.0
            } else {
                part.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>() / part.len() as f64
            };
            let mut spectrum = vec![0.0f64; FFT_SIZE / 2];
            let mut start = 0;
            loop {
                for (i, b) in buf.iter_mut().enumerate() {
                    let s = part.get(start + i).copied().map_or(0.0, f64::from);
                    *b = Complex::new(s * window[i], 0.0);
                }
                fft.process(&mut buf);
                for (e, c) in spectrum.iter_mut().zip(&buf).skip(1) {
                    *e += c.norm_sqr();
                }
                start += FFT_SIZE / 2;
                if start + FFT_SIZE / 2 >= part.len() {
                    break;
                }
            }
            let (mut lows, mut mids, mut highs, mut weighted) = (0.0, 0.0, 0.0, 0.0);
            for (bin, e) in spectrum.iter().enumerate().skip(1) {
                let hz = bin as f64 * bin_hz;
                if hz < 250.0 {
                    lows += e;
                } else if hz < 2_000.0 {
                    mids += e;
                } else {
                    highs += e;
                }
                weighted += hz * e;
            }
            let total = lows + mids + highs;
            let share = |x: f64| if total > 0.0 { 100.0 * x / total } else { 0.0 };
            Stretch {
                from_beats,
                to_beats,
                power,
                lows: share(lows),
                mids: share(mids),
                highs: share(highs),
                brightness_hz: if total > 0.0 { weighted / total } else { 0.0 },
            }
        })
        .collect();

    let loudest = stretches.iter().map(|s| s.power).fold(0.0f64, f64::max);
    if loudest <= 1e-12 {
        return vec!["Spectrogram in words: the audio is silent.".into()];
    }
    let rel_db = |s: &Stretch| {
        if s.power > 0.0 {
            10.0 * (s.power / loudest).log10()
        } else {
            -120.0
        }
    };
    let is_tail = |s: &Stretch| s.from_beats >= timing.end_beats - 1e-9;
    let name = |s: &Stretch| name_stretch(s.from_beats, s.to_beats, bar, is_tail(s));

    let mut out = vec![format!(
        "Spectrogram in words. Each stretch: loudness in dB below the loudest stretch; \
         energy split into lows (below 250 Hz: kick, bass), mids (250 Hz-2 kHz: body, \
         chords, melody) and highs (above 2 kHz: hats, cymbals, bite, air); brightness is \
         the spectral centroid (around 300 Hz is dark, 1-2 kHz typical, above 3 kHz bright). \
         Stretches are {} bar{} long.",
        fmt_num(stretch_beats / bar),
        if stretch_beats > bar { "s" } else { "" },
    )];
    for s in &stretches {
        let db = rel_db(s);
        if db < SILENT_DB {
            out.push(format!("{}: silent.", capitalize(&name(s))));
        } else {
            out.push(format!(
                "{}: {}; lows {:.0}%, mids {:.0}%, highs {:.0}%; brightness {}.",
                capitalize(&name(s)),
                if s.power >= loudest {
                    "loudest".to_owned()
                } else if db > -0.5 {
                    "0 dB".to_owned()
                } else {
                    format!("{db:.0} dB")
                },
                s.lows,
                s.mids,
                s.highs,
                fmt_hz(s.brightness_hz),
            ));
        }
    }

    // Changes between neighbouring stretches of music (not the tail).
    let music: Vec<&Stretch> = stretches.iter().filter(|s| !is_tail(s)).collect();
    let mut changes = Vec::new();
    for pair in music.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (da, db) = (rel_db(a), rel_db(b));
        let at = name_stretch(b.from_beats, b.to_beats, bar, false);
        if da < SILENT_DB && db >= SILENT_DB {
            changes.push(format!("sound starts at {at}"));
            continue;
        }
        if db < SILENT_DB {
            if da >= SILENT_DB {
                changes.push(format!("goes silent at {at}"));
            }
            continue;
        }
        if db - da >= 6.0 {
            changes.push(format!("gets much louder at {at} (+{:.0} dB)", db - da));
        } else if da - db >= 6.0 {
            changes.push(format!("drops much quieter at {at} ({:.0} dB)", db - da));
        }
        if a.brightness_hz > 0.0 && b.brightness_hz >= 1.8 * a.brightness_hz {
            changes.push(format!("turns much brighter at {at}"));
        } else if b.brightness_hz > 0.0 && a.brightness_hz >= 1.8 * b.brightness_hz {
            changes.push(format!("turns much darker at {at}"));
        }
        if a.highs >= 10.0 && b.highs < 3.0 {
            changes.push(format!("the highs drop out at {at}"));
        } else if a.highs < 3.0 && b.highs >= 10.0 {
            changes.push(format!("highs come in at {at}"));
        }
        if a.lows >= 30.0 && b.lows < 8.0 {
            changes.push(format!("the lows drop out at {at}"));
        } else if a.lows < 8.0 && b.lows >= 30.0 {
            changes.push(format!("lows come in at {at}"));
        }
    }
    let sounding: Vec<f64> = music
        .iter()
        .map(|s| rel_db(s))
        .filter(|&d| d >= SILENT_DB)
        .collect();
    if !changes.is_empty() {
        out.push(format!("Changes: {}.", changes.join("; ")));
    } else if music.len() >= 4
        && sounding.len() == music.len()
        && sounding.iter().fold(0.0f64, |m, &d| m.min(d)) > -3.0
    {
        out.push(
            "Changes: none. Loudness and tone stay level the whole way, so there is little \
             build or release; for game music that can be fine as a bed, or vary the parts to add movement."
                .into(),
        );
    }

    out.extend(build_ups(&mono, sr));
    out
}

/// Narrow peaks: third-octave bands well above the average of their
/// neighbours, which can ring, whistle, or sound boomy.
fn build_ups(mono: &[f32], sr: f64) -> Option<String> {
    let fft = FftPlanner::<f64>::new().plan_fft_forward(PEAK_FFT_SIZE);
    let window: Vec<f64> = (0..PEAK_FFT_SIZE)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / PEAK_FFT_SIZE as f64).cos())
        .collect();
    let bin_hz = sr / PEAK_FFT_SIZE as f64;
    let mut buf = vec![Complex::new(0.0f64, 0.0); PEAK_FFT_SIZE];
    let mut spectrum = vec![0.0f64; PEAK_FFT_SIZE / 2];
    let mut start = 0;
    while start < mono.len() {
        for (i, b) in buf.iter_mut().enumerate() {
            let s = mono.get(start + i).copied().map_or(0.0, f64::from);
            *b = Complex::new(s * window[i], 0.0);
        }
        fft.process(&mut buf);
        for (e, c) in spectrum.iter_mut().zip(&buf).skip(1) {
            *e += c.norm_sqr();
        }
        start += PEAK_FFT_SIZE / 2;
    }
    let band_db: Vec<f64> = THIRD_OCTAVES_HZ
        .iter()
        .map(|&centre| {
            let (lo, hi) = (centre / 2f64.powf(1.0 / 6.0), centre * 2f64.powf(1.0 / 6.0));
            let e: f64 = spectrum
                .iter()
                .enumerate()
                .filter(|(bin, _)| (lo..hi).contains(&(*bin as f64 * bin_hz)))
                .map(|(_, e)| e)
                .sum();
            10.0 * e.max(1e-20).log10()
        })
        .collect();
    let loudest = band_db.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mut peaks: Vec<(f64, f64)> = (2..band_db.len() - 2)
        .filter_map(|i| {
            let around = [i - 2, i - 1, i + 1, i + 2];
            let neighbours = around.iter().map(|&j| band_db[j]).sum::<f64>() / 4.0;
            let lift = band_db[i] - neighbours;
            let is_local_max = around[1..3].iter().all(|&j| band_db[i] >= band_db[j]);
            (is_local_max && lift >= 10.0 && band_db[i] > loudest - 30.0)
                .then_some((THIRD_OCTAVES_HZ[i], lift))
        })
        .collect();
    if peaks.is_empty() {
        return None;
    }
    peaks.sort_by(|a, b| b.1.total_cmp(&a.1));
    let list: Vec<String> = peaks
        .iter()
        .take(3)
        .map(|(hz, lift)| {
            // Past ~30 dB the exact figure only says "a pure tone".
            let lift = if *lift > 30.0 {
                "over 30 dB".to_owned()
            } else {
                format!("{lift:.0} dB")
            };
            format!("around {} ({lift} above its neighbours)", fmt_hz(*hz))
        })
        .collect();
    Some(format!(
        "Narrow build-ups: {}. A single held or repeated pitch is normal there; otherwise \
         it may ring, whistle or boom, and a narrow EQ cut would tame it.",
        list.join(", ")
    ))
}

/// "bars 5-8", "bar 3", "beats 6.5-8", or "the tail after beat 16".
fn name_stretch(from_beats: f64, to_beats: f64, bar: f64, tail: bool) -> String {
    if tail {
        return format!("the tail after beat {}", fmt_num(from_beats));
    }
    let whole = |b: f64| (b / bar - (b / bar).round()).abs() < 1e-6;
    if whole(from_beats) && whole(to_beats) {
        let first = (from_beats / bar).round() as i64 + 1;
        let last = (to_beats / bar).round() as i64;
        if first == last {
            format!("bar {first}")
        } else {
            format!("bars {first}-{last}")
        }
    } else {
        format!("beats {}-{}", fmt_num(from_beats), fmt_num(to_beats))
    }
}

fn fmt_num(x: f64) -> String {
    let s = format!("{x:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn fmt_hz(hz: f64) -> String {
    if hz >= 1_000.0 {
        format!("{} kHz", fmt_num((hz / 100.0).round() / 10.0))
    } else {
        format!("{:.0} Hz", hz)
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    /// 120 bpm in 4/4: one bar = 2 s.
    fn timing(bars: f64, tail_s: f64) -> (Timing, usize) {
        let t = Timing {
            start_beats: 0.0,
            end_beats: bars * 4.0,
            tempo_bpm: 120.0,
            beats_per_bar: 4.0,
        };
        (t, ((bars * 2.0 + tail_s) * f64::from(SR)) as usize)
    }

    /// Stereo audio built per bar from `f(bar, seconds)`.
    fn audio(frames: usize, mut f: impl FnMut(usize, f32) -> f32) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let t = i as f32 / SR as f32;
                let s = f((t / 2.0) as usize, t);
                [s, s]
            })
            .collect()
    }

    fn sine(hz: f32, t: f32) -> f32 {
        (std::f32::consts::TAU * hz * t).sin()
    }

    #[test]
    fn silence_says_so() {
        let (t, n) = timing(4.0, 0.0);
        let lines = describe_spectrogram(&vec![0.0; n * 2], SR, t);
        assert_eq!(lines, ["Spectrogram in words: the audio is silent."]);
    }

    #[test]
    fn names_each_bar_and_the_band_it_sits_in() {
        let (t, n) = timing(2.0, 0.0);
        // Bar 1: bass at 100 Hz. Bar 2: a 5 kHz tone.
        let a = audio(n, |bar, t| {
            0.3 * sine(if bar == 0 { 100.0 } else { 5_000.0 }, t)
        });
        let lines = describe_spectrogram(&a, SR, t);
        let bar1 = lines
            .iter()
            .find(|l| l.starts_with("Bar 1:"))
            .expect("bar 1");
        let bar2 = lines
            .iter()
            .find(|l| l.starts_with("Bar 2:"))
            .expect("bar 2");
        assert!(bar1.contains("lows 100%"), "{bar1}");
        assert!(bar2.contains("highs 100%"), "{bar2}");
        assert!(bar2.contains("brightness 5 kHz"), "{bar2}");
        let changes = lines
            .iter()
            .find(|l| l.starts_with("Changes:"))
            .expect("changes");
        assert!(
            changes.contains("turns much brighter at bar 2"),
            "{changes}"
        );
        assert!(changes.contains("highs come in at bar 2"), "{changes}");
        assert!(changes.contains("the lows drop out at bar 2"), "{changes}");
    }

    #[test]
    fn reports_loudness_jumps_and_silence() {
        let (t, n) = timing(4.0, 0.0);
        let a = audio(n, |bar, t| match bar {
            0 => 0.02 * sine(440.0, t),
            1 => 0.4 * sine(440.0, t),
            _ => 0.0,
        });
        let lines = describe_spectrogram(&a, SR, t);
        let changes = lines
            .iter()
            .find(|l| l.starts_with("Changes:"))
            .expect("changes");
        assert!(
            changes.contains("gets much louder at bar 2 (+26 dB)"),
            "{changes}"
        );
        assert!(changes.contains("goes silent at bar 3"), "{changes}");
        assert!(lines.iter().any(|l| l == "Bar 4: silent."), "{lines:?}");
        assert!(
            lines.iter().any(|l| l.starts_with("Bar 2: loudest;")),
            "{lines:?}"
        );
    }

    #[test]
    fn long_songs_group_bars_and_name_the_tail() {
        let (t, n) = timing(64.0, 1.5);
        let a = audio(n, |_, t| 0.2 * sine(300.0, t));
        let lines = describe_spectrogram(&a, SR, t);
        assert!(
            lines[0].contains("Stretches are 4 bars long"),
            "{}",
            lines[0]
        );
        assert!(
            lines.iter().any(|l| l.starts_with("Bars 61-64:")),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("The tail after beat 256:")),
            "{lines:?}"
        );
        assert!(lines.len() <= 1 + MAX_STRETCHES + 1 + 2 + 1);
        assert!(
            lines.iter().any(|l| l.starts_with("Changes: none")),
            "{lines:?}"
        );
    }

    #[test]
    fn finds_a_narrow_build_up_over_noise() {
        let (t, n) = timing(2.0, 0.0);
        // Deterministic white-ish noise plus a strong 2.5 kHz whistle.
        let mut seed = 1u32;
        let a = audio(n, |_, t| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5;
            0.1 * noise + 0.3 * sine(2_500.0, t)
        });
        let lines = describe_spectrogram(&a, SR, t);
        let line = lines
            .iter()
            .find(|l| l.starts_with("Narrow build-ups:"))
            .expect("build-up");
        assert!(line.contains("around 2.5 kHz"), "{line}");
    }

    #[test]
    fn region_not_on_a_bar_line_is_named_in_beats() {
        let t = Timing {
            start_beats: 2.0,
            end_beats: 10.0,
            tempo_bpm: 120.0,
            beats_per_bar: 4.0,
        };
        let a = audio(4 * SR as usize, |_, t| 0.2 * sine(300.0, t));
        let lines = describe_spectrogram(&a, SR, t);
        assert!(
            lines.iter().any(|l| l.starts_with("Beats 2-4:")),
            "{lines:?}"
        );
        assert!(lines.iter().any(|l| l.starts_with("Bar 2:")), "{lines:?}");
        assert!(
            lines.iter().any(|l| l.starts_with("Beats 8-10:")),
            "{lines:?}"
        );
    }
}
