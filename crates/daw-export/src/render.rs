//! Rendering regions as seamless loops, and loudness normalization.

use std::sync::Arc;

use daw_engine::AudioPool;
use daw_engine::offline::render_region;
use daw_model::{Project, TrackId};

use crate::{ExportError, GAME_SAMPLE_RATE_HZ};

/// How much ring-out (reverb, echo, release) is rendered past the end.
const TAIL_SECONDS: f64 = 4.0;
/// A non-looping export keeps this much of the ring-out.
const ONE_SHOT_TAIL_SECONDS: f64 = 2.0;
/// Normalizing never pushes peaks above this (dBFS).
const PEAK_CEILING_DB: f64 = -1.0;

/// Rendered audio for one file.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedLoop {
    /// Interleaved stereo at [`GAME_SAMPLE_RATE_HZ`].
    pub stereo: Vec<f32>,
    pub looped: bool,
    /// Loop length in beats and the tempo, for Godot's beat-synced looping.
    pub beats: f64,
    pub tempo_bpm: f64,
    pub beats_per_bar: u8,
}

impl RenderedLoop {
    pub fn seconds(&self) -> f64 {
        self.stereo.len() as f64 / 2.0 / f64::from(GAME_SAMPLE_RATE_HZ)
    }

    pub fn is_silent(&self) -> bool {
        self.stereo.iter().all(|s| s.abs() < 1e-5)
    }
}

/// The region an export covers when none is given: the loop region if it
/// is on, else the whole song rounded up to whole bars.
pub(crate) fn default_region(
    project: &Project,
    start: Option<f64>,
    end: Option<f64>,
) -> Result<(f64, f64), ExportError> {
    let bpb = project.beats_per_bar();
    let (s, e) = match (start, end) {
        (Some(s), Some(e)) => (s, e),
        _ if project.loop_region.enabled => (
            project.loop_region.start_beats,
            project.loop_region.end_beats,
        ),
        _ => (
            start.unwrap_or(0.0),
            end.unwrap_or_else(|| (project.end_beats() / bpb).ceil() * bpb),
        ),
    };
    if !(e > s && s >= 0.0) {
        return Err(ExportError::Empty(
            "the song is empty (add some clips first)".into(),
        ));
    }
    Ok((s, e))
}

/// Renders beats `start..end`. Looped: the ring-out after `end` is folded
/// back onto the start, so the file loops with no gap or click and its
/// first bar already has the reverb of the last. `solo` renders one track.
pub fn render_loop(
    project: &Project,
    audio: &Arc<AudioPool>,
    start: f64,
    end: f64,
    looped: bool,
    solo: Option<TrackId>,
) -> RenderedLoop {
    let mut p = project.clone();
    if let Some(id) = solo {
        for t in &mut p.tracks {
            t.mixer.solo = false;
            t.mixer.mute = t.id != id;
        }
    }
    let sr = f64::from(GAME_SAMPLE_RATE_HZ);
    let seconds = (end - start) * 60.0 / p.tempo_bpm;
    let loop_frames = (seconds * sr).round() as usize;
    let tail = if looped {
        TAIL_SECONDS
    } else {
        ONE_SHOT_TAIL_SECONDS
    };
    let mut stereo = render_region(&p, audio, start, end, tail, GAME_SAMPLE_RATE_HZ);
    if looped && loop_frames > 0 {
        let split = (loop_frames * 2).min(stereo.len());
        let (body, rest) = stereo.split_at_mut(split);
        // The tail may be longer than a short loop: wrap it round as often
        // as needed.
        for (i, s) in rest.iter().enumerate() {
            body[i % body.len()] += s;
        }
        stereo.truncate(loop_frames * 2);
    }
    RenderedLoop {
        stereo,
        looped,
        beats: end - start,
        tempo_bpm: p.tempo_bpm,
        beats_per_bar: p.time_signature.numerator,
    }
}

/// Gain (dB) that brings `main` to `target_lufs` without peaks above -1 dBFS.
pub(crate) fn normalize_gain(main: &RenderedLoop, target_lufs: Option<f64>) -> f64 {
    let Some(target) = target_lufs else {
        return 0.0;
    };
    let m = daw_analysis::measure(&main.stereo, GAME_SAMPLE_RATE_HZ);
    let Some(lufs) = m.integrated_lufs.filter(|l| l.is_finite()) else {
        return 0.0;
    };
    let wanted = target - lufs;
    let peak_db = 20.0 * f64::from(main.stereo.iter().fold(0.0f32, |a, s| a.max(s.abs()))).log10();
    let headroom = PEAK_CEILING_DB - peak_db;
    wanted.min(headroom).clamp(-40.0, 40.0)
}

pub(crate) fn apply_gain(stereo: &[f32], gain_db: f64) -> Vec<f32> {
    let g = 10f32.powf(gain_db as f32 / 20.0);
    stereo.iter().map(|s| (s * g).clamp(-1.0, 1.0)).collect()
}
