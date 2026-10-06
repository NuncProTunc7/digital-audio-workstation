//! Saving and loading project files (`.nptune`, JSON).

use std::path::Path;

use thiserror::Error;

use crate::command::{CommandError, check_value};
use crate::project::{FORMAT_VERSION, MAX_TRACKS, Project};

/// File extension for project files.
pub const PROJECT_EXTENSION: &str = "nptune";

#[derive(Debug, Error)]
pub enum FileError {
    #[error("could not read or write the file: {0}")]
    Io(String),
    #[error("this is not a valid Nunc Pro Tune project: {0}")]
    Parse(String),
    #[error("this project was made with a newer version of Nunc Pro Tune (format {0})")]
    TooNew(u32),
    #[error("the project file is damaged: {0}")]
    Invalid(String),
}

impl From<CommandError> for FileError {
    fn from(e: CommandError) -> Self {
        FileError::Invalid(e.to_string())
    }
}

/// Pretty-printed JSON, so files are readable and diff well.
pub fn project_to_json(project: &Project) -> String {
    let mut p = project.clone();
    p.format_version = FORMAT_VERSION;
    serde_json::to_string_pretty(&p).unwrap_or_default() + "\n"
}

/// Parses and validates a project, filling in defaults for anything missing.
pub fn project_from_json(json: &str) -> Result<Project, FileError> {
    let raw: serde_json::Value =
        serde_json::from_str(json).map_err(|e| FileError::Parse(e.to_string()))?;
    let version = raw
        .get("format_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::from(FORMAT_VERSION));
    if version > u64::from(FORMAT_VERSION) {
        return Err(FileError::TooNew(
            u32::try_from(version).unwrap_or(u32::MAX),
        ));
    }
    let mut project: Project =
        serde_json::from_value(raw).map_err(|e| FileError::Parse(e.to_string()))?;
    validate(&mut project)?;
    project.format_version = FORMAT_VERSION;
    Ok(project)
}

fn validate(project: &mut Project) -> Result<(), FileError> {
    use crate::command::Command;
    // Reuse the Commands' own checks for tempo and meter.
    let mut probe = Project::default();
    Command::SetTempo {
        bpm: project.tempo_bpm,
    }
    .apply(&mut probe)?;
    Command::SetTimeSignature {
        numerator: project.time_signature.numerator,
        denominator: project.time_signature.denominator,
    }
    .apply(&mut probe)?;
    if project.tracks.len() > MAX_TRACKS {
        return Err(FileError::Invalid(format!(
            "it has more than {MAX_TRACKS} tracks"
        )));
    }

    let mut ids = project.all_ids();
    ids.sort_unstable();
    if let Some(w) = ids.windows(2).find(|w| w[0] == w[1]) {
        return Err(FileError::Invalid(format!("id {} is used twice", w[0])));
    }
    if ids.first() == Some(&0) {
        return Err(FileError::Invalid("id 0 is not allowed".into()));
    }
    if let Some(&max) = ids.last() {
        project.reserve_id(max);
    }

    for track in &mut project.tracks {
        track.instrument = crate::command::complete_instrument_pub(track.instrument.clone())?;
        for lane in &mut track.automation {
            lane.points
                .retain(|p| p.beats.is_finite() && p.value.is_finite());
            lane.points.sort_by(|a, b| a.beats.total_cmp(&b.beats));
        }
        for clip in &track.clips {
            crate::command::check_clip_fits_pub(track.instrument.kind, clip)?;
            for n in &clip.notes {
                crate::command::validate_note_pub(n)?;
            }
        }
    }
    let chains = project
        .tracks
        .iter_mut()
        .map(|t| &mut t.mixer.effects)
        .chain(std::iter::once(&mut project.master.effects));
    for chain in chains {
        for effect in chain.iter_mut() {
            for (id, value) in &effect.params {
                let spec = crate::effect::effect_spec(effect.kind, id).ok_or_else(|| {
                    FileError::Invalid(format!("{:?} has no parameter {id}", effect.kind))
                })?;
                check_value(spec, *value)?;
            }
            for spec in crate::effect::effect_params(effect.kind) {
                effect
                    .params
                    .entry(spec.id.to_owned())
                    .or_insert(spec.default);
            }
        }
    }
    Ok(())
}

/// Writes the project, replacing the file only once the new copy is safely
/// on disk (so a crash mid-save can't destroy the old file).
pub fn save_project(project: &Project, path: &Path) -> Result<(), FileError> {
    let json = project_to_json(project);
    let tmp = path.with_extension(format!("{PROJECT_EXTENSION}.tmp"));
    std::fs::write(&tmp, json).map_err(|e| FileError::Io(e.to_string()))?;
    std::fs::rename(&tmp, path).map_err(|e| FileError::Io(e.to_string()))
}

pub fn load_project(path: &Path) -> Result<Project, FileError> {
    let json = std::fs::read_to_string(path).map_err(|e| FileError::Io(e.to_string()))?;
    project_from_json(&json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, EffectKind, NoteInput, Session};

    fn busy_project() -> Project {
        let mut s = Session::default();
        s.execute(Command::CreateClip {
            track_id: 1,
            start_beats: 4.0,
            length_beats: 8.0,
            name: None,
            notes: vec![NoteInput {
                pitch: 60,
                start_beats: 0.0,
                length_beats: 1.0,
                velocity: 90,
                id: None,
            }],
        })
        .expect("clip");
        s.execute(Command::AddEffect {
            track_id: Some(1),
            kind: EffectKind::Reverb,
            index: None,
        })
        .expect("fx");
        s.execute(Command::AddEffect {
            track_id: None,
            kind: EffectKind::Limiter,
            index: None,
        })
        .expect("master fx");
        s.project().clone()
    }

    #[test]
    fn round_trips_through_json() {
        let p = busy_project();
        let back = project_from_json(&project_to_json(&p)).expect("load");
        assert_eq!(p, back);
    }

    #[test]
    fn round_trips_through_a_file() {
        let dir = std::env::temp_dir().join(format!("npt-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("song.nptune");
        let p = busy_project();
        save_project(&p, &path).expect("save");
        assert_eq!(load_project(&path).expect("load"), p);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn loads_phase_1_style_files_without_new_fields() {
        let mut v = serde_json::to_value(Project::default()).expect("json");
        let obj = v.as_object_mut().expect("object");
        for key in ["format_version", "master", "loop_region", "next_id"] {
            obj.remove(key);
        }
        for t in obj["tracks"].as_array_mut().expect("tracks") {
            let t = t.as_object_mut().expect("track");
            t.remove("mixer");
            t.remove("clips");
        }
        let p = project_from_json(&v.to_string()).expect("load");
        assert_eq!(p.tracks.len(), 3);
        assert!(p.next_id >= 4, "next_id {}", p.next_id);
    }

    #[test]
    fn rejects_newer_formats_and_duplicate_ids() {
        let mut v = serde_json::to_value(Project::default()).expect("json");
        v["format_version"] = serde_json::json!(FORMAT_VERSION + 1);
        assert!(matches!(
            project_from_json(&v.to_string()),
            Err(FileError::TooNew(_))
        ));

        let mut v = serde_json::to_value(Project::default()).expect("json");
        v["tracks"][1]["id"] = serde_json::json!(1);
        assert!(matches!(
            project_from_json(&v.to_string()),
            Err(FileError::Invalid(_))
        ));
        assert!(matches!(
            project_from_json("not json"),
            Err(FileError::Parse(_))
        ));
    }
}
