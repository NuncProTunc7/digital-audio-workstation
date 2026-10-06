use std::path::{Path, PathBuf};
use std::sync::{Arc, MutexGuard};

use base64::Engine as _;
use daw_audio::audio_folder_for;
use daw_engine::capture::AudioRecorder;
use daw_engine::{AudioPool, Engine, EngineMessage};
use daw_model::summary::{clip_detail, song_summary, track_detail};
use daw_model::{AudioRegion, ClipId, Command, InstrumentKind, Project, Session, TrackId};
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
    /// Where the project's audio files are.
    fn audio(&self) -> Arc<AudioPool>;
    /// Starts recording the microphone onto an audio track. Hosts without a
    /// microphone say so.
    fn start_audio_recording(&self, _track_id: TrackId) -> Result<(), String> {
        Err("this copy of Nunc Pro Tune can't record audio".into())
    }
    /// Stops recording; returns the new clip's id (None if nothing was kept).
    fn stop_audio_recording(&self) -> Result<Option<ClipId>, String> {
        Err("nothing is recording".into())
    }
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
            Ok(json!({
                "saved_to": saved.path.display().to_string(),
                "missing_audio": saved.missing_audio,
            }))
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
            let analysis = daw_analysis::analyze(&project, &host.audio(), &options);
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
            let seconds = export_wav(
                &project,
                &host.audio(),
                &path,
                start_beats,
                end_beats,
                tail_seconds,
            )?;
            Ok(json!({ "exported_to": path.display().to_string(), "seconds": seconds }))
        }
        Request::ImportAudio {
            path,
            track_id,
            start_beats,
        } => {
            let clip_id = import_audio(host, Path::new(&path), track_id, start_beats)?;
            host.project_changed("Claude imported audio");
            let session = host.session()?;
            let project = session.project();
            let (track_id, clip) = project
                .clip(clip_id)
                .ok_or_else(|| "the imported clip vanished".to_owned())?;
            Ok(json!({ "track_id": track_id, "clip": clip, "song": song_summary(project) }))
        }
        Request::RecordAudio { track_id } => {
            host.start_audio_recording(track_id)?;
            Ok(json!({ "recording": true, "track_id": track_id }))
        }
        Request::StopRecording => {
            let clip_id = host.stop_audio_recording()?;
            if clip_id.is_some() {
                host.project_changed("Claude recorded audio");
            }
            Ok(json!({ "recording": false, "clip_id": clip_id }))
        }
    }
}

/// Imports an audio file onto `track_id` (or a new audio track named after
/// the file) at `start_beats` (default: the start). One undo step. Returns
/// the new clip's id.
pub fn import_audio<H: Host>(
    host: &H,
    path: &Path,
    track_id: Option<TrackId>,
    start_beats: Option<f64>,
) -> Result<ClipId, String> {
    let imported = host.audio().import(path).map_err(|e| e.to_string())?;
    let mut session = host.session()?;
    let project = session.project();
    let start_beats = start_beats.unwrap_or(0.0).max(0.0);
    let audio = AudioRegion {
        file: imported.file,
        file_seconds: imported.seconds,
        offset_seconds: 0.0,
        gain_db: 0.0,
        fade_in_seconds: 0.0,
        fade_out_seconds: 0.0,
    };
    let (command, track_id) = match track_id {
        Some(id) => (
            Command::AddAudioClip {
                track_id: id,
                start_beats,
                audio,
                length_beats: None,
                name: Some(imported.name),
            },
            id,
        ),
        None => {
            // The session lock is held, so the next id is the new track's.
            let new_id = project.next_id.max(1);
            (
                Command::Batch {
                    commands: vec![
                        Command::AddTrack {
                            name: imported.name.clone(),
                            instrument: InstrumentKind::Audio,
                            preset: None,
                            index: None,
                        },
                        Command::AddAudioClip {
                            track_id: new_id,
                            start_beats,
                            audio,
                            length_beats: None,
                            name: Some(imported.name),
                        },
                    ],
                },
                new_id,
            )
        }
    };
    session.execute(command).map_err(|e| e.to_string())?;
    session.end_gesture();
    sync(host, &session);
    newest_clip(session.project(), track_id)
}

