//! Game sound effects into a Godot project: one WAV per sound.

use std::path::PathBuf;

use daw_model::{Id, Project};
use serde::Serialize;

use crate::{ExportError, GAME_SAMPLE_RATE_HZ, file_slug, godot, io_err, write_wav};

/// Loudest a written sound may peak, in dBFS.
const MAX_PEAK_DBFS: f64 = -1.0;

/// Which sounds to write, and where.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundExport {
    /// The Godot project folder (the one with `project.godot`).
    pub project_dir: PathBuf,
    /// Folder inside the project, e.g. `sfx` (created if needed).
    pub folder: String,
    /// The sounds to write; None writes them all.
    pub sound_ids: Option<Vec<Id>>,
}

/// One file written.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SoundFile {
    pub sound_id: Id,
    /// Where Godot finds it, e.g. `res://sfx/coin.wav`.
    pub res_path: String,
    pub seconds: f64,
    /// The written file's peak.
    pub peak_dbfs: f64,
    /// How much it was turned down so it doesn't clip (0 = not at all).
    pub turned_down_db: f64,
}

/// What was written.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SoundExportReport {
    pub files: Vec<SoundFile>,
}

/// A file name for `name`, numbered if an earlier sound already took it.
fn file_stem(name: &str, used: &[String]) -> String {
    let base = if name.chars().any(|c| c.is_ascii_alphanumeric()) {
        file_slug(name)
    } else {
        "sound".to_owned()
    };
    let mut stem = base.clone();
    let mut n = 2;
    while used.contains(&stem) {
        stem = format!("{base}_{n}");
        n += 1;
    }
    stem
}

/// Renders the chosen sounds and writes each as a 16-bit, 44.1 kHz WAV
/// into the Godot project. Sounds that would peak above -1 dBFS are turned
/// down to it. Godot's default WAV import (no loop) suits effects, so no
/// `.import` file is written.
pub fn export_sounds(
    project: &Project,
    spec: &SoundExport,
) -> Result<SoundExportReport, ExportError> {
    if !spec.project_dir.join("project.godot").is_file() {
        return Err(ExportError::NotGodotProject(spec.project_dir.clone()));
    }
    let folder = godot::check_folder(&spec.folder)?;
    let chosen: Vec<&daw_model::SoundEffect> = match &spec.sound_ids {
        None => project.sounds.iter().collect(),
        Some(ids) => ids
            .iter()
            .map(|&id| {
                project
                    .sound(id)
                    .ok_or_else(|| ExportError::Empty(format!("there is no sound with id {id}")))
            })
            .collect::<Result<_, _>>()?,
    };
    if chosen.is_empty() {
        return Err(ExportError::Empty(
            "the project has no sound effects yet".into(),
        ));
    }
    let dir = spec.project_dir.join(&folder);
    std::fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;

    // Stems already taken, so two names that slug alike don't overwrite.
    let mut used: Vec<String> = Vec::new();
    let mut files = Vec::new();
    for sound in chosen {
        let mono = daw_sfx::render(&sound.recipe, GAME_SAMPLE_RATE_HZ);
        let peak = f64::from(daw_sfx::peak(&mono));
        let peak_db = if peak > 0.0 {
            20.0 * peak.log10()
        } else {
            f64::NEG_INFINITY
        };
        let turned_down_db = (peak_db - MAX_PEAK_DBFS).max(0.0);
        let gain = 10f64.powf(-turned_down_db / 20.0) as f32;
        let stereo: Vec<f32> = mono.iter().flat_map(|&s| [s * gain, s * gain]).collect();
        let stem = file_stem(&sound.name, &used);
        let file = format!("{stem}.wav");
        used.push(stem);
        write_wav(&dir.join(&file), &stereo, GAME_SAMPLE_RATE_HZ, None)?;
        files.push(SoundFile {
            sound_id: sound.id,
            res_path: godot::res_path(&folder, &file),
            seconds: mono.len() as f64 / f64::from(GAME_SAMPLE_RATE_HZ),
            peak_dbfs: (peak_db - turned_down_db).max(-120.0),
            turned_down_db,
        });
    }
    Ok(SoundExportReport { files })
}
