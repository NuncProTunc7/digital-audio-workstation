use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use daw_engine::{AudioPool, offline::render_bounded_region};
use daw_model::Project;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::encode::{encode_ogg, wav_bytes};
use crate::inspect::{check_seam, db, rms};
use crate::render::{TAIL_SECONDS, apply_gain, fold_tail, normalize_gain_for_samples};
use crate::{AudioFormat, ExportError, Finding, GAME_SAMPLE_RATE_HZ, Level, io_err};

/// How a game will play the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AudioExportMode {
    Loop,
    OneShot,
    Playlist,
}

/// An explicit selection on the arrangement timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AudioRegion {
    Marker { name: String },
    Beats { start_beats: f64, end_beats: f64 },
    FullArrangement,
}

/// Options shared by direct export and its dry run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioExport {
    /// File path, including an extension matching the format. Parent must exist.
    pub path: PathBuf,
    pub format: AudioFormat,
    pub mode: AudioExportMode,
    pub region: AudioRegion,
    /// For one_shot only. Defaults to zero, maximum 30 seconds.
    #[schemars(range(min = 0, max = 30))]
    pub tail_seconds: Option<f64>,
    /// Omit to preserve the mix level.
    #[schemars(range(min = -70, max = 0))]
    pub target_lufs: Option<f64>,
    /// Replace an existing file only if explicitly enabled.
    #[serde(default)]
    pub overwrite: bool,
}

/// Which part of the arrangement to audition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PreviewSelection {
    FirstEightBars,
    Region {
        region: AudioRegion,
    },
    /// Use the given region or the enabled arrangement loop.
    LoopSeam {
        region: Option<AudioRegion>,
    },
}

/// A playable, measured excerpt at the current mix level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioPreview {
    pub path: PathBuf,
    pub format: AudioFormat,
    pub selection: PreviewSelection,
    #[serde(default)]
    pub overwrite: bool,
}

/// Measurements of the render, or of the decoded file after export.
#[derive(Debug, Serialize)]
pub struct AudioExportReport {
    pub path: PathBuf,
    pub format: AudioFormat,
    pub mode: AudioExportMode,
    pub sample_rate_hz: u32,
    pub channels: usize,
    pub duration_seconds: f64,
    pub start_beats: f64,
    pub end_beats: f64,
    pub integrated_lufs: Option<f64>,
    pub peak_dbfs: f64,
    pub true_peak_dbtp: f64,
    pub gain_db: f64,
    pub loop_start_samples: Option<usize>,
    /// Exclusive. Both loop positions count samples per channel (frames).
    pub loop_end_samples: Option<usize>,
    /// Sample offset per channel of the loop join inside a seam preview.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_seam_samples: Option<usize>,
    pub opening_rms_dbfs: Option<f64>,
    pub ending_rms_dbfs: Option<f64>,
    pub verified_encoded_file: bool,
    pub findings: Vec<Finding>,
}

fn invalid(message: impl Into<String>) -> ExportError {
    ExportError::InvalidAudioExport(message.into())
}

fn destination(spec: &AudioExport) -> Result<PathBuf, ExportError> {
    if !spec
        .path
        .extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| x.eq_ignore_ascii_case(spec.format.extension()))
    {
        return Err(invalid(
            "The destination extension must match the audio format.",
        ));
    }
    let parent = spec
        .path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = std::path::absolute(parent).map_err(|e| io_err(parent, e))?;
    if !std::fs::metadata(&parent)
        .map_err(|e| io_err(&parent, e))?
        .is_dir()
    {
        return Err(invalid("Choose an existing destination folder."));
    }
    let path = parent.join(
        spec.path
            .file_name()
            .ok_or_else(|| invalid("Give a file name."))?,
    );
    match std::fs::symlink_metadata(&path) {
        Ok(m) if !m.is_file() || m.permissions().readonly() => {
            Err(invalid("The destination must be a writable regular file."))
        }
        Ok(_) if !spec.overwrite => Err(invalid(
            "The destination exists. Use overwrite to replace it.",
        )),
        Ok(_) => Ok(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(path),
        Err(e) => Err(io_err(&path, e)),
    }
}

fn region_beats(project: &Project, region: &AudioRegion) -> Result<(f64, f64), ExportError> {
    let (start, end) = match region {
        AudioRegion::FullArrangement => (0.0, project.end_beats()),
        AudioRegion::Beats {
            start_beats,
            end_beats,
        } => (*start_beats, *end_beats),
        AudioRegion::Marker { name } => {
            let sections = project.sections();
            let mut matches = sections
                .iter()
                .filter(|s| s.name.eq_ignore_ascii_case(name));
            let section = matches.next().ok_or_else(|| {
                invalid(format!(
                    "No marker named {name:?} has an exportable section."
                ))
            })?;
            if matches.next().is_some() {
                return Err(invalid(format!(
                    "More than one marker is named {name:?}. Give the start and end beats."
                )));
            }
            (section.start_beats, section.end_beats)
        }
    };
    if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
        return Err(invalid(
            "Choose a non-empty region with a nonnegative start beat.",
        ));
    }
    Ok((start, end))
}

