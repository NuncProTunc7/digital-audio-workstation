//! Godot's side files: `.import` settings and adaptive-music `.tres`
//! resources. See `docs/godot-formats.md` for where each key comes from.

use std::fmt::Write as _;
use std::path::Path;

use daw_model::Project;

use crate::{AudioFormat, ExportError, RenderedLoop, io_err};

/// Validates a folder inside the project (`music/boss`), normalizing slashes.
pub(crate) fn check_folder(folder: &str) -> Result<String, ExportError> {
    let f = folder.trim().replace('\\', "/");
    let f = f.trim_start_matches("res://").trim_matches('/').to_owned();
    let bad = f
        .split('/')
        .any(|p| p == ".." || p == "." || p.contains(':'))
        || f.starts_with('/')
        || f.is_empty();
    if bad {
        Err(ExportError::BadFolder(folder.to_owned()))
    } else {
        Ok(f)
    }
}

pub(crate) fn res_path(folder: &str, file: &str) -> String {
    format!("res://{folder}/{file}")
}

/// Keeps the `uid` Godot gave a file, so re-exports don't break references.
fn existing_uid(import_path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(import_path).ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("uid=\"").and_then(|r| r.strip_suffix('"')))
        .map(str::to_owned)
}

/// Writes `<file>.import` so Godot imports the file looping (or not), with
/// beat information for beat-synced transitions.
pub(crate) fn write_import(
    audio_path: &Path,
    format: AudioFormat,
    rendered: &RenderedLoop,
    project: &Project,
) -> Result<(), ExportError> {
    let mut import_path = audio_path.as_os_str().to_owned();
    import_path.push(".import");
    let import_path = std::path::PathBuf::from(import_path);
    let uid = existing_uid(&import_path);
    let mut s = String::from("[remap]\n\n");
    match format {
        AudioFormat::Ogg => {
            s.push_str("importer=\"oggvorbisstr\"\ntype=\"AudioStreamOggVorbis\"\n")
        }
        AudioFormat::Wav => s.push_str("importer=\"wav\"\ntype=\"AudioStreamWAV\"\n"),
    }
    if let Some(uid) = uid {
        let _ = writeln!(s, "uid=\"{uid}\"");
    }
    s.push_str("\n[params]\n\n");
    // Godot loops at beat_count*60/bpm when both are set; only offer whole
    // beats so that matches the file exactly.
    let whole_beats =
        (rendered.beats - rendered.beats.round()).abs() < 1e-6 && rendered.beats >= 1.0;
    match format {
        AudioFormat::Ogg => {
            let _ = writeln!(s, "loop={}", rendered.looped);
            s.push_str("loop_offset=0.0\n");
            if whole_beats {
                let _ = writeln!(s, "bpm={}", project.tempo_bpm);
                let _ = writeln!(s, "beat_count={}", rendered.beats.round() as i64);
            } else {
                s.push_str("bpm=0.0\nbeat_count=0\n");
            }
            let _ = writeln!(s, "bar_beats={}", rendered.beats_per_bar.max(2));
        }
        AudioFormat::Wav => {
            let frames = rendered.stereo.len() / 2;
            if rendered.looped {
                let _ = writeln!(
                    s,
                    "edit/loop_mode=2\nedit/loop_begin=0\nedit/loop_end={frames}"
                );
            } else {
                s.push_str("edit/loop_mode=1\n");
            }
        }
    }
    std::fs::write(&import_path, s).map_err(|e| io_err(&import_path, e))
}

fn resource_header(kind: &str, paths: &[&str]) -> String {
    let mut s = format!(
        "[gd_resource type=\"{kind}\" load_steps={} format=3]\n\n",
        paths.len() + 1
    );
    for (i, p) in paths.iter().enumerate() {
        let _ = writeln!(
            s,
            "[ext_resource type=\"AudioStream\" path=\"{p}\" id=\"{}\"]",
            i + 1
        );
    }
    s.push_str("\n[resource]\n");
    s
}

/// An `AudioStreamSynchronized`: all stems start together, every layer at
/// full volume. Turn layers up and down from game code.
pub(crate) fn write_synchronized(path: &Path, stems: &[String]) -> Result<(), ExportError> {
    let stems: Vec<&str> = stems.iter().take(32).map(String::as_str).collect();
    let mut s = resource_header("AudioStreamSynchronized", &stems);
    let _ = writeln!(s, "stream_count = {}", stems.len());
    for i in 0..stems.len() {
        let _ = writeln!(s, "stream_{i}/stream = ExtResource(\"{}\")", i + 1);
        let _ = writeln!(s, "stream_{i}/volume = 0.0");
    }
    std::fs::write(path, s).map_err(|e| io_err(path, e))
}

/// An `AudioStreamInteractive` with one looping clip per section; switching
/// waits for the next bar and crossfades over two beats.
pub(crate) fn write_interactive(
    path: &Path,
    sections: &[(String, String)],
) -> Result<(), ExportError> {
    let sections = &sections[..sections.len().min(63)];
    let paths: Vec<&str> = sections.iter().map(|(_, p)| p.as_str()).collect();
    let mut s = resource_header("AudioStreamInteractive", &paths);
    // clip_count must come before initial_clip (Godot applies them in order).
    let _ = writeln!(s, "clip_count = {}", sections.len());
    for (i, (name, _)) in sections.iter().enumerate() {
        let name = name.replace('"', "'");
        let _ = writeln!(s, "clip_{i}/name = &\"{name}\"");
        let _ = writeln!(s, "clip_{i}/stream = ExtResource(\"{}\")", i + 1);
        let _ = writeln!(s, "clip_{i}/auto_advance = 0");
    }
    s.push_str("initial_clip = 0\n");
    s.push_str("_transitions = {\nVector2i(-1, -1): {\"from_time\": 2, \"to_time\": 1, \"fade_mode\": 3, \"fade_beats\": 2.0}\n}\n");
    std::fs::write(path, s).map_err(|e| io_err(path, e))
}
