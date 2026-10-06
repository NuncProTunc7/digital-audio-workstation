use std::path::{Path, PathBuf};
use std::sync::{Arc, MutexGuard};

use base64::Engine as _;
use daw_engine::Engine;
use daw_model::summary::{clip_detail, song_summary, track_detail};
use daw_model::{Command, Project, Session};
use serde_json::{Value, json};

use crate::protocol::{Request, Response};

/// What the request handler needs from whoever owns the project (the desktop
/// app, or a test).
pub trait Host: Send + Sync {
    fn session(&self) -> Result<MutexGuard<'_, Session>, String>;
    /// The engine playing to the sound card, if audio is running.
    fn engine(&self) -> Option<Arc<Engine>>;
    /// Called after the project changed (by an edit, undo, open, ...), with a
    /// short description such as "Claude: create clip". The app refreshes its
    /// screens here.
    fn project_changed(&self, description: &str);
    fn project_path(&self) -> Option<PathBuf>;
    fn set_project_path(&self, path: Option<PathBuf>);
}

/// A short label for the app's activity list, e.g. "Claude: set tempo" or
/// "Claude: 4 changes (set tempo, create clip)".
fn describe(command: &Command) -> String {
    let words = |c: &Command| c.name().replace('_', " ");
    match command {
        Command::Batch { commands } => {
            let mut names: Vec<String> = Vec::new();
            for c in commands {
                let n = words(c);
                if !names.contains(&n) {
                    names.push(n);
                }
            }
            let more = if names.len() > 3 { ", ..." } else { "" };
            names.truncate(3);
            format!(
                "Claude: {} changes ({}{more})",
                commands.len(),
                names.join(", ")
            )
        }
        c => format!("Claude: {}", words(c)),
    }
}

/// Handles one request. Errors come back as messages written for Claude.
pub fn handle<H: Host>(host: &H, request: Request) -> Response {
    handle_inner(host, request).into()
}