fn validate_render(
    project: &Project,
    spec: &AudioExport,
    start: f64,
    end: f64,
) -> Result<(f64, usize), ExportError> {
    if spec
        .target_lufs
        .is_some_and(|v| !(-70.0..=0.0).contains(&v))
    {
        return Err(invalid("target_lufs must be between -70 and 0."));
    }
    let tail = match (spec.mode, spec.tail_seconds) {
        (AudioExportMode::Loop, None) => TAIL_SECONDS,
        (AudioExportMode::Playlist, None) => 0.0,
        (AudioExportMode::OneShot, None) => 0.0,
        (AudioExportMode::OneShot, Some(v)) if (0.0..=30.0).contains(&v) => v,
        _ => {
            return Err(invalid(
                "tail_seconds is available for one_shot and must be between 0 and 30.",
            ));
        }
    };
    let seconds = project.tempo_map().seconds_between(start, end);
    let frames = (seconds * f64::from(GAME_SAMPLE_RATE_HZ)).round() as usize;
    if frames == 0 || !(0.0..=3600.0).contains(&(seconds + tail)) {
        return Err(invalid(
            "Choose an audio region between one sample and one hour, including its render tail.",
        ));
    }
    Ok((tail, frames))
}

/// Checks the selected render and destination without writing a file.
pub fn inspect_audio_export(
    project: &Project,
    audio: &Arc<AudioPool>,
    spec: &AudioExport,
) -> Result<AudioExportReport, ExportError> {
    run(project, audio, spec, false, false)
}

/// Renders, verifies, and publishes the file in one filesystem operation.
pub fn export_audio(
    project: &Project,
    audio: &Arc<AudioPool>,
    spec: &AudioExport,
) -> Result<AudioExportReport, ExportError> {
    run(project, audio, spec, true, false)
}

/// Renders a short excerpt or the end and beginning of a loop joined together.
pub fn preview_audio(
    project: &Project,
    audio: &Arc<AudioPool>,
    preview: &AudioPreview,
) -> Result<AudioExportReport, ExportError> {
    let seam = matches!(preview.selection, PreviewSelection::LoopSeam { .. });
    let selection = match &preview.selection {
        PreviewSelection::FirstEightBars => AudioRegion::Beats {
            start_beats: 0.0,
            end_beats: (project.beats_per_bar() * 8.0).min(project.end_beats()),
        },
        PreviewSelection::Region { region }
        | PreviewSelection::LoopSeam {
            region: Some(region),
        } => region.clone(),
        PreviewSelection::LoopSeam { region: None } => {
            if !project.loop_region.enabled {
                return Err(invalid(
                    "Enable a loop in the arrangement or specify a region for the seam preview.",
                ));
            }
            AudioRegion::Beats {
                start_beats: project.loop_region.start_beats,
                end_beats: project.loop_region.end_beats,
            }
        }
    };
    let mut spec = AudioExport {
        path: preview.path.clone(),
        format: preview.format,
        mode: if seam {
            AudioExportMode::Loop
        } else {
            AudioExportMode::OneShot
        },
        region: selection,
        target_lufs: None,
        tail_seconds: None,
        overwrite: preview.overwrite,
    };
    if !seam {
        let (start, end) = region_beats(project, &spec.region)?;
        let tempo = project.tempo_map();
        let end = end.min(tempo.beats_at(tempo.seconds_at(start) + 30.0));
        spec.region = AudioRegion::Beats {
            start_beats: start,
            end_beats: end,
        };
    }
    run(project, audio, &spec, true, seam)
}

