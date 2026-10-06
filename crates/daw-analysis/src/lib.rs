//! Mix analysis: Claude's "ears".
//!
//! Renders a project offline (exactly as it sounds in the app) and measures
//! loudness, peaks, stereo image, tonal balance, and per-track levels, then
//! turns the numbers into plain-language hints. Optionally draws a
//! spectrogram so Claude can look at the sound.

mod measure;
mod spectrogram;

use daw_engine::offline::render_region;
use daw_model::Project;
use serde::Serialize;

pub use measure::{Band, Measurements, measure};
pub use spectrogram::spectrogram_png;

pub const SAMPLE_RATE_HZ: u32 = 48_000;
/// Seconds rendered after the end so reverb and echo tails are included.
const TAIL_SECONDS: f64 = 1.5;

/// What to analyze.
#[derive(Debug, Clone)]
pub struct AnalyzeOptions {
    /// Start, in beats (default: song start).
    pub start_beats: Option<f64>,
    /// End, in beats (default: end of the last clip).
    pub end_beats: Option<f64>,
    /// Also measure each track on its own.
    pub per_track: bool,
    /// Also draw a spectrogram PNG.
    pub spectrogram: bool,
}

impl Default for AnalyzeOptions {
    fn default() -> Self {
        Self {
            start_beats: None,
            end_beats: None,
            per_track: true,
            spectrogram: false,
        }
    }
}

/// One track's level in the mix.
#[derive(Debug, Clone, Serialize)]
pub struct TrackLevel {
    pub id: u32,
    pub name: String,
    /// Integrated loudness of this track alone (through the master chain).
    pub lufs: Option<f64>,
    pub peak_dbfs: f64,
    /// Share of the mix's total energy, 0–100.
    pub energy_share_percent: f64,
}

/// The analysis result.
#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub start_beats: f64,
    pub end_beats: f64,
    pub mix: Measurements,
    pub tracks: Vec<TrackLevel>,
    /// Plain-language observations, most important first.
    pub hints: Vec<String>,
    #[serde(skip)]
    pub spectrogram_png: Option<Vec<u8>>,
}

/// Renders and measures `project`.
pub fn analyze(project: &Project, options: &AnalyzeOptions) -> Analysis {
    let start = options.start_beats.unwrap_or(0.0).max(0.0);
    let end = options
        .end_beats
        .unwrap_or_else(|| project.end_beats())
        .max(start);
    let tail = if options.end_beats.is_some() {
        0.0
    } else {
        TAIL_SECONDS
    };
    let audio = render_region(project, start, end, tail, SAMPLE_RATE_HZ);
    let mix = measure(&audio, SAMPLE_RATE_HZ);

    let mut tracks = Vec::new();
    if options.per_track && project.tracks.len() > 1 {
        let mut energies = Vec::new();
        for t in &project.tracks {
            let mut solo = project.clone();
            for other in &mut solo.tracks {
                other.mixer.solo = false;
                other.mixer.mute = other.id != t.id;
            }
            let rendered = render_region(&solo, start, end, tail, SAMPLE_RATE_HZ);
            let m = measure(&rendered, SAMPLE_RATE_HZ);
            energies.push(m.energy);
            tracks.push(TrackLevel {
                id: t.id,
                name: t.name.clone(),
                lufs: m.integrated_lufs,
                peak_dbfs: m.sample_peak_dbfs,
                energy_share_percent: 0.0,
            });
        }
        let total: f64 = energies.iter().sum();
        for (t, e) in tracks.iter_mut().zip(energies) {
            t.energy_share_percent = if total > 0.0 { 100.0 * e / total } else { 0.0 };
        }
    }

    let hints = hints(project, &mix, &tracks);
    let spectrogram_png = options
        .spectrogram
        .then(|| spectrogram_png(&audio, SAMPLE_RATE_HZ, 900, 320));
    Analysis {
        start_beats: start,
        end_beats: end,
        mix,
        tracks,
        hints,
        spectrogram_png,
    }
}