fn handle_inner<H: Host>(host: &H, request: Request) -> Result<Value, String> {
    match request {
        Request::Ping => {
            Ok(json!({ "app": "Nunc Pro Tune", "version": env!("CARGO_PKG_VERSION") }))
        }
        Request::Execute { command } => {
            let name = command.name();
            let description = describe(&command);
            let mut session = host.session()?;
            session.execute(command).map_err(|e| e.to_string())?;
            // Each remote edit is its own undo step.
            session.end_gesture();
            sync(host, &session);
            let summary = song_summary(session.project());
            drop(session);
            host.project_changed(&description);
            Ok(json!({ "applied": name, "song": summary }))
        }
        Request::Undo | Request::Redo => {
            let undo = matches!(request, Request::Undo);
            let mut session = host.session()?;
            let done = if undo { session.undo() } else { session.redo() };
            sync(host, &session);
            let summary = song_summary(session.project());
            drop(session);
            if done {
                host.project_changed(if undo { "Claude: undo" } else { "Claude: redo" });
            }
            Ok(json!({ "changed": done, "song": summary }))
        }
        Request::GetSong => Ok(song_summary(host.session()?.project())),
        Request::GetTrack { track_id } => {
            let session = host.session()?;
            let track = session
                .project()
                .track(track_id)
                .ok_or_else(|| format!("there is no track with id {track_id}"))?;
            Ok(track_detail(track))
        }
        Request::GetClip { clip_id } => {
            let session = host.session()?;
            let project = session.project();
            let (track_id, clip) = project
                .clip(clip_id)
                .ok_or_else(|| format!("there is no clip with id {clip_id}"))?;
            let track = project
                .track(track_id)
                .ok_or_else(|| format!("there is no track with id {track_id}"))?;
            Ok(clip_detail(track, clip))
        }
        Request::Describe => {
            serde_json::to_value(daw_instruments::catalog()).map_err(|e| e.to_string())
        }
        Request::Play { from_beats } => {
            let engine = engine(host)?;
            if let Some(b) = from_beats {
                engine.locate(b);
            }
            engine.play();
            Ok(json!({ "playing": true }))
        }
        Request::Stop => {
            engine(host)?.stop();
            Ok(json!({ "playing": false }))
        }
        Request::Locate { beats } => {
            engine(host)?.locate(beats);
            Ok(json!({ "position_beats": beats.max(0.0) }))
        }
        Request::SetMetronome { on } => {
            engine(host)?.set_metronome(on);
            Ok(json!({ "metronome": on }))
        }
        Request::Status => {
            let s = engine(host)?.status();
            Ok(json!({
                "playing": s.playing,
                "position_beats": s.position_beats,
                "cpu_load": s.cpu_load,
                "sample_rate_hz": s.sample_rate_hz,
            }))
        }
        Request::Save { path } => {
            let saved = save_project(host, path.as_deref().map(Path::new))?;
            host.project_changed("Claude saved the project");
            Ok(json!({ "saved_to": saved.display().to_string() }))
        }
        Request::Open { path } => {
            open_project(host, Path::new(&path))?;
            host.project_changed("Claude opened a project");
            Ok(song_summary(host.session()?.project()))
        }
        Request::New => {
            new_project(host)?;
            host.project_changed("Claude started a new project");
            Ok(song_summary(host.session()?.project()))
        }
        Request::Analyze {
            start_beats,
            end_beats,
            per_track,
            spectrogram,
        } => {
            // Snapshot, then render without holding the lock (rendering can
            // take a few seconds and the UI must stay responsive).
            let project = host.session()?.project().clone();
            let options = daw_analysis::AnalyzeOptions {
                start_beats,
                end_beats,
                per_track: per_track.unwrap_or(true),
                spectrogram: spectrogram.unwrap_or(false),
            };
            let analysis = daw_analysis::analyze(&project, &options);
            let mut v = serde_json::to_value(&analysis).map_err(|e| e.to_string())?;
            if let Some(png) = analysis.spectrogram_png {
                v["spectrogram_png_base64"] =
                    json!(base64::engine::general_purpose::STANDARD.encode(png));
            }
            Ok(v)
        }
        Request::ExportWav {
            path,
            start_beats,
            end_beats,
            tail_seconds,
        } => {
            let project = host.session()?.project().clone();
            let path = PathBuf::from(path);
            let seconds = export_wav(&project, &path, start_beats, end_beats, tail_seconds)?;
            Ok(json!({ "exported_to": path.display().to_string(), "seconds": seconds }))
        }
    }
}

fn engine<H: Host>(host: &H) -> Result<Arc<Engine>, String> {
    host.engine().ok_or_else(|| {
        "audio isn't running in Nunc Pro Tune (check the output device in the status bar)".into()
    })
}

fn sync<H: Host>(host: &H, session: &Session) {
    if let Some(e) = host.engine() {
        e.sync(session.project());
    }
}

/// Replaces the open project and stops playback.
fn replace<H: Host>(host: &H, project: Project, path: Option<PathBuf>) -> Result<(), String> {
    if let Some(e) = host.engine() {
        // Twice: the second stop rewinds to the start.
        e.stop();
        e.stop();
        e.all_notes_off();
    }
    let mut session = host.session()?;
    session.replace_project(project);
    sync(host, &session);
    drop(session);
    host.set_project_path(path);
    Ok(())
}

/// Starts a fresh default project.
pub fn new_project<H: Host>(host: &H) -> Result<(), String> {
    replace(host, Project::default(), None)
}

pub fn open_project<H: Host>(host: &H, path: &Path) -> Result<(), String> {
    let project = daw_model::load_project(path).map_err(|e| e.to_string())?;
    replace(host, project, Some(path.to_owned()))
}

/// Saves to `path` (adding `.nptune` if missing) or to the current file.
/// Returns where it was saved.
pub fn save_project<H: Host>(host: &H, path: Option<&Path>) -> Result<PathBuf, String> {
    let target = match path {
        Some(p) => with_extension(p),
        None => host
            .project_path()
            .ok_or_else(|| "the project has not been saved yet; give a file path".to_owned())?,
    };
    let mut session = host.session()?;
    daw_model::save_project(session.project(), &target).map_err(|e| e.to_string())?;
    session.mark_saved();
    drop(session);
    host.set_project_path(Some(target.clone()));
    Ok(target)
}

