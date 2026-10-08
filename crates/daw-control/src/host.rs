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
    /// Whether the user has the metronome on (restored after calibrating).
    fn metronome_on(&self) -> bool {
        false
    }
    /// Records that Claude is changing the song and returns when it last
    /// did in this run (`Some(None)`: not yet). Claude's first change after
    /// a quiet spell saves a "Before Claude's changes" version first. Hosts
    /// that return None never save versions automatically.
    fn note_claude_edit(&self) -> Option<Option<std::time::Instant>> {
        None
    }
    /// Devices, load, and what happened recently, for
    /// [`diagnostics::report`](crate::diagnostics::report).
    fn diagnostic_info(&self) -> crate::diagnostics::DiagnosticInfo {
        crate::diagnostics::DiagnosticInfo {
            app_version: env!("CARGO_PKG_VERSION").into(),
            os: std::env::consts::OS.into(),
            ..Default::default()
        }
    }
    /// Where the user's own presets are kept.
    fn presets_path(&self) -> PathBuf {
        crate::presets::presets_path()
    }
    /// Where the list of installed plugins is remembered.
    fn plugin_cache_path(&self) -> PathBuf {
        crate::plugins::cache_path()
    }
    /// The folders plugins are installed in.
    fn plugin_folders(&self) -> Vec<PathBuf> {
        daw_plugins::scan::default_folders()
    }
    /// A program that answers `--scan-vst3 <path>` (the app itself), so
    /// plugins are first opened in a separate process. None opens them
    /// here.
    fn plugin_scanner(&self) -> Option<PathBuf> {
        None
    }
    /// Where downloaded library instruments are kept.
    fn library_dir(&self) -> PathBuf {
        crate::library::library_dir()
    }
    /// Bars of clicks before recording starts (0 = none).
    fn count_in_bars(&self) -> u32 {
        0
    }
    /// Sets the count-in (0-2 bars); returns what was stored.
    fn set_count_in_bars(&self, _bars: u32) -> Result<u32, String> {
        Err("this copy of Nunc Pro Tune has no count-in setting".into())
    }
    /// How late the current microphone's recordings arrive (ms); takes are
    /// moved this much earlier.
    fn recording_offset_ms(&self) -> f64 {
        0.0
    }
    /// The current microphone's recording delay, after setting it to `ms`
    /// when given.
    fn recording_delay(&self, _set_ms: Option<f64>) -> Result<RecordingDelay, String> {
        Err("this copy of Nunc Pro Tune can't record audio".into())
    }
    /// Has the user clap along with clicks to measure the recording delay,
    /// and keeps the result when it's reliable.
    fn calibrate_recording(&self) -> Result<CalibrationResult, String> {
        Err("this copy of Nunc Pro Tune can't record audio".into())
    }
}

/// A microphone's recording delay setting.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RecordingDelay {
    /// The microphone it applies to.
    pub device: Option<String>,
    pub offset_ms: f64,
    /// The device looks like a Bluetooth headset (expect a delay).
    pub bluetooth: bool,
}

/// What clapping along measured, and whether it was kept.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CalibrationResult {
    #[serde(flatten)]
    pub measured: crate::calibrate::Calibration,
    /// The measurement was steady enough and is now the device's offset.
    pub saved: bool,
    pub device: Option<String>,
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
    let method = serde_json::to_value(&request)
        .ok()
        .and_then(|v| v.get("method").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default();
    let result = handle_inner(host, request);
    if let Err(e) = &result {
        crate::diagnostics::log_error(&format!("Claude's {method} failed: {e}"));
    }
    result.into()
}

/// Seconds of ring-out kept after the song's end in a frozen rendering.
const FREEZE_TAIL_SECONDS: f64 = 4.0;
/// Sample rate frozen renderings are made at (resampled to the device's).
const FREEZE_SAMPLE_RATE_HZ: u32 = 48_000;