/// The clip on `track_id` with the highest id (the one just added).
fn newest_clip(project: &Project, track_id: TrackId) -> Result<ClipId, String> {
    project
        .track(track_id)
        .and_then(|t| t.clips.iter().map(|c| c.id).max())
        .ok_or_else(|| format!("there is no clip on track {track_id}"))
}

/// Starts a take on `track_id`: checks it is an audio track, starts the
/// transport (with looping paused, so the take runs straight through), and
/// starts writing the microphone to a new file.
pub fn begin_take<H: Host>(
    host: &H,
    recorder: &mut AudioRecorder,
    track_id: TrackId,
) -> Result<(), String> {
    {
        let session = host.session()?;
        let track = session
            .project()
            .track(track_id)
            .ok_or_else(|| format!("there is no track with id {track_id}"))?;
        if !track.instrument.kind.is_audio() {
            return Err(format!(
                "\"{}\" is not an audio track; record audio onto an audio track",
                track.name
            ));
        }
    }
    let engine = engine(host)?;
    let (file, path) = host
        .audio()
        .new_recording_path()
        .map_err(|e| e.to_string())?;
    engine.send(EngineMessage::SetLoop {
        enabled: false,
        start_beats: 0.0,
        end_beats: 0.0,
    });
    let fallback = engine.status().position_beats;
    recorder
        .start(file, path, engine.status_handle(), fallback)
        .map_err(|e| e.to_string())?;
    engine.play();
    Ok(())
}

/// Finishes the take and places it on `track_id` as one undo step. Returns
/// the clip id, or None when the take was too short to keep.
pub fn end_take<H: Host>(
    host: &H,
    recorder: &mut AudioRecorder,
    track_id: TrackId,
) -> Result<Option<ClipId>, String> {
    let Some(result) = recorder.stop() else {
        return Ok(None);
    };
    let mut session = host.session()?;
    // Looping comes back as the project has it.
    if let Some(e) = host.engine() {
        let l = session.project().loop_region;
        e.send(EngineMessage::SetLoop {
            enabled: l.enabled,
            start_beats: l.start_beats,
            end_beats: l.end_beats,
        });
    }
    let take = result.map_err(|e| e.to_string())?;
    if take.seconds < 0.05 {
        let _ = std::fs::remove_file(&take.path);
        return Ok(None);
    }
    session
        .execute(Command::AddAudioClip {
            track_id,
            start_beats: take.start_beats,
            audio: AudioRegion {
                file: take.file,
                file_seconds: take.seconds,
                offset_seconds: 0.0,
                gain_db: 0.0,
                fade_in_seconds: 0.0,
                fade_out_seconds: 0.0,
            },
            length_beats: None,
            name: None,
        })
        .map_err(|e| e.to_string())?;
    session.end_gesture();
    sync(host, &session);
    newest_clip(session.project(), track_id).map(Some)
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
    let audio = host.audio();
    audio.clear_cache();
    audio.set_project_folder(None);
    replace(host, Project::default(), None)
}

pub fn open_project<H: Host>(host: &H, path: &Path) -> Result<(), String> {
    let project = daw_model::load_project(path).map_err(|e| e.to_string())?;
    let audio = host.audio();
    audio.clear_cache();
    audio.set_project_folder(Some(audio_folder_for(path)));
    replace(host, project, Some(path.to_owned()))
}

/// Where a project was saved, and any audio it refers to that couldn't be
/// found (those clips stay silent).
#[derive(Debug, Clone, PartialEq)]
pub struct SavedProject {
    pub path: PathBuf,
    pub missing_audio: Vec<String>,
}