fn with_extension(path: &Path) -> PathBuf {
    if path
        .extension()
        .is_some_and(|e| e == daw_model::PROJECT_EXTENSION)
    {
        path.to_owned()
    } else {
        let mut s = path.as_os_str().to_owned();
        s.push(".");
        s.push(daw_model::PROJECT_EXTENSION);
        PathBuf::from(s)
    }
}

/// Renders a region (default: whole song plus a 2 s tail) to 24-bit WAV.
fn export_wav(
    project: &Project,
    path: &Path,
    start_beats: Option<f64>,
    end_beats: Option<f64>,
    tail_seconds: Option<f64>,
) -> Result<f64, String> {
    const SR: u32 = 48_000;
    let start = start_beats.unwrap_or(0.0).max(0.0);
    let end = end_beats.unwrap_or_else(|| project.end_beats()).max(start);
    let tail = tail_seconds.unwrap_or(2.0).clamp(0.0, 60.0);
    if end <= start && tail <= 0.0 {
        return Err("nothing to export: the song is empty".into());
    }
    let audio = daw_engine::offline::render_region(project, start, end, tail, SR);
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SR,
        bits_per_sample: 24,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    const FULL_SCALE: f32 = 8_388_607.0;
    for s in &audio {
        let v = (s.clamp(-1.0, 1.0) * FULL_SCALE).round() as i32;
        writer.write_sample(v).map_err(|e| e.to_string())?;
    }
    writer.finalize().map_err(|e| e.to_string())?;
    Ok(audio.len() as f64 / 2.0 / f64::from(SR))
}