/// Renders `track_id`'s sound (instrument and effects, fader flat, nothing
/// else playing) for the whole song, saves it in the project's audio
/// folder, and freezes the track to it (one undo step).
pub fn freeze_track<H: Host>(host: &H, track_id: TrackId) -> Result<(), String> {
    // Plugins: render what is playing, not the last save.
    crate::plugins::store_states(host);
    let project = host.session()?.project().clone();
    let track = project
        .track(track_id)
        .ok_or_else(|| format!("there is no track with id {track_id}"))?;
    if track.instrument.kind.is_audio() {
        return Err("audio tracks are already audio; there's nothing to freeze".into());
    }
    let fingerprint = project.freeze_fingerprint(track);
    // Only this track, as it sounds before its fader.
    let mut solo = project.clone();
    solo.loop_region.enabled = false;
    solo.master.volume_db = 0.0;
    solo.master.effects.clear();
    for t in &mut solo.tracks {
        if t.id == track_id {
            t.frozen = None;
            t.mixer.volume_db = 0.0;
            t.mixer.pan = 0.0;
            t.mixer.mute = false;
            t.mixer.solo = false;
            t.output = None;
            t.sends.clear();
            t.automation.retain(|l| {
                !matches!(
                    l.target,
                    daw_model::AutomationTarget::Volume | daw_model::AutomationTarget::Pan
                )
            });
        } else {
            t.mixer.solo = false;
            t.mixer.mute = true;
        }
    }
    let audio = host.audio();
    let stereo =
        daw_engine::offline::render_song(&solo, &audio, FREEZE_SAMPLE_RATE_HZ, FREEZE_TAIL_SECONDS);
    let data = daw_audio::AudioData {
        sample_rate_hz: FREEZE_SAMPLE_RATE_HZ,
        channels: vec![
            stereo.iter().step_by(2).copied().collect(),
            stereo.iter().skip(1).step_by(2).copied().collect(),
        ],
    };
    let file = format!("freeze-{track_id}-{fingerprint:016x}.wav");
    let path = audio.write_folder().join(&file);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    daw_audio::write_wav(&path, &data).map_err(|e| e.to_string())?;
    let mut session = host.session()?;
    session
        .execute(Command::FreezeTrack {
            track_id,
            frozen: daw_model::Frozen { file, fingerprint },
        })
        .map_err(|e| e.to_string())?;
    session.end_gesture();
    sync(host, &session);
    Ok(())
}

/// Saves `track_id`'s instrument as the user's preset `name`.
pub fn save_user_preset<H: Host>(
    host: &H,
    track_id: TrackId,
    name: &str,
) -> Result<Vec<crate::presets::UserPreset>, String> {
    let instrument = host
        .session()?
        .project()
        .track(track_id)
        .ok_or_else(|| format!("there is no track with id {track_id}"))?
        .instrument
        .clone();
    if instrument.kind == InstrumentKind::Plugin {
        return Err("plugins keep their own presets: save one in the plugin's window".into());
    }
    crate::presets::save(&host.presets_path(), name, &instrument)
}

/// Gives `track_id` the user's preset `name` (as one undoable edit).
pub fn load_user_preset<H: Host>(host: &H, track_id: TrackId, name: &str) -> Result<(), String> {
    let mut session = host.session()?;
    let kind = session
        .project()
        .track(track_id)
        .ok_or_else(|| format!("there is no track with id {track_id}"))?
        .instrument
        .kind;
    let preset = crate::presets::find(&host.presets_path(), kind, name)?;
    session
        .execute(Command::SetInstrument {
            track_id,
            instrument: preset.instrument(),
        })
        .map_err(|e| e.to_string())?;
    session.end_gesture();
    sync(host, &session);
    Ok(())
}