fn hints(project: &Project, mix: &Measurements, tracks: &[TrackLevel]) -> Vec<String> {
    let mut out = Vec::new();
    let Some(lufs) = mix.integrated_lufs else {
        out.push(
            "The mix is silent in this range. Check that clips have notes and tracks aren't muted."
                .into(),
        );
        return out;
    };
    if mix.true_peak_dbtp > -1.0 {
        out.push(format!(
            "Peaks reach {:.1} dBTP, too close to 0 dB: they may distort when exported. Lower the master volume or add a Limiter last on the master with its ceiling at -1 dB.",
            mix.true_peak_dbtp
        ));
    }
    if lufs > -10.0 {
        out.push(format!("Very loud ({lufs:.1} LUFS). Game music usually sits around -16 to -20 LUFS so dialogue and sound effects have room."));
    } else if lufs < -26.0 {
        out.push(format!("Quiet overall ({lufs:.1} LUFS). Raise track or master volumes; game music commonly sits around -16 to -20 LUFS."));
    }
    let share = |name: &str| {
        mix.bands
            .iter()
            .find(|b| b.name == name)
            .map_or(0.0, |b| b.share_percent)
    };
    let low = share("sub") + share("bass");
    // Most music carries the bulk of its raw energy below 250 Hz, so only
    // flag clearly extreme cases.
    if low > 88.0 {
        out.push(format!("Bass-heavy: {low:.0}% of the energy is below 250 Hz. Consider lowering the bass, or an EQ low cut on non-bass tracks."));
    }
    if share("highs") < 1.0 && share("high_mids") < 4.0 {
        out.push("Dull top end: very little energy above 2 kHz. Brighter presets, opening filter cutoffs, or an EQ high shelf would add clarity.".into());
    } else if share("highs") + share("high_mids") > 45.0 {
        out.push("Bright or harsh: a lot of energy above 2 kHz. Lower hi-hats/cymbals or use an EQ high shelf cut.".into());
    }
    if mix.stereo_correlation > 0.97 && project.tracks.len() > 1 {
        out.push(
            "The mix is almost mono. Panning tracks apart, chorus, or reverb would widen it."
                .into(),
        );
    } else if mix.stereo_correlation < 0.0 {
        out.push("Stereo correlation is negative: parts may cancel out on mono speakers (phones, some TVs).".into());
    }
    if let Some(loudest) = tracks
        .iter()
        .max_by(|a, b| a.energy_share_percent.total_cmp(&b.energy_share_percent))
        && loudest.energy_share_percent > 75.0
        && tracks.len() > 1
    {
        out.push(format!(
            "\"{}\" makes up {:.0}% of the mix's energy and may be drowning out the others.",
            loudest.name, loudest.energy_share_percent
        ));
    }
    for t in tracks.iter().filter(|t| t.lufs.is_none()) {
        out.push(format!("\"{}\" is silent in this range.", t.name));
    }
    if out.is_empty() {
        out.push(format!(
            "Levels look healthy: {lufs:.1} LUFS with peaks at {:.1} dBTP.",
            mix.true_peak_dbtp
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_model::{Command, NoteInput, Session};

    fn song() -> Project {
        let mut s = Session::default();
        let notes = |pitch: u8| -> Vec<NoteInput> {
            (0..8)
                .map(|i| NoteInput {
                    pitch,
                    start_beats: f64::from(i) * 0.5,
                    length_beats: 0.45,
                    velocity: 110,
                    id: None,
                })
                .collect()
        };
        for (track_id, pitch) in [(1, 60), (2, 36), (3, 36)] {
            s.execute(Command::CreateClip {
                track_id,
                start_beats: 0.0,
                length_beats: 4.0,
                name: None,
                notes: notes(pitch),
            })
            .expect("clip");
        }
        s.project().clone()
    }

    #[test]
    fn analyzes_a_song_with_per_track_levels() {
        let a = analyze(&song(), &AnalyzeOptions::default());
        let lufs = a.mix.integrated_lufs.expect("not silent");
        assert!((-40.0..0.0).contains(&lufs), "{lufs}");
        assert_eq!(a.tracks.len(), 3);
        let total: f64 = a.tracks.iter().map(|t| t.energy_share_percent).sum();
        assert!((total - 100.0).abs() < 0.5, "{total}");
        assert!(a.tracks.iter().all(|t| t.lufs.is_some()));
        assert!(!a.hints.is_empty());
    }

    #[test]
    fn empty_song_is_reported_silent() {
        let a = analyze(&Project::default(), &AnalyzeOptions::default());
        assert!(a.mix.integrated_lufs.is_none());
        assert!(a.hints[0].contains("silent"));
    }

    #[test]
    fn muting_a_track_shows_in_the_hints() {
        let mut p = song();
        p.tracks[0].clips.clear();
        let a = analyze(&p, &AnalyzeOptions::default());
        assert!(
            a.hints.iter().any(|h| h.contains("\"Keys\" is silent")),
            "{:?}",
            a.hints
        );
    }

    #[test]
    fn spectrogram_is_a_png() {
        let a = analyze(
            &song(),
            &AnalyzeOptions {
                per_track: false,
                spectrogram: true,
                ..AnalyzeOptions::default()
            },
        );
        let png = a.spectrogram_png.expect("png");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }
}