/// Shortcut for tests.
#[cfg(test)]
fn execute<H: Host>(host: &H, command: Command) -> Response {
    handle(host, Request::Execute { command })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A host with no sound card, like a headless test or CI.
    #[derive(Default)]
    pub struct TestHost {
        pub session: Mutex<Session>,
        pub path: Mutex<Option<PathBuf>>,
        pub changes: Mutex<Vec<String>>,
    }

    impl Host for TestHost {
        fn session(&self) -> Result<MutexGuard<'_, Session>, String> {
            self.session.lock().map_err(|_| "poisoned".to_owned())
        }
        fn engine(&self) -> Option<Arc<Engine>> {
            None
        }
        fn project_changed(&self, description: &str) {
            if let Ok(mut c) = self.changes.lock() {
                c.push(description.to_owned());
            }
        }
        fn project_path(&self) -> Option<PathBuf> {
            self.path.lock().ok().and_then(|p| p.clone())
        }
        fn set_project_path(&self, path: Option<PathBuf>) {
            if let Ok(mut p) = self.path.lock() {
                *p = path;
            }
        }
    }

    fn ok(r: Response) -> Value {
        r.into_result().expect("request succeeded")
    }

    #[test]
    fn edits_report_the_new_song_and_notify_the_ui() {
        let host = TestHost::default();
        let v = ok(execute(&host, Command::SetTempo { bpm: 140.0 }));
        assert_eq!(v["applied"], "set_tempo");
        assert_eq!(v["song"]["tempo_bpm"], 140.0);
        assert_eq!(host.changes.lock().expect("lock")[0], "Claude: set tempo");
    }

    #[test]
    fn batches_are_described_by_their_steps() {
        let tempo = |bpm| Command::SetTempo { bpm };
        let rename = Command::RenameProject {
            name: "Boss".into(),
        };
        let batch = Command::Batch {
            commands: vec![tempo(100.0), rename, tempo(110.0)],
        };
        assert_eq!(
            describe(&batch),
            "Claude: 3 changes (set tempo, rename project)"
        );
    }

    #[test]
    fn each_remote_edit_is_its_own_undo_step() {
        let host = TestHost::default();
        for bpm in [100.0, 110.0] {
            ok(execute(
                &host,
                Command::SetInstrumentParam {
                    track_id: 1,
                    param: "filter.cutoff_hz".into(),
                    value: bpm * 10.0,
                },
            ));
        }
        ok(handle(&host, Request::Undo));
        let v = ok(handle(&host, Request::GetTrack { track_id: 1 }));
        assert_eq!(v["instrument_params"]["filter.cutoff_hz"], 1000.0);
    }

    #[test]
    fn errors_are_readable() {
        let host = TestHost::default();
        let err = execute(&host, Command::DeleteClip { clip_id: 42 })
            .into_result()
            .expect_err("fails");
        assert!(err.contains("no clip with id 42"), "{err}");
        let err = handle(&host, Request::Play { from_beats: None })
            .into_result()
            .expect_err("no audio");
        assert!(err.contains("audio isn't running"), "{err}");
    }

    #[test]
    fn clip_and_track_queries() {
        let host = TestHost::default();
        let v = ok(execute(
            &host,
            Command::CreateClip {
                track_id: 3,
                start_beats: 0.0,
                length_beats: 4.0,
                name: Some("Beat".into()),
                notes: vec![daw_model::NoteInput {
                    pitch: 36,
                    start_beats: 0.0,
                    length_beats: 0.25,
                    velocity: 120,
                    id: None,
                }],
            },
        ));
        let clip_id = v["song"]["tracks"][2]["clips"][0]["id"]
            .as_u64()
            .expect("id") as u32;
        let clip = ok(handle(&host, Request::GetClip { clip_id }));
        assert_eq!(clip["clip"]["notes"][0]["pitch"], 36);
        assert_eq!(clip["instrument"], "drums");
        let catalog = ok(handle(&host, Request::Describe));
        assert!(catalog["effects"].as_array().is_some_and(|e| e.len() == 7));
    }

    #[test]
    fn save_open_and_export_round_trip() {
        let host = TestHost::default();
        let dir = std::env::temp_dir().join(format!("npt-control-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        ok(execute(
            &host,
            Command::RenameProject {
                name: "Saved".into(),
            },
        ));
        ok(execute(
            &host,
            Command::CreateClip {
                track_id: 1,
                start_beats: 0.0,
                length_beats: 4.0,
                name: None,
                notes: vec![daw_model::NoteInput {
                    pitch: 60,
                    start_beats: 0.0,
                    length_beats: 2.0,
                    velocity: 100,
                    id: None,
                }],
            },
        ));
        let saved = ok(handle(
            &host,
            Request::Save {
                path: Some(dir.join("song").display().to_string()),
            },
        ));
        assert!(
            saved["saved_to"]
                .as_str()
                .is_some_and(|p| p.ends_with("song.nptune"))
        );
        ok(handle(&host, Request::New));
        assert_eq!(ok(handle(&host, Request::GetSong))["name"], "Untitled");
        ok(handle(
            &host,
            Request::Open {
                path: dir.join("song.nptune").display().to_string(),
            },
        ));
        assert_eq!(ok(handle(&host, Request::GetSong))["name"], "Saved");

        let wav = dir.join("song.wav");
        let v = ok(handle(
            &host,
            Request::ExportWav {
                path: wav.display().to_string(),
                start_beats: None,
                end_beats: None,
                tail_seconds: Some(0.5),
            },
        ));
        assert!((v["seconds"].as_f64().expect("secs") - 2.5).abs() < 0.01);
        let reader = hound::WavReader::open(&wav).expect("wav");
        assert_eq!(reader.spec().bits_per_sample, 24);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn analyze_returns_measurements_and_optional_spectrogram() {
        let host = TestHost::default();
        ok(execute(
            &host,
            Command::CreateClip {
                track_id: 1,
                start_beats: 0.0,
                length_beats: 4.0,
                name: None,
                notes: vec![daw_model::NoteInput {
                    pitch: 60,
                    start_beats: 0.0,
                    length_beats: 3.0,
                    velocity: 100,
                    id: None,
                }],
            },
        ));
        let v = ok(handle(
            &host,
            Request::Analyze {
                start_beats: None,
                end_beats: None,
                per_track: Some(false),
                spectrogram: Some(true),
            },
        ));
        assert!(v["mix"]["integrated_lufs"].is_number());
        assert!(
            v["spectrogram_png_base64"]
                .as_str()
                .is_some_and(|s| s.starts_with("iVBOR"))
        );
    }
}