fn run(
    project: &Project,
    audio: &Arc<AudioPool>,
    spec: &AudioExport,
    write: bool,
    preview_seam: bool,
) -> Result<AudioExportReport, ExportError> {
    let path = destination(spec)?;
    let (start, end) = region_beats(project, &spec.region)?;
    let (tail, frames) = validate_render(project, spec, start, end)?;
    let mut temp = write
        .then(|| {
            tempfile::Builder::new()
                .prefix(".npt-export-")
                .suffix(&format!(".{}", spec.format.extension()))
                .tempfile_in(path.parent().expect("resolved parent"))
        })
        .transpose()
        .map_err(|e| io_err(&path, e))?;
    let looped = spec.mode == AudioExportMode::Loop;
    let mut stereo = render_bounded_region(project, audio, start, end, tail, GAME_SAMPLE_RATE_HZ);
    if looped {
        fold_tail(&mut stereo, 0, frames);
    }
    let preview_seam_samples = preview_seam.then(|| {
        let side = frames.min(GAME_SAMPLE_RATE_HZ as usize * 2);
        let excerpt: Vec<_> = stereo[(frames - side) * 2..]
            .iter()
            .chain(&stereo[..side * 2])
            .copied()
            .collect();
        stereo = excerpt;
        side
    });
    let gain_db = normalize_gain_for_samples(&stereo, spec.target_lufs);
    let mut findings = Vec::new();
    let mut missing: Vec<_> = project
        .tracks
        .iter()
        .filter(|track| crate::heard_in_mix(project, track))
        .flat_map(|track| &track.clips)
        .filter(|clip| {
            !clip.muted && clip.start_beats < end && clip.start_beats + clip.length_beats > start
        })
        .filter_map(|clip| clip.audio.as_ref())
        .map(|region| region.file.as_str())
        .filter(|file| !audio.has(file))
        .collect();
    missing.sort_unstable();
    missing.dedup();
    if !missing.is_empty() {
        findings.push(Finding {
            level: Level::Problem,
            message: format!(
                "These recordings are missing from the selected region: {}",
                missing.join(", ")
            ),
        });
    }
    let raw_peak = db(stereo.iter().fold(0.0f32, |m, s| m.max(s.abs()))) + gain_db;
    if raw_peak > 0.0 {
        findings.push(Finding {
            level: Level::Problem,
            message:
                "The mix exceeds full scale and will clip. Turn it down or set a loudness target."
                    .into(),
        });
    }
    let mut stereo = apply_gain(&stereo, gain_db);
    if let Some(file) = &mut temp {
        let staged = file.path().to_owned();
        match spec.format {
            AudioFormat::Ogg => {
                encode_ogg(file.as_file_mut(), &path, &stereo, GAME_SAMPLE_RATE_HZ)?
            }
            AudioFormat::Wav => file
                .as_file_mut()
                .write_all(&wav_bytes(&stereo, GAME_SAMPLE_RATE_HZ, None, None))
                .map_err(|e| io_err(&path, e))?,
        }
        file.as_file().sync_all().map_err(|e| io_err(&path, e))?;
        let decoded = daw_audio::decode_file(&staged)
            .map_err(|e| invalid(format!("Could not verify the encoded audio: {e}")))?;
        if decoded.sample_rate_hz != GAME_SAMPLE_RATE_HZ
            || decoded.channels.len() != 2
            || decoded.frames() != stereo.len() / 2
        {
            return Err(invalid(format!(
                "The encoded file has {} frames at {} Hz with {} channels. Expected {} frames at {GAME_SAMPLE_RATE_HZ} Hz with two channels.",
                decoded.frames(),
                decoded.sample_rate_hz,
                decoded.channels.len(),
                stereo.len() / 2
            )));
        }
        for ((frame, left), right) in stereo
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(&decoded.channels[0])
            .zip(&decoded.channels[1])
        {
            *frame = [*left, *right];
        }
    }
    let measured = daw_analysis::measure(&stereo, GAME_SAMPLE_RATE_HZ);
    if measured.sample_peak_dbfs < -100.0 {
        findings.push(Finding {
            level: Level::Problem,
            message: "The selected region is silent.".into(),
        });
    }
    if measured.true_peak_dbtp > -1.0 {
        findings.push(Finding {
            level: Level::Warning,
            message: format!(
                "True peak is {:.1} dBTP. Listen for distortion on playback.",
                measured.true_peak_dbtp
            ),
        });
    }
    if looped {
        // The check compares the end to the beginning. For the preview the
        // join is in the middle, so measure it in that order after encoding.
        if let Some(seam_at) = preview_seam_samples {
            stereo.rotate_left(seam_at * 2);
        }
        check_seam(
            &mut |level, message| findings.push(Finding { level, message }),
            &stereo,
            0,
            "The loop",
        );
    }
    let (opening_rms_dbfs, ending_rms_dbfs) = if spec.mode == AudioExportMode::Playlist {
        let window = (GAME_SAMPLE_RATE_HZ as usize / 20 * 2).min(stereo.len());
        let opening = db(rms(&stereo[..window]));
        let ending = db(rms(&stereo[stereo.len() - window..]));
        if opening > -25.0 || ending > -25.0 {
            findings.push(Finding { level: Level::Warning, message: "The track starts or ends at an audible level. Listen to its transition in the playlist.".into() });
        }
        (Some(opening), Some(ending))
    } else {
        (None, None)
    };
    if let Some(file) = temp {
        let result = if spec.overwrite {
            file.persist(&path)
        } else {
            file.persist_noclobber(&path)
        };
        result.map_err(|e| io_err(&path, e.error))?;
    }
    Ok(AudioExportReport {
        path,
        format: spec.format,
        mode: if preview_seam {
            AudioExportMode::OneShot
        } else {
            spec.mode
        },
        sample_rate_hz: GAME_SAMPLE_RATE_HZ,
        channels: 2,
        duration_seconds: measured.duration_s,
        start_beats: start,
        end_beats: end,
        integrated_lufs: measured.integrated_lufs,
        peak_dbfs: measured.sample_peak_dbfs,
        true_peak_dbtp: measured.true_peak_dbtp,
        gain_db,
        loop_start_samples: (looped && !preview_seam).then_some(0),
        loop_end_samples: (looped && !preview_seam).then_some(frames),
        preview_seam_samples,
        opening_rms_dbfs,
        ending_rms_dbfs,
        verified_encoded_file: write,
        findings,
    })
}