/// Saves to `path` (adding `.nptune` if missing) or to the current file,
/// and copies the project's audio into the "<Song> Audio" folder beside it.
pub fn save_project<H: Host>(host: &H, path: Option<&Path>) -> Result<SavedProject, String> {
    let target = match path {
        Some(p) => with_extension(p),
        None => host
            .project_path()
            .ok_or_else(|| "the project has not been saved yet; give a file path".to_owned())?,
    };
    let mut session = host.session()?;
    let files: Vec<String> = session
        .project()
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .filter_map(|c| c.audio.as_ref().map(|a| a.file.clone()))
        .collect();
    let audio = host.audio();
    let folder = audio_folder_for(&target);
    let missing_audio = if files.is_empty() {
        Vec::new()
    } else {
        audio
            .gather(files.iter().map(String::as_str), &folder)
            .map_err(|e| e.to_string())?
    };
    daw_model::save_project(session.project(), &target).map_err(|e| e.to_string())?;
    session.mark_saved();
    drop(session);
    audio.set_project_folder(Some(folder));
    host.set_project_path(Some(target.clone()));
    Ok(SavedProject {
        path: target,
        missing_audio,
    })
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
    audio: &Arc<AudioPool>,
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
    let audio = daw_engine::offline::render_region(project, audio, start, end, tail, SR);
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
    pub struct TestHost {
        pub session: Mutex<Session>,
        pub path: Mutex<Option<PathBuf>>,
        pub changes: Mutex<Vec<String>>,
        pub audio: Arc<AudioPool>,
        // Keeps the scratch folder alive for the test.
        _scratch: tempfile::TempDir,
    }

    impl Default for TestHost {
        fn default() -> Self {
            let scratch = tempfile::tempdir().expect("tmp");
            Self {
                session: Mutex::default(),
                path: Mutex::default(),
                changes: Mutex::default(),
                audio: Arc::new(AudioPool::new(scratch.path().to_owned())),
                _scratch: scratch,
            }
        }
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
        fn audio(&self) -> Arc<AudioPool> {
            Arc::clone(&self.audio)
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

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../daw-audio/tests/fixtures")
            .join(name)
    }

    #[test]
    fn importing_a_phone_recording_makes_an_audio_track_in_one_undo_step() {
        let host = TestHost::default();
        let before = host.session().expect("s").project().clone();
        let v = ok(handle(
            &host,
            Request::ImportAudio {
                path: fixture("voice-memo.m4a").display().to_string(),
                track_id: None,
                start_beats: Some(4.0),
            },
        ));
        assert_eq!(v["clip"]["name"], "voice-memo");
        assert_eq!(v["clip"]["start_beats"], 4.0);
        let track_id = v["track_id"].as_u64().expect("id") as u32;
        {
            let s = host.session().expect("s");
            let t = s.project().track(track_id).expect("new track");
            assert_eq!(t.instrument.kind, InstrumentKind::Audio);
            assert_eq!(t.name, "voice-memo");
            // One second at 120 BPM is two beats.
            assert!((t.clips[0].length_beats - 2.0).abs() < 0.1);
        }
        ok(handle(&host, Request::Undo));
        assert_eq!(host.session().expect("s").project().tracks, before.tracks);
    }

    #[test]
    fn importing_onto_an_instrument_track_is_refused() {
        let host = TestHost::default();
        let err = handle(
            &host,
            Request::ImportAudio {
                path: fixture("tone.mp3").display().to_string(),
                track_id: Some(1),
                start_beats: None,
            },
        )
        .into_result()
        .expect_err("not an audio track");
        assert!(err.contains("not an audio track"), "{err}");
    }

    #[test]
    fn saving_puts_audio_next_to_the_project_and_opening_finds_it() {
        let host = TestHost::default();
        let dir = tempfile::tempdir().expect("tmp");
        let clip_id = import_audio(&host, &fixture("tone.flac"), None, None).expect("import");
        let file = {
            let s = host.session().expect("s");
            s.project()
                .clip(clip_id)
                .expect("clip")
                .1
                .audio
                .clone()
                .expect("audio")
                .file
        };
        let song = dir.path().join("Boss Theme.nptune");
        let saved = ok(handle(
            &host,
            Request::Save {
                path: Some(song.display().to_string()),
            },
        ));
        assert_eq!(saved["missing_audio"].as_array().map(Vec::len), Some(0));
        let copied = dir.path().join("Boss Theme Audio").join(&file);
        assert!(copied.is_file(), "{}", copied.display());

        // A fresh app (new pool, empty scratch) opening the file finds it.
        let other = TestHost::default();
        ok(handle(
            &other,
            Request::Open {
                path: song.display().to_string(),
            },
        ));
        assert!(other.audio.has(&file));
    }

    #[test]
    fn recording_needs_a_microphone() {
        let host = TestHost::default();
        let err = handle(&host, Request::RecordAudio { track_id: 1 })
            .into_result()
            .expect_err("no mic");
        assert!(err.contains("can't record"), "{err}");
    }
}
