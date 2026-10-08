//! Getting music out of Nunc Pro Tune and into a game.
//!
//! [`export_to_godot`] renders the song (or its loop region, or named
//! sections) into a Godot project as seamless loops, optionally with one
//! stem per track, writes Godot's import settings so the files loop as
//! soon as Godot sees them, and can write adaptive-music resources:
//! `AudioStreamSynchronized` (stems as layers) and `AudioStreamInteractive`
//! (sections you switch between). Formats follow `docs/godot-formats.md`.

mod encode;
mod godot;
mod inspect;
mod render;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use daw_engine::AudioPool;
use daw_model::Project;
use serde::Serialize;
use thiserror::Error;

pub use encode::{write_ogg, write_wav};
pub use inspect::{Finding, Inspection, Level, inspect};
pub use render::{RenderedLoop, render_loop, render_with_intro};

/// Sample rate for game audio: Godot mixes at 44.1 kHz by default.
pub const GAME_SAMPLE_RATE_HZ: u32 = 44_100;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("{0} isn't a Godot project folder (it has no project.godot)")]
    NotGodotProject(PathBuf),
    #[error(
        "the folder inside the Godot project must be a relative path like music/boss, got \"{0}\""
    )]
    BadFolder(String),
    #[error("nothing to export: {0}")]
    Empty(String),
    #[error("could not write {path}: {message}")]
    Io { path: PathBuf, message: String },
    #[error("OGG encoding failed: {0}")]
    Encode(String),
}

pub(crate) fn io_err(path: &Path, e: impl ToString) -> ExportError {
    ExportError::Io {
        path: path.to_owned(),
        message: e.to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioFormat {
    /// OGG Vorbis: small files, Godot's preferred music format.
    #[default]
    Ogg,
    /// 16-bit WAV with a loop point.
    Wav,
}

impl AudioFormat {
    pub fn extension(self) -> &'static str {
        match self {
            AudioFormat::Ogg => "ogg",
            AudioFormat::Wav => "wav",
        }
    }
}

/// A named part of the song for interactive music (e.g. "explore", "combat").
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct Section {
    pub name: String,
    pub start_beats: f64,
    pub end_beats: f64,
}

/// What to export and where.
#[derive(Debug, Clone, PartialEq)]
pub struct GodotExport {
    /// The Godot project folder (the one with `project.godot`).
    pub project_dir: PathBuf,
    /// Folder inside the project, e.g. `music/boss` (created if needed).
    pub folder: String,
    /// Base file name, e.g. `boss_theme`.
    pub name: String,
    pub format: AudioFormat,
    /// Region to export; defaults to the loop region if looping is on,
    /// otherwise the whole song rounded to bars.
    pub start_beats: Option<f64>,
    pub end_beats: Option<f64>,
    /// Make a seamless loop (and tell Godot to loop it). Off: the region
    /// plays once and rings out.
    pub looped: bool,
    /// With `looped`: start the file at the song start and loop only the
    /// region, so everything before it plays once as an intro.
    pub intro: bool,
    /// Also export each track on its own.
    pub stems: bool,
    /// With stems: one per bus (the tracks playing into it, with what they
    /// send to other buses) instead of one per track; tracks that play
    /// straight into the master share an "other" stem.
    pub bus_stems: bool,
    /// With stems: an `AudioStreamSynchronized` that plays them together.
    pub layers_resource: bool,
    /// Sections for an `AudioStreamInteractive` (each exported as a loop).
    pub sections: Vec<Section>,
    /// Loudness to normalize to (LUFS), e.g. -16. None leaves levels as mixed.
    pub target_lufs: Option<f64>,
}

/// What was written.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportReport {
    /// Every file written, as `res://` paths.
    pub files: Vec<String>,
    /// Loudness of the main file after normalizing.
    pub integrated_lufs: Option<f64>,
    /// Gain applied to every file.
    pub gain_db: f64,
    pub seconds: f64,
    pub looped: bool,
    /// Seconds of intro before the loop starts (0 = loops from the start).
    pub loop_start_seconds: f64,
}

/// `Boss Theme!` → `boss_theme`.
pub fn file_slug(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let s = s
        .split('_')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if s.is_empty() { "music".into() } else { s }
}