/// The diagnostic report as plain text.
pub fn diagnostic_report<H: Host>(host: &H) -> Result<String, String> {
    let info = host.diagnostic_info();
    let session = host.session()?;
    Ok(crate::diagnostics::report(&info, session.project()))
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
            let command = with_safety_version(host, session.project(), command);
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
            let mut v = track_detail(track);
            if let Some(pack) = &track.instrument.sample_pack {
                v["sample_pack_status"] = json!(sample_pack_status(Path::new(pack)));
            }
            Ok(v)
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
        Request::DiagnosticReport => Ok(json!({ "report": diagnostic_report(host)? })),
        Request::FreezeTrack { track_id } => {
            freeze_track(host, track_id)?;
            host.project_changed("Claude froze a track");
            Ok(json!({ "frozen": track_id }))
        }
        Request::InspectExport(options) => {
            serde_json::to_value(inspect_export(host, &options)?).map_err(|e| e.to_string())
        }
        Request::SavePreset { track_id, name } => {
            let list = save_user_preset(host, track_id, &name)?;
            host.project_changed(&format!("Claude saved preset {}", name.trim()));
            Ok(json!({ "saved": name.trim(), "presets": list }))
        }
        Request::UserPresets => {
            Ok(json!({ "presets": crate::presets::load(&host.presets_path()) }))
        }
        Request::LoadUserPreset { track_id, name } => {
            load_user_preset(host, track_id, &name)?;
            host.project_changed(&format!("Claude: load preset {name}"));
            Ok(json!({ "track_id": track_id, "preset": name }))
        }
        Request::Plugins { rescan } => {
            let cache = if rescan {
                crate::plugins::rescan(host)
            } else {
                crate::plugins::load_cache(&host.plugin_cache_path())
            };
            let failures: Vec<Value> = cache
                .failures()
                .into_iter()
                .map(|(path, error)| json!({ "path": path, "error": error }))
                .collect();
            Ok(json!({ "plugins": cache.plugins(), "could_not_use": failures }))
        }
        Request::LoadPlugin { track_id, uid } => {
            let info = crate::plugins::load_plugin(host, track_id, &uid)?;
            Ok(json!({ "track_id": track_id, "plugin": info.name }))
        }
        Request::PluginParams { track_id } => {
            let params = crate::plugins::params(host, track_id)?;
            Ok(json!({ "track_id": track_id, "params": params }))
        }
        Request::SampleLibrary => {
            let dir = host.library_dir();
            let session = host.session()?;
            Ok(json!({
                "instruments": crate::library::list(&dir),
                "credits_this_song_needs": crate::library::credits_for(session.project(), &dir),
            }))
        }
        Request::DownloadSamplePack { id } => {
            crate::library::start_download(&host.library_dir(), &id)?;
            Ok(
                json!({ "downloading": id, "check": "call sample_library until it shows installed programs" }),
            )
        }
        Request::SetCountIn { bars } => {
            let bars = host.set_count_in_bars(bars)?;
            host.project_changed("Claude set the count-in");
            Ok(json!({ "count_in_bars": bars }))
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
            // Plugins: render what is playing, not the last save.
            crate::plugins::store_states(host);
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
            // Plugins: render what is playing, not the last save.
            crate::plugins::store_states(host);
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
        Request::ImportMidi { path } => {
            let tracks = crate::notation::import_midi_file(host, Path::new(&path))?;
            host.project_changed("Claude imported a MIDI file");
            Ok(json!({ "new_track_ids": tracks, "song": song_summary(host.session()?.project()) }))
        }
        Request::ExportMidi { path } => {
            crate::notation::export_midi_file(host, Path::new(&path))?;
            Ok(json!({ "exported_to": path }))
        }
        Request::ExportGodot(options) => {
            let report = export_godot(host, &options)?;
            serde_json::to_value(report).map_err(|e| e.to_string())
        }
        Request::ImportMusicXml { path, xml } => {
            let tracks = match (path, xml) {
                (Some(p), _) => crate::notation::import_musicxml_file(host, Path::new(&p))?,
                (None, Some(x)) => crate::notation::import_musicxml_text(host, &x)?,
                (None, None) => return Err("give either a path or MusicXML text".into()),
            };
            host.project_changed("Claude imported sheet music");
            Ok(json!({ "new_track_ids": tracks, "song": song_summary(host.session()?.project()) }))
        }
        Request::ExportMusicXml { path, track_ids } => match path {
            Some(p) => {
                crate::notation::export_musicxml_file(host, Path::new(&p), track_ids.as_deref())?;
                Ok(json!({ "exported_to": p }))
            }
            None => {
                Ok(json!({ "musicxml": crate::notation::sheet_music(host, track_ids.as_deref())? }))
            }
        },
        Request::RecordingDelay { ms } => Ok(json!(host.recording_delay(ms)?)),
        Request::CalibrateRecording => Ok(json!(host.calibrate_recording()?)),
        Request::StopRecording => {
            let clip_id = host.stop_audio_recording()?;
            if clip_id.is_some() {
                host.project_changed("Claude recorded audio");
            }
            Ok(json!({ "recording": false, "clip_id": clip_id }))
        }
    }
}

