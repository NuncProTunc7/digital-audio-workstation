//! Measuring how late recordings arrive (Bluetooth headsets add a delay the
//! sound card doesn't report).
//!
//! The user claps along with metronome clicks while the microphone records.
//! Recording already places the take where the clock says it belongs, so
//! any steady difference between the claps and the beats is the delay left
//! to correct. Claps work where a played-back click wouldn't: Bluetooth
//! headsets cancel echo, which hides the click from their own microphone.

use std::time::{Duration, Instant};

use daw_engine::capture::AudioRecorder;
use daw_model::Project;
use serde::Serialize;

use crate::host::{Host, engine};

/// Slow enough that a delay of up to ±330 ms is still nearest its own beat.
pub const CALIBRATION_TEMPO_BPM: f64 = 80.0;
/// Clicks to get the feel before clapping.
pub const COUNT_IN_BEATS: u32 = 4;
/// Clicks to clap along with.
pub const CLAP_BEATS: u32 = 8;
/// Fewest claps that give a usable measurement.
const MIN_CLAPS: usize = 4;
/// Claps further apart than this (median deviation, ms) are too uneven.
const MAX_SPREAD_MS: f64 = 40.0;

/// The result of clapping along.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Calibration {
    /// How late the claps arrived (ms); the recording offset to use.
    pub offset_ms: f64,
    /// Claps heard on the beats.
    pub claps: usize,
    /// How much the claps varied (median deviation, ms).
    pub spread_ms: f64,
}

impl Calibration {
    /// Steady enough to use.
    pub fn is_reliable(&self) -> bool {
        self.claps >= MIN_CLAPS && self.spread_ms <= MAX_SPREAD_MS
    }
}