/// Renders and writes everything `spec` asks for into the Godot project.
pub fn export_to_godot(
    project: &Project,
    audio: &Arc<AudioPool>,
    spec: &GodotExport,
) -> Result<ExportReport, ExportError> {
    if !spec.project_dir.join("project.godot").is_file() {
        return Err(ExportError::NotGodotProject(spec.project_dir.clone()));
    }
    let folder = godot::check_folder(&spec.folder)?;
    let dir = spec.project_dir.join(&folder);
    std::fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
    let base = file_slug(&spec.name);

    let (start, end) = render::default_region(project, spec.start_beats, spec.end_beats)?;
    // An intro plays from the song start up to the loop.
    let from = if spec.intro && spec.looped {
        0.0
    } else {
        start
    };
    let main = render::render_with_intro(project, audio, from, start, end, spec.looped, None);
    let gain_db = render::normalize_gain(&main, spec.target_lufs);

    let mut files = Vec::new();
    let mut write = |stem_name: &str, rendered: &RenderedLoop| -> Result<String, ExportError> {
        let file = format!("{stem_name}.{}", spec.format.extension());
        let path = dir.join(&file);
        let samples = render::apply_gain(&rendered.stereo, gain_db);
        match spec.format {
            AudioFormat::Ogg => write_ogg(&path, &samples, GAME_SAMPLE_RATE_HZ)?,
            AudioFormat::Wav => write_wav(
                &path,
                &samples,
                GAME_SAMPLE_RATE_HZ,
                rendered
                    .looped
                    .then_some((rendered.loop_start_frame(), samples.len() / 2)),
            )?,
        }
        godot::write_import(&path, spec.format, rendered, project)?;
        let res = godot::res_path(&folder, &file);
        files.push(res.clone());
        Ok(res)
    };

    write(&base, &main)?;
    let mut stem_paths = Vec::new();
    if spec.stems && spec.bus_stems && !project.buses.is_empty() {
        let mut groups: Vec<(String, Vec<daw_model::TrackId>)> = project
            .buses
            .iter()
            .map(|b| {
                let ids = project
                    .tracks
                    .iter()
                    .filter(|t| t.output == Some(b.id))
                    .map(|t| t.id)
                    .collect();
                (b.name.clone(), ids)
            })
            .collect();
        groups.push((
            "other".into(),
            project
                .tracks
                .iter()
                .filter(|t| t.output.is_none())
                .map(|t| t.id)
                .collect(),
        ));
        for (name, ids) in groups {
            if ids.is_empty() {
                continue;
            }
            let mut only = project.clone();
            for t in &mut only.tracks {
                t.mixer.solo = false;
                t.mixer.mute = t.mixer.mute || !ids.contains(&t.id);
            }
            let stem = render::render_with_intro(&only, audio, from, start, end, spec.looped, None);
            if stem.is_silent() {
                continue;
            }
            let res = write(&format!("{base}_{}", file_slug(&name)), &stem)?;
            stem_paths.push(res);
        }
    } else if spec.stems {
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
                continue;
            }
            let res = write(&format!("{base}_{}", file_slug(&t.name)), &stem)?;
            stem_paths.push(res);
        }
    }
    let mut section_paths = Vec::new();
    for s in &spec.sections {
        if s.end_beats <= s.start_beats {
            return Err(ExportError::Empty(format!(
                "section \"{}\" ends before it starts",
                s.name
            )));
        }
        let rendered = render_loop(project, audio, s.start_beats, s.end_beats, true, None);
        let res = write(&format!("{base}_{}", file_slug(&s.name)), &rendered)?;
        section_paths.push((s.name.clone(), res));
    }
    if spec.stems && spec.layers_resource && !stem_paths.is_empty() {
        let path = dir.join(format!("{base}_layers.tres"));
        godot::write_synchronized(&path, &stem_paths)?;
        files.push(godot::res_path(&folder, &format!("{base}_layers.tres")));
    }
    if !section_paths.is_empty() {
        let path = dir.join(format!("{base}_sections.tres"));
        godot::write_interactive(&path, &section_paths)?;
        files.push(godot::res_path(&folder, &format!("{base}_sections.tres")));
    }
    let lufs = daw_analysis::measure(
        &render::apply_gain(&main.stereo, gain_db),
        GAME_SAMPLE_RATE_HZ,
    )
    .integrated_lufs;
    Ok(ExportReport {
        files,
        integrated_lufs: lufs.filter(|l| l.is_finite()),
        gain_db,
        seconds: main.seconds(),
        looped: main.looped,
        loop_start_seconds: main.loop_start_seconds(),
    })
}
