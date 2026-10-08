//! Checks an export before it's written: renders exactly what
//! `export_to_godot` would and looks for problems a game would show (a click
//! at the loop point, clipping, silence, missing recordings, stems of
//! different lengths, loops Godot can't beat-sync).

use std::sync::Arc;

use daw_engine::AudioPool;
use daw_model::Project;
use serde::Serialize;

use crate::render::{self, RenderedLoop};
use crate::{ExportError, GAME_SAMPLE_RATE_HZ, GodotExport};

/// How serious a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Will sound wrong in the game.
    Problem,
    /// Worth a listen.
    Warning,
    /// Checked and fine.
    Ok,
}

/// One finding, in plain words.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    pub level: Level,
    pub message: String,
}

/// Everything the check found.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Inspection {
    pub findings: Vec<Finding>,
    /// Loudness of the main file after normalizing (LUFS).
    pub integrated_lufs: Option<f64>,
    /// Highest sample after normalizing (dBFS).
    pub peak_dbfs: f64,
    pub seconds: f64,
}

impl Inspection {
    /// No problems (warnings allowed).
    pub fn ready(&self) -> bool {
        !self.findings.iter().any(|f| f.level == Level::Problem)
    }
}

fn db(x: f32) -> f64 {
    20.0 * f64::from(x.max(1e-9)).log10()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// How much the wave jumps where the file wraps from its last sample to
/// the loop start, compared with how much it moves from sample to sample
/// nearby. Returns (jump, typical step).
fn seam_jump(stereo: &[f32], loop_start_frame: usize) -> (f32, f32) {
    let frames = stereo.len() / 2;
    if frames < 64 || loop_start_frame + 32 >= frames {
        return (0.0, 0.0);
    }
    let mut jump = 0.0f32;
    let mut steps = Vec::new();
    for ch in 0..2 {
        let s = |f: usize| stereo[f * 2 + ch];
        jump = jump.max((s(loop_start_frame) - s(frames - 1)).abs());
        for f in frames - 32..frames - 1 {
            steps.push((s(f + 1) - s(f)).abs());
        }
        for f in loop_start_frame..loop_start_frame + 31 {
            steps.push((s(f + 1) - s(f)).abs());
        }
    }
    steps.sort_by(f32::total_cmp);
    (jump, steps[steps.len() * 9 / 10])
}

/// Renders what `spec` would export and reports what's wrong with it.
pub fn inspect(
    project: &Project,
    audio: &Arc<AudioPool>,
    spec: &GodotExport,
) -> Result<Inspection, ExportError> {
    let mut findings = Vec::new();
    let mut say = |level, message: String| findings.push(Finding { level, message });

    // Recordings that can't be found play as silence.
    let mut missing: Vec<&str> = project
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .filter_map(|c| c.audio.as_ref().map(|a| a.file.as_str()))
        .filter(|f| !audio.has(f))
        .collect();
    missing.sort_unstable();
    missing.dedup();
    if missing.is_empty() {
        say(Level::Ok, "Every recording the song uses was found.".into());
    } else {
        say(
            Level::Problem,
            format!(
                "{} recording(s) are missing and would be silent: {}",
                missing.len(),
                missing.join(", ")
            ),
        );
    }

    let (start, end) = render::default_region(project, spec.start_beats, spec.end_beats)?;
    let from = if spec.intro && spec.looped {
        0.0
    } else {
        start
    };
    let main = render::render_with_intro(project, audio, from, start, end, spec.looped, None);
    let gain_db = render::normalize_gain(&main, spec.target_lufs);
    let samples = render::apply_gain(&main.stereo, gain_db);
    let raw_peak = main.stereo.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let peak = db(raw_peak) + gain_db;
    let measured = daw_analysis::measure(&samples, GAME_SAMPLE_RATE_HZ);
    let lufs = measured.integrated_lufs.filter(|l| l.is_finite());

    if main.is_silent() {
        say(
            Level::Problem,
            "The export would be silent: nothing plays in this part of the song.".into(),
        );
    }
    if peak > -0.1 {
        say(
            Level::Problem,
            format!(
                "It clips: peaks reach {peak:.1} dBFS. Turn the master down or add a Limiter on it."
            ),
        );
    } else if peak > -1.0 {
        say(
            Level::Warning,
            format!(
                "Peaks reach {peak:.1} dBFS; some devices distort above -1 dB. A Limiter on the master set to -1 dB fixes it."
            ),
        );
    } else {
        say(Level::Ok, format!("No clipping (peaks {peak:.1} dBFS)."));
    }
    if let (Some(l), None) = (lufs, spec.target_lufs)
        && !(-23.0..=-12.0).contains(&l)
    {
        say(
            Level::Warning,
            format!(
                "Loudness is {l:.1} LUFS; game music usually sits around -16 to -20. Turn on the loudness setting to match it."
            ),
        );
    }

    if spec.looped {
        check_loop(&mut say, &main, &samples, project, "The loop");
    }

    if spec.stems {
        let mut silent = Vec::new();
        let mut lengths_ok = true;
        for t in &project.tracks {
            let stem = render::render_with_intro(
                project,
                audio,
                from,
                start,
                end,
                spec.looped,
                Some(t.id),
            );
            if stem.is_silent() {
                silent.push(t.name.as_str());
            } else if stem.stereo.len() != main.stereo.len() {
                lengths_ok = false;
            }
        }
        if lengths_ok {
            say(
                Level::Ok,
                "Every stem is exactly as long as the song file, so layers stay in sync.".into(),
            );
        } else {
            say(
                Level::Problem,
                "Some stems are a different length from the song file; layers would drift apart."
                    .into(),
            );
        }
        if !silent.is_empty() {
            say(
                Level::Warning,
                format!(
                    "These tracks are silent here and get no stem: {}",
                    silent.join(", ")
                ),
            );
        }
    }

    for s in &spec.sections {
        if s.end_beats <= s.start_beats {
            say(
                Level::Problem,
                format!("Section \"{}\" ends before it starts.", s.name),
            );
            continue;
        }
        let rendered = render::render_loop(project, audio, s.start_beats, s.end_beats, true, None);
        if rendered.is_silent() {
            say(Level::Warning, format!("Section \"{}\" is silent.", s.name));
        }
        let samples = render::apply_gain(&rendered.stereo, gain_db);
        check_loop(
            &mut say,
            &rendered,
            &samples,
            project,
            &format!("Section \"{}\"", s.name),
        );
    }

    Ok(Inspection {
        findings,
        integrated_lufs: lufs,
        peak_dbfs: peak,
        seconds: main.seconds(),
    })
}

/// Seam and beat-sync checks for one looping file.
fn check_loop(
    say: &mut impl FnMut(Level, String),
    rendered: &RenderedLoop,
    samples: &[f32],
    project: &Project,
    what: &str,
) {
    let loop_beats = rendered.beats - rendered.intro_beats;
    let bpb = project.beats_per_bar();
    if (loop_beats - loop_beats.round()).abs() > 1e-6 {
        say(
            Level::Warning,
            format!(
                "{what} is {loop_beats:.2} beats long, not a whole number of beats, so Godot can't keep it in time for beat-synced switching."
            ),
        );
    } else if (loop_beats / bpb - (loop_beats / bpb).round()).abs() > 1e-6 {
        say(
            Level::Warning,
            format!(
                "{what} isn't a whole number of bars; it may feel like it restarts early or late."
            ),
        );
    }
    let start_frame = rendered.loop_start_frame();
    let (jump, step) = seam_jump(samples, start_frame);
    if jump > 0.05 && jump > 8.0 * step.max(1e-4) {
        say(
            Level::Problem,
            format!(
                "{what} clicks where it repeats: the sound jumps by {:.0}% at the loop point. Shorten or fade a note that rings across the end.",
                f64::from(jump) * 100.0
            ),
        );
    } else {
        say(
            Level::Ok,
            format!("{what} joins smoothly where it repeats."),
        );
    }
    // A big level change at the join is audible even without a click.
    let window = (GAME_SAMPLE_RATE_HZ as usize / 20) * 2;
    if samples.len() > 4 * window && start_frame * 2 + window < samples.len() {
        let tail = rms(&samples[samples.len() - window..]);
        let head = rms(&samples[start_frame * 2..start_frame * 2 + window]);
        let change = db(head) - db(tail);
        if tail.max(head) > 0.01 && change.abs() > 9.0 {
            say(
                Level::Warning,
                format!(
                    "{what} gets {:.0} dB {} where it repeats; listen to the loop point.",
                    change.abs(),
                    if change > 0.0 { "louder" } else { "quieter" }
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seam_jump_stands_out_from_normal_motion() {
        // A slow sine that wraps cleanly...
        let n = 4_800;
        let smooth: Vec<f32> = (0..n)
            .flat_map(|i| {
                let v = (std::f32::consts::TAU * i as f32 / n as f32).sin() * 0.5;
                [v, v]
            })
            .collect();
        let (jump, step) = seam_jump(&smooth, 0);
        assert!(jump < 8.0 * step + 0.01, "{jump} vs {step}");
        // ...and one cut off at its peak.
        let cut: Vec<f32> = smooth[..smooth.len() * 5 / 8].to_vec();
        let (jump, step) = seam_jump(&cut, 0);
        assert!(jump > 0.3 && jump > 8.0 * step, "{jump} vs {step}");
    }
}