/// How a sampler's pack is doing, as JSON for the UI and Claude.
pub fn sample_pack_status(path: &Path) -> Value {
    use daw_sampler::PackStatus as S;
    match daw_sampler::pack_status(path) {
        S::NotLoaded => json!({ "state": "not_loaded" }),
        S::Loading => json!({ "state": "loading" }),
        S::Ready {
            name,
            zones,
            megabytes,
            layers_kept,
            layers_total,
        } => json!({
            "state": "ready",
            "name": name,
            "zones": zones,
            "megabytes": megabytes,
            "layers_kept": layers_kept,
            "layers_total": layers_total,
        }),
        S::Failed(e) => json!({ "state": "failed", "error": e }),
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
        source_bpm: None,
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

/// Claude's first edit after this long without one saves a version first.
pub const CLAUDE_QUIET: std::time::Duration = std::time::Duration::from_secs(10 * 60);
/// Name of the versions saved before Claude starts changing things.
pub const BEFORE_CLAUDE: &str = "Before Claude's changes";

/// Puts "save a version" in front of Claude's first edit after a quiet
/// spell, as one undo step, so the user can always get back to their own
/// work. Only for songs with something in them, and never for edits that
/// only touch versions.
fn with_safety_version<H: Host>(host: &H, project: &Project, command: Command) -> Command {
    let Some(previous) = host.note_claude_edit() else {
        return command;
    };
    let quiet = previous.is_none_or(|t| t.elapsed() >= CLAUDE_QUIET);
    let has_music = project.tracks.iter().any(|t| !t.clips.is_empty());
    let about_versions = matches!(
        command,
        Command::TakeSnapshot { .. }
            | Command::LoadSnapshot { .. }
            | Command::RenameSnapshot { .. }
            | Command::DeleteSnapshot { .. }
            | Command::RestoreSnapshot { .. }
    );
    let full = project.snapshots.len() >= daw_model::MAX_SNAPSHOTS;
    if !quiet || !has_music || about_versions || full {
        return command;
    }
    let taken = project
        .snapshots
        .iter()
        .filter(|s| s.name.starts_with(BEFORE_CLAUDE))
        .count();
    let name = if taken == 0 {
        BEFORE_CLAUDE.to_owned()
    } else {
        format!("{BEFORE_CLAUDE} ({})", taken + 1)
    };
    Command::Batch {
        commands: vec![Command::TakeSnapshot { name }, command],
    }
}

/// Where a take goes: `(start_beats, offset_seconds)`. Sound captured
/// before `from_beats` (where recording was asked to start, after any
/// count-in) or before the song's start is trimmed off rather than shifting
/// the take, so everything after stays in time. None if almost nothing is
/// left. `late_ms` is the microphone's known recording delay: the take moves
/// that much earlier.
fn place_take(
    start_beats: f64,
    seconds: f64,
    tempo_bpm: f64,
    late_ms: f64,
    from_beats: f64,
) -> Option<(f64, f64)> {
    let start_beats = start_beats - late_ms / 1000.0 * tempo_bpm / 60.0;
    let keep_from = from_beats.max(0.0);
    let offset_seconds = ((keep_from - start_beats) * 60.0 / tempo_bpm).max(0.0);
    (seconds - offset_seconds >= 0.05).then_some((start_beats.max(keep_from), offset_seconds))
}

/// Starts a take on `track_id`: checks it is an audio track, starts the
/// transport after the user's count-in (with looping paused, so the take
/// runs straight through), and starts writing the microphone to a new file.
pub fn begin_take<H: Host>(
    host: &H,
    recorder: &mut AudioRecorder,
    track_id: TrackId,
) -> Result<(), String> {
    let count_in_beats = {
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
        f64::from(host.count_in_bars()) * session.project().beats_per_bar()
    };
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
    engine.play_with_count_in(count_in_beats);
    crate::diagnostics::log(&format!(
        "Recording audio on track {track_id} from beat {fallback:.2}, count-in {count_in_beats} beats"
    ));
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
    let take = result.map_err(|e| {
        crate::diagnostics::log_error(&format!("recording failed: {e}"));
        e.to_string()
    })?;
    let late_ms = host.recording_offset_ms();
    let placed = place_take(
        take.start_beats,
        take.seconds,
        session.project().tempo_bpm,
        late_ms,
        take.requested_beats,
    );
    crate::diagnostics::log(&format!(
        "Take: {:.2} s, clock says beat {:.3}, asked for beat {:.3}, delay {late_ms:.0} ms, dropped samples {}{}",
        take.seconds,
        take.start_beats,
        take.requested_beats,
        take.dropped_samples,
        if placed.is_none() {
            ", too short to keep"
        } else {
            ""
        }
    ));
    let Some((start_beats, offset_seconds)) = placed else {
        let _ = std::fs::remove_file(&take.path);
        return Ok(None);
    };
    // A new take over an old one: the new one plays, the old one is kept
    // muted (one undo step). The clip's id is the next one handed out.
    let new_clip = session.project().next_id.max(1);
    session
        .execute(Command::Batch {
            commands: vec![
                Command::AddAudioClip {
                    track_id,
                    start_beats,
                    audio: AudioRegion {
                        file: take.file,
                        file_seconds: take.seconds,
                        offset_seconds,
                        gain_db: 0.0,
                        fade_in_seconds: 0.0,
                        fade_out_seconds: 0.0,
                        source_bpm: None,
                    },
                    length_beats: None,
                    name: None,
                },
                Command::CompTake { clip_id: new_clip },
            ],
        })
        .map_err(|e| e.to_string())?;
    session.end_gesture();
    sync(host, &session);
    newest_clip(session.project(), track_id).map(Some)
}

pub(crate) fn engine<H: Host>(host: &H) -> Result<Arc<Engine>, String> {
    host.engine().ok_or_else(|| {
        "audio isn't running in Nunc Pro Tune (check the output device in the status bar)".into()
    })
}

pub(crate) fn sync<H: Host>(host: &H, session: &Session) {
    if let Some(e) = host.engine() {
        e.sync(session.project());
    }
}

/// Replaces the open project and stops playback.
pub(crate) fn replace<H: Host>(
    host: &H,
    project: Project,
    path: Option<PathBuf>,
) -> Result<(), String> {
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
    // The file keeps each plugin's own settings as they are now.
    crate::plugins::store_states(host);
    let target = match path {
        Some(p) => with_extension(p),
        None => host
            .project_path()
            .ok_or_else(|| "the project has not been saved yet; give a file path".to_owned())?,
    };
    let mut session = host.session()?;
    // Saved versions keep their recordings too.
    let files = session.project().audio_files();
    let audio = host.audio();
    let folder = audio_folder_for(&target);
    let missing_audio = if files.is_empty() {
        Vec::new()
    } else {
        audio
            .gather(files.iter().map(String::as_str), &folder)
            .map_err(|e| e.to_string())?
    };
    keep_backups(&target);
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

/// How many earlier saves are kept beside a song (`Song.nptune.bak1` is the
/// most recent).
pub const BACKUPS: usize = 3;

/// Before `target` is overwritten, keeps its current contents as
/// `.bak1`, shifting older backups along. Best effort: a backup that can't be
/// made never stops the save.
fn keep_backups(target: &Path) {
    if !target.is_file() {
        return;
    }
    let bak = |n: usize| {
        let mut s = target.as_os_str().to_owned();
        s.push(format!(".bak{n}"));
        PathBuf::from(s)
    };
    for n in (1..BACKUPS).rev() {
        if bak(n).is_file() {
            let _ = std::fs::rename(bak(n), bak(n + 1));
        }
    }
    // A copy, so the song file itself is never missing.
    let _ = std::fs::copy(target, bak(1));
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

/// Default loudness for game music exports (LUFS).
pub const DEFAULT_GAME_LUFS: f64 = -16.0;

/// Renders loops into a Godot project. Rendering runs on a snapshot, so
/// the app stays responsive.
pub fn export_godot<H: Host>(
    host: &H,
    options: &crate::protocol::GodotOptions,
) -> Result<daw_export::ExportReport, String> {
    // Plugins: render what is playing, not the last save.
    crate::plugins::store_states(host);
    let project = host.session()?.project().clone();
    let spec = godot_spec(&project, options);
    daw_export::export_to_godot(&project, &host.audio(), &spec).map_err(|e| e.to_string())
}

/// Checks what `export_godot` with `options` would write, without writing.
pub fn inspect_export<H: Host>(
    host: &H,
    options: &crate::protocol::GodotOptions,
) -> Result<daw_export::Inspection, String> {
    // Plugins: render what is playing, not the last save.
    crate::plugins::store_states(host);
    let project = host.session()?.project().clone();
    let spec = godot_spec(&project, options);
    daw_export::inspect(&project, &host.audio(), &spec).map_err(|e| e.to_string())
}

fn godot_spec(
    project: &Project,
    options: &crate::protocol::GodotOptions,
) -> daw_export::GodotExport {
    daw_export::GodotExport {
        project_dir: PathBuf::from(&options.project_dir),
        folder: options.folder.clone().unwrap_or_else(|| "music".into()),
        name: options.name.clone().unwrap_or_else(|| project.name.clone()),
        format: options.format.unwrap_or_default(),
        start_beats: options.start_beats,
        end_beats: options.end_beats,
        looped: options.looped.unwrap_or(true),
        intro: options.intro.unwrap_or(false),
        stems: options.stems.unwrap_or(false),
        bus_stems: options.bus_stems.unwrap_or(false),
        layers_resource: options.layers.unwrap_or(true),
        sections: match &options.sections {
            Some(s) => s.clone(),
            None if options.sections_from_markers == Some(true) => project
                .sections()
                .into_iter()
                .map(|s| daw_export::Section {
                    name: s.name,
                    start_beats: s.start_beats,
                    end_beats: s.end_beats,
                })
                .collect(),
            None => Vec::new(),
        },
        target_lufs: if options.normalize == Some(false) {
            None
        } else {
            Some(options.target_lufs.unwrap_or(DEFAULT_GAME_LUFS))
        },
    }
}

/// Renders the whole song (plus a 2 s tail) to a 24-bit WAV file. Returns
/// its length in seconds.
pub fn export_song_wav<H: Host>(host: &H, path: &Path) -> Result<f64, String> {
    // Plugins: render what is playing, not the last save.
    crate::plugins::store_states(host);
    let project = host.session()?.project().clone();
    export_wav(&project, &host.audio(), path, None, None, None)
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
        /// Save versions before Claude's edits, like the app does.
        pub keep_versions: bool,
        pub presets: PathBuf,
        pub last_claude: Mutex<Option<std::time::Instant>>,
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
                keep_versions: false,
                presets: scratch.path().join("presets.json"),
                last_claude: Mutex::default(),
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
        fn presets_path(&self) -> PathBuf {
            self.presets.clone()
        }
        fn library_dir(&self) -> PathBuf {
            self.presets.with_file_name("library")
        }
        fn plugin_cache_path(&self) -> PathBuf {
            self.presets.with_file_name("plugins.json")
        }
        fn plugin_folders(&self) -> Vec<PathBuf> {
            vec![self.presets.with_file_name("VST3")]
        }
        fn note_claude_edit(&self) -> Option<Option<std::time::Instant>> {
            if !self.keep_versions {
                return None;
            }
            let mut last = self.last_claude.lock().ok()?;
            Some(last.replace(std::time::Instant::now()))
        }
    }

    fn ok(r: Response) -> Value {
        r.into_result().expect("request succeeded")
    }

    #[test]
    fn a_frozen_track_sounds_the_same_until_it_is_edited() {
        let host = TestHost::default();
        ok(execute(
            &host,
            Command::CreateClip {
                track_id: 1,
                start_beats: 0.0,
                length_beats: 4.0,
                name: None,
                notes: (0..4)
                    .map(|b| daw_model::NoteInput {
                        chance: 100,
                        pitch: 60 + b as u8 * 2,
                        start_beats: f64::from(b),
                        length_beats: 0.8,
                        velocity: 100,
                        id: None,
                    })
                    .collect(),
            },
        ));
        ok(execute(
            &host,
            Command::SetTrackMixer {
                track_id: 1,
                volume_db: Some(-6.0),
                pan: Some(0.4),
                mute: None,
                solo: None,
            },
        ));
        let render = |host: &TestHost| {
            let p = host.session.lock().expect("session").project().clone();
            daw_engine::offline::render_song(&p, &host.audio, 48_000, 1.0)
        };
        let live = render(&host);
        ok(handle(&host, Request::FreezeTrack { track_id: 1 }));
        let p = host.session.lock().expect("session").project().clone();
        assert!(p.frozen_is_current(&p.tracks[0]));
        assert!(daw_engine::plays_frozen(&p, &p.tracks[0], &host.audio));
        assert!(
            host.audio
                .has(&p.tracks[0].frozen.as_ref().expect("frozen").file)
        );
        assert!(p.audio_files().iter().any(|f| f.starts_with("freeze-1-")));
        // The frozen track (with its fader and pan applied live) sounds
        // like the live one.
        let frozen = render(&host);
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        let diff: Vec<f32> = live.iter().zip(&frozen).map(|(a, b)| a - b).collect();
        assert!(
            rms(&diff) < 0.02 * rms(&live),
            "{} vs {}",
            rms(&diff),
            rms(&live)
        );
        // An edit makes it stale: it plays live again.
        ok(execute(
            &host,
            Command::TransposeNotes {
                clip_id: p.tracks[0].clips[0].id,
                semitones: 12,
                note_ids: None,
            },
        ));
        let p = host.session.lock().expect("session").project().clone();
        assert!(p.tracks[0].frozen.is_some() && !p.frozen_is_current(&p.tracks[0]));
        // Unfreeze and undo work like any edit.
        ok(execute(&host, Command::UnfreezeTrack { track_id: 1 }));
        ok(handle(&host, Request::Undo));
        assert!(
            host.session.lock().expect("session").project().tracks[0]
                .frozen
                .is_some()
        );
        // Audio tracks can't freeze.
        ok(execute(
            &host,
            Command::AddTrack {
                name: "Vox".into(),
                instrument: InstrumentKind::Audio,
                preset: None,
                index: None,
            },
        ));
        let vox = host.session.lock().expect("session").project().tracks[3].id;
        assert!(
            handle(&host, Request::FreezeTrack { track_id: vox })
                .into_result()
                .is_err()
        );
    }

    #[test]
    fn installed_plugins_are_listed_and_loaded_onto_tracks() {
        let host = TestHost::default();
        // An installed plugin described by its moduleinfo.json.
        let res = host
            .plugin_folders()
            .remove(0)
            .join("Acme Strings.vst3")
            .join("Contents")
            .join("Resources");
        std::fs::create_dir_all(&res).expect("dirs");
        std::fs::write(
            res.join("moduleinfo.json"),
            r#"{ "Factory Info": { "Vendor": "Acme" }, "Classes": [
                { "CID": "0123456789ABCDEF0123456789ABCDEF", "Category": "Audio Module Class",
                  "Name": "Acme Strings", "Sub Categories": ["Instrument", "Sampler"] },
                { "CID": "FEDCBA9876543210FEDCBA9876543210", "Category": "Audio Module Class",
                  "Name": "Acme Hall", "Sub Categories": ["Fx", "Reverb"] } ] }"#,
        )
        .expect("write");
        let listed = ok(handle(&host, Request::Plugins { rescan: true }));
        assert_eq!(listed["plugins"].as_array().map(Vec::len), Some(2));
        // Remembered without rescanning.
        let again = ok(handle(&host, Request::Plugins { rescan: false }));
        assert_eq!(again["plugins"], listed["plugins"]);

        ok(handle(
            &host,
            Request::LoadPlugin {
                track_id: 1,
                uid: "0123456789abcdef0123456789abcdef".into(),
            },
        ));
        {
            let s = host.session.lock().expect("session");
            let inst = &s.project().track(1).expect("track").instrument;
            assert_eq!(inst.kind, InstrumentKind::Plugin);
            assert_eq!(
                inst.plugin.as_ref().map(|p| p.name.as_str()),
                Some("Acme Strings")
            );
        }
        // Effects aren't instruments; unknown ids are explained.
        for uid in [
            "FEDCBA9876543210FEDCBA9876543210",
            "00000000000000000000000000000000",
        ] {
            let r = handle(
                &host,
                Request::LoadPlugin {
                    track_id: 2,
                    uid: uid.into(),
                },
            );
            assert!(!r.ok, "{uid}");
        }
        // Without audio running there's no live plugin to ask.
        assert!(!handle(&host, Request::PluginParams { track_id: 1 }).ok);
        // Loading is one undo step.
        ok(handle(&host, Request::Undo));
        let s = host.session.lock().expect("session");
        assert_eq!(
            s.project().track(1).expect("t").instrument.kind,
            InstrumentKind::Synth
        );
    }

    #[test]
    fn claude_sees_the_sample_library() {
        let host = TestHost::default();
        let listed = ok(handle(&host, Request::SampleLibrary));
        let instruments = listed["instruments"].as_array().expect("list");
        assert!(
            instruments
                .iter()
                .any(|i| i["id"] == "cello" && i["installed"].is_null())
        );
        assert_eq!(listed["credits_this_song_needs"], json!([]));
        assert!(!handle(&host, Request::DownloadSamplePack { id: "kazoo".into() }).ok);
    }

    #[test]
    fn a_saved_preset_can_be_loaded_onto_another_track_and_undone() {
        let host = TestHost::default();
        ok(execute(
            &host,
            Command::SetInstrumentParam {
                track_id: 1,
                param: "filter.cutoff_hz".into(),
                value: 640.0,
            },
        ));
        ok(handle(
            &host,
            Request::SavePreset {
                track_id: 1,
                name: "Muffled Keys".into(),
            },
        ));
        let listed = ok(handle(&host, Request::UserPresets));
        assert_eq!(listed["presets"][0]["name"], "Muffled Keys");
        // Track 2 is a synth too (the bass).
        ok(handle(
            &host,
            Request::LoadUserPreset {
                track_id: 2,
                name: "muffled keys".into(),
            },
        ));
        {
            let session = host.session.lock().expect("session");
            let bass = &session.project().tracks[1].instrument;
            assert_eq!(bass.preset, "Muffled Keys");
            assert_eq!(bass.params["filter.cutoff_hz"], 640.0);
        }
        ok(handle(&host, Request::Undo));
        let session = host.session.lock().expect("session");
        assert_eq!(session.project().tracks[1].instrument.preset, "Fat Bass");
        drop(session);
        // Drum tracks don't get synth presets.
        let err = handle(
            &host,
            Request::LoadUserPreset {
                track_id: 3,
                name: "Muffled Keys".into(),
            },
        )
        .into_result()
        .expect_err("wrong kind");
        assert!(err.contains("no saved drums preset"), "{err}");
    }

    #[test]
    fn godot_sections_can_come_from_the_songs_markers() {
        let host = TestHost::default();
        ok(execute(
            &host,
            Command::CreateClip {
                track_id: 1,
                start_beats: 0.0,
                length_beats: 16.0,
                name: None,
                notes: (0..16)
                    .map(|b| daw_model::NoteInput {
                        chance: 100,
                        pitch: 60,
                        start_beats: f64::from(b),
                        length_beats: 0.5,
                        velocity: 100,
                        id: None,
                    })
                    .collect(),
            },
        ));
        for (name, beat) in [("Explore", 0.0), ("Combat", 8.0)] {
            ok(execute(
                &host,
                Command::AddMarker {
                    name: name.into(),
                    start_beats: beat,
                },
            ));
        }
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("project.godot"), "config_version=5\n").expect("godot");
        let options: crate::protocol::GodotOptions = serde_json::from_value(json!({
            "project_dir": dir.path().to_string_lossy(),
            "name": "Dungeon",
            "sections_from_markers": true,
        }))
        .expect("options");
        let report = export_godot(&host, &options).expect("export");
        for f in [
            "res://music/dungeon_explore.ogg",
            "res://music/dungeon_combat.ogg",
            "res://music/dungeon_sections.tres",
        ] {
            assert!(
                report.files.iter().any(|x| x == f),
                "{f} in {:?}",
                report.files
            );
        }
        let tres =
            std::fs::read_to_string(dir.path().join("music/dungeon_sections.tres")).expect("tres");
        assert!(
            tres.contains("clip_0/name = &\"Explore\"")
                && tres.contains("clip_1/name = &\"Combat\"")
        );
    }

    #[test]
    fn claudes_first_change_after_a_while_saves_a_version_first() {
        let host = TestHost {
            keep_versions: true,
            ..TestHost::default()
        };
        // The user's own work.
        host.session
            .lock()
            .expect("session")
            .execute(Command::CreateClip {
                track_id: 1,
                start_beats: 0.0,
                length_beats: 4.0,
                name: Some("Mine".into()),
                notes: Vec::new(),
            })
            .expect("clip");
        let snapshots = |h: &TestHost| {
            h.session
                .lock()
                .expect("session")
                .project()
                .snapshots
                .iter()
                .map(|s| (s.name.clone(), s.song.tempo_bpm))
                .collect::<Vec<_>>()
        };
        ok(execute(&host, Command::SetTempo { bpm: 90.0 }));
        assert_eq!(snapshots(&host), [(BEFORE_CLAUDE.to_owned(), 120.0)]);
        // Claude keeps working: no more versions.
        ok(execute(&host, Command::SetTempo { bpm: 95.0 }));
        assert_eq!(snapshots(&host).len(), 1);
        // After a quiet spell, the next change saves another one.
        *host.last_claude.lock().expect("time") =
            std::time::Instant::now().checked_sub(CLAUDE_QUIET + std::time::Duration::from_secs(1));
        ok(execute(&host, Command::SetTempo { bpm: 100.0 }));
        assert_eq!(
            snapshots(&host),
            [
                (BEFORE_CLAUDE.to_owned(), 120.0),
                (format!("{BEFORE_CLAUDE} (2)"), 95.0)
            ]
        );
        // One undo takes back the change and its version together.
        ok(handle(&host, Request::Undo));
        let session = host.session.lock().expect("session");
        assert_eq!(session.project().tempo_bpm, 95.0);
        assert_eq!(session.project().snapshots.len(), 1);
    }

    #[test]
    fn saving_keeps_the_last_three_versions_as_backups() {
        let dir = tempfile::tempdir().expect("tmp");
        let song = dir.path().join("Song.nptune");
        let host = TestHost::default();
        let bak = |n: usize| dir.path().join(format!("Song.nptune.bak{n}"));
        for bpm in [101.0, 102.0, 103.0, 104.0, 105.0] {
            ok(execute(&host, Command::SetTempo { bpm }));
            save_project(&host, Some(&song)).expect("save");
        }
        let tempo = |p: &Path| daw_model::load_project(p).expect("load").tempo_bpm;
        assert_eq!(tempo(&song), 105.0);
        assert_eq!(tempo(&bak(1)), 104.0);
        assert_eq!(tempo(&bak(2)), 103.0);
        assert_eq!(tempo(&bak(3)), 102.0);
        assert!(!bak(4).exists());
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
                    chance: 100,
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
                    chance: 100,
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
                    chance: 100,
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

    #[test]
    fn takes_that_start_before_the_song_are_trimmed_not_shifted() {
        // Starts on beat 8: placed as is.
        assert_eq!(place_take(8.0, 4.0, 120.0, 0.0, 0.0), Some((8.0, 0.0)));
        // Capture began 0.05 beats (25 ms at 120 BPM) before beat 0: the
        // first 25 ms are skipped so beat 1 of the take is beat 1 of the song.
        let (start, offset) = place_take(-0.05, 4.0, 120.0, 0.0, 0.0).expect("kept");
        assert_eq!(start, 0.0);
        assert!((offset - 0.025).abs() < 1e-12);
        // Nothing left after trimming.
        assert_eq!(place_take(-1.0, 0.5, 120.0, 0.0, 0.0), None);
        assert_eq!(place_take(0.0, 0.01, 120.0, 0.0, 0.0), None);
    }

    #[test]
    fn a_known_recording_delay_moves_takes_earlier() {
        // A Bluetooth headset delivers sound 180 ms late: a take the clock
        // puts at beat 8.36 (at 120 BPM, 0.36 beats = 180 ms) was played
        // on beat 8.
        let (start, offset) = place_take(8.36, 4.0, 120.0, 180.0, 0.0).expect("kept");
        assert!((start - 8.0).abs() < 1e-9 && offset == 0.0);
        // Moved before the song start: trimmed, as usual.
        let (start, offset) = place_take(0.2, 4.0, 120.0, 180.0, 0.0).expect("kept");
        assert_eq!(start, 0.0);
        assert!((offset - 0.08).abs() < 1e-9);
    }

    #[test]
    fn the_count_in_is_trimmed_off_a_take() {
        // Recording from beat 8 with a one-bar count-in: capture began at
        // beat 4, so the first 4 beats (2 s at 120 BPM) are skipped.
        let (start, offset) = place_take(4.0, 6.0, 120.0, 0.0, 8.0).expect("kept");
        assert_eq!(start, 8.0);
        assert!((offset - 2.0).abs() < 1e-12);
        // With a recording delay, the corrected take is trimmed the same way.
        let (start, offset) = place_take(4.36, 6.0, 120.0, 180.0, 8.0).expect("kept");
        assert_eq!(start, 8.0);
        assert!((offset - 2.0).abs() < 1e-9);
        // Stopped during the count-in: nothing to keep.
        assert_eq!(place_take(4.0, 1.5, 120.0, 0.0, 8.0), None);
    }
}