/// Finds the claps in a take and how far, on average, they are from the
/// beats. `start_beats` is where the take begins on the timeline.
pub fn measure_offset(
    samples: &[f32],
    sample_rate_hz: u32,
    start_beats: f64,
    tempo_bpm: f64,
) -> Result<Calibration, String> {
    let sr = f64::from(sample_rate_hz.max(1));
    let beat_s = 60.0 / tempo_bpm;
    let first = f64::from(COUNT_IN_BEATS);
    let last = f64::from(COUNT_IN_BEATS + CLAP_BEATS - 1);
    let index_at = |beats: f64| -> usize {
        (((beats - start_beats) * beat_s * sr).max(0.0) as usize).min(samples.len())
    };
    let window = &samples[index_at(first - 0.45)..index_at(last + 0.45)];
    let offset = index_at(first - 0.45);
    let peak = window.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak < 0.01 {
        return Err("no claps were heard; check the microphone meter moves when you clap".into());
    }
    let threshold = peak * 0.3;
    let refractory = (0.25 * sr) as usize;
    let mut deviations_ms: Vec<f64> = Vec::new();
    let mut used_beats: Vec<i64> = Vec::new();
    let mut i = 0;
    while i < window.len() {
        if window[i].abs() < threshold {
            i += 1;
            continue;
        }
        let beats = start_beats + (offset + i) as f64 / sr / beat_s;
        let nearest = beats.round();
        let k = nearest as i64;
        if (first..=last).contains(&nearest)
            && (beats - nearest).abs() <= 0.45
            && !used_beats.contains(&k)
        {
            used_beats.push(k);
            deviations_ms.push((beats - nearest) * beat_s * 1000.0);
        }
        i += refractory;
    }
    if deviations_ms.len() < MIN_CLAPS {
        return Err(format!(
            "only {} claps were heard; clap clearly on each of the {CLAP_BEATS} clicks after the first {COUNT_IN_BEATS}",
            deviations_ms.len()
        ));
    }
    let offset_ms = median(&mut deviations_ms.clone());
    let mut spread: Vec<f64> = deviations_ms
        .iter()
        .map(|d| (d - offset_ms).abs())
        .collect();
    Ok(Calibration {
        offset_ms: offset_ms.round(),
        claps: deviations_ms.len(),
        spread_ms: median(&mut spread).round(),
    })
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n == 0 {
        0.0
    } else if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// Plays clicks with the song silenced and records the user clapping along
/// (about ten seconds), then puts the song back. Blocks until done.
pub fn calibrate_recording<H: Host>(
    host: &H,
    recorder: &mut AudioRecorder,
) -> Result<Calibration, String> {
    if recorder.is_recording() {
        return Err("stop recording first".into());
    }
    let engine = engine(host)?;
    engine.stop();
    // Only the clicks: an empty song at a steady tempo.
    let mut clicks = Project::default();
    clicks.tracks.clear();
    clicks.tempo_bpm = CALIBRATION_TEMPO_BPM;
    clicks.loop_region.enabled = false;
    engine.sync(&clicks);
    engine.locate(0.0);
    engine.set_metronome(true);
    let path = std::env::temp_dir().join(format!("npt-calibration-{}.wav", std::process::id()));
    let started = recorder.start(
        "calibration.wav".into(),
        path.clone(),
        engine.status_handle(),
        0.0,
    );
    let end_beats = f64::from(COUNT_IN_BEATS + CLAP_BEATS) + 0.5;
    if started.is_ok() {
        engine.play();
        let deadline = Instant::now()
            + Duration::from_secs_f64(end_beats * 60.0 / CALIBRATION_TEMPO_BPM + 3.0);
        while engine.status().position_beats < end_beats && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let take = recorder.stop();
    // Put the song back however the recording went.
    engine.stop();
    engine.locate(0.0);
    engine.set_metronome(host.metronome_on());
    {
        let session = host.session()?;
        engine.sync(session.project());
    }
    started.map_err(|e| e.to_string())?;
    let take = take
        .ok_or("the recording didn't start")?
        .map_err(|e| e.to_string())?;
    let audio = daw_audio::read_wav(&take.path).map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&take.path);
    let audio = audio?;
    let samples = audio.channels.first().map_or(&[][..], Vec::as_slice);
    measure_offset(
        samples,
        audio.sample_rate_hz,
        take.start_beats,
        CALIBRATION_TEMPO_BPM,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A take of claps `late_ms` after each beat (with human wobble), plus
    /// quiet noise and a stray clap during the count-in.
    fn claps(late_ms: f64, start_beats: f64, wobble_ms: &[f64]) -> Vec<f32> {
        let sr = 48_000.0;
        let beat_s = 60.0 / CALIBRATION_TEMPO_BPM;
        let len = ((13.0 - start_beats) * beat_s * sr) as usize;
        let mut x: Vec<f32> = (0..len)
            .map(|i| 0.002 * ((i as f32 * 12.9898).sin() * 43758.547).fract())
            .collect();
        let mut clap_at = |seconds: f64, gain: f32| {
            let at = (seconds * sr) as usize;
            for j in 0..2_000 {
                if let Some(s) = x.get_mut(at + j) {
                    *s += gain * (-(j as f32) / 300.0).exp() * if j % 2 == 0 { 1.0 } else { -1.0 };
                }
            }
        };
        clap_at((1.3 - start_beats) * beat_s, 0.9);
        for (n, beat) in (COUNT_IN_BEATS..COUNT_IN_BEATS + CLAP_BEATS).enumerate() {
            let w = wobble_ms
                .get(n % wobble_ms.len().max(1))
                .copied()
                .unwrap_or(0.0);
            clap_at(
                (f64::from(beat) - start_beats) * beat_s + (late_ms + w) / 1000.0,
                0.5 + 0.05 * n as f32,
            );
        }
        x
    }

    #[test]
    fn finds_a_bluetooth_sized_delay() {
        let wobble = [-12.0, 8.0, 3.0, -5.0, 10.0, -2.0, 0.0, 6.0];
        for late_ms in [180.0, 0.0, -40.0, 290.0] {
            // Takes rarely start exactly on a beat.
            let x = claps(late_ms, -0.37, &wobble);
            let c = measure_offset(&x, 48_000, -0.37, CALIBRATION_TEMPO_BPM).expect("measured");
            assert!((c.offset_ms - late_ms).abs() <= 5.0, "{late_ms}: {c:?}");
            assert_eq!(c.claps, 8);
            assert!(c.is_reliable(), "{c:?}");
        }
    }

    #[test]
    fn says_what_went_wrong() {
        let silence = vec![0.0f32; 48_000 * 12];
        let err = measure_offset(&silence, 48_000, 0.0, CALIBRATION_TEMPO_BPM).expect_err("silent");
        assert!(err.contains("no claps"));
        // Wildly uneven claps measure, but aren't trusted.
        let x = claps(100.0, 0.0, &[-150.0, 140.0, -120.0, 160.0]);
        let c = measure_offset(&x, 48_000, 0.0, CALIBRATION_TEMPO_BPM).expect("measured");
        assert!(!c.is_reliable(), "{c:?}");
    }
}
