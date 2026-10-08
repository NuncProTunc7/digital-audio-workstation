//! Nunc Pro Tune desktop shell: connects the React UI to the Rust engine.
//!
//! Two kinds of calls arrive from the UI:
//! - **Project edits** arrive as `daw_model::Command`s (the same type Claude
//!   sends over MCP). They are undoable and saved.
//! - **Live actions** (playing notes, play/stop, metronome, device choice)
//!   change what you hear right now but not the song itself.
//!
//! Claude reaches the same state through the local control server
//! (`daw_control`), started at launch; see [`AppState`]'s `Host` impl.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use daw_control::autosave::{self, AutosaveSlot, Recoverable};
use daw_control::claude_setup;
use daw_control::compare::{self, Comparison, Side};
use daw_control::diagnostics::{self, DiagnosticInfo};
use daw_control::settings::{self, Settings};
use daw_control::{CalibrationResult, ControlServer, Host, RecordingDelay};
use daw_engine::capture::AudioRecorder;
use daw_engine::device::{self, AudioInput, AudioOutput};
use daw_engine::midi::{MidiEvent, MidiInputs};
use daw_engine::{AudioPool, Engine};
use daw_model::{ClipId, Command, InstrumentKind, Project, Session, TrackId};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

/// The engine currently driving the sound card, shared with the MIDI thread.
type EngineSlot = Arc<RwLock<Option<Arc<Engine>>>>;

struct AppState {
    session: Mutex<Session>,
    engine: EngineSlot,
    output: Mutex<Option<AudioOutput>>,
    /// The output device the user picked (None = system default).
    output_choice: Mutex<Option<String>>,
    /// Overload count last seen, and when it last went up.
    overload_watch: Mutex<(u32, Option<Instant>)>,
    /// When Claude last changed the song (for "Before Claude's changes").
    last_claude_edit: Mutex<Option<Instant>>,
    /// A/B listening against a saved version, and which side is playing.
    comparison: Mutex<Option<(Comparison, Side)>>,
    audio_error: Mutex<Option<String>>,
    midi: Mutex<Option<MidiInputs>>,
    /// Track that MIDI keyboards play.
    selected_track: Arc<AtomicU32>,
    metronome_on: AtomicBool,
    /// Where the open project was last saved or opened from.
    project_path: Mutex<Option<PathBuf>>,
    /// Track being recorded onto, while recording notes.
    recording_track: Mutex<Option<TrackId>>,
    /// The project's audio files.
    audio: Arc<AudioPool>,
    /// Microphone, open while an audio track is selected or recording.
    input: Mutex<Option<AudioInput>>,
    recorder: Mutex<Option<AudioRecorder>>,
    /// Chosen input (None = system default).
    input_device: Mutex<Option<String>>,
    input_error: Mutex<Option<String>>,
    /// Audio track being recorded onto, while recording audio.
    audio_take: Mutex<Option<TrackId>>,
    /// For telling the UI about changes made by Claude.
    app: OnceLock<AppHandle>,
    /// Keeps the control server (Claude's way in) alive.
    control: Mutex<Option<ControlServer>>,
    control_error: Mutex<Option<String>>,
    /// When Claude last talked to the app, and what it did recently.
    last_remote: Mutex<Option<Instant>>,
    remote_log: Mutex<VecDeque<RemoteActivity>>,
    /// This computer's settings (recording delays).
    settings: Mutex<Settings>,
    /// This run's autosave of unsaved work.
    autosave: Mutex<AutosaveSlot>,
    /// Unsaved work left by a run that didn't close properly, until the
    /// user recovers or declines it.
    recoverable: Mutex<Option<Recoverable>>,
}

/// One thing Claude did, for the activity list.
#[derive(Clone, Serialize)]
struct RemoteActivity {
    description: String,
    /// Milliseconds since the Unix epoch.
    at_ms: u64,
}

const REMOTE_LOG_LEN: usize = 30;

impl Default for AppState {
    fn default() -> Self {
        Self {
            session: Mutex::new(Session::default()),
            engine: Arc::new(RwLock::new(None)),
            output: Mutex::new(None),
            output_choice: Mutex::new(None),
            overload_watch: Mutex::new((0, None)),
            last_claude_edit: Mutex::new(None),
            comparison: Mutex::new(None),
            audio_error: Mutex::new(None),
            midi: Mutex::new(None),
            selected_track: Arc::new(AtomicU32::new(1)),
            metronome_on: AtomicBool::new(true),
            project_path: Mutex::new(None),
            recording_track: Mutex::new(None),
            audio: Arc::new(AudioPool::new(daw_control::unsaved_audio_dir())),
            input: Mutex::new(None),
            recorder: Mutex::new(None),
            input_device: Mutex::new(None),
            input_error: Mutex::new(None),
            audio_take: Mutex::new(None),
            app: OnceLock::new(),
            control: Mutex::new(None),
            control_error: Mutex::new(None),
            last_remote: Mutex::new(None),
            remote_log: Mutex::new(VecDeque::new()),
            settings: Mutex::new(Settings::load(&settings::settings_path())),
            autosave: Mutex::new(AutosaveSlot::new(autosave::autosave_dir())),
            recoverable: Mutex::new(None),
        }
    }
}

impl daw_control::Host for AppState {
    fn session(&self) -> Result<MutexGuard<'_, Session>, String> {
        AppState::session(self)
    }

    fn engine(&self) -> Option<Arc<Engine>> {
        AppState::engine(self)
    }

    fn project_changed(&self, description: &str) {
        // The engine now plays the edited song; A/B is over.
        self.end_comparison();
        if let Ok(mut log) = self.remote_log.lock() {
            if log.len() == REMOTE_LOG_LEN {
                log.pop_front();
            }
            let at_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64);
            log.push_back(RemoteActivity {
                description: description.to_owned(),
                at_ms,
            });
        }
        if let Some(app) = self.app.get() {
            let _ = app.emit("project-changed", description.to_owned());
        }
    }

    fn project_path(&self) -> Option<PathBuf> {
        self.project_path.lock().ok().and_then(|p| p.clone())
    }

    fn set_project_path(&self, path: Option<PathBuf>) {
        if let Ok(mut p) = self.project_path.lock() {
            *p = path;
        }
    }

    fn audio(&self) -> Arc<AudioPool> {
        Arc::clone(&self.audio)
    }

    fn start_audio_recording(&self, track_id: TrackId) -> Result<(), String> {
        self.ensure_input()?;
        let mut recorder = self
            .recorder
            .lock()
            .map_err(|_| "recording state is unavailable")?;
        let recorder = recorder
            .as_mut()
            .ok_or("no microphone is open; check the input in the status bar")?;
        daw_control::begin_take(self, recorder, track_id)?;
        if let Ok(mut t) = self.audio_take.lock() {
            *t = Some(track_id);
        }
        Ok(())
    }

    fn stop_audio_recording(&self) -> Result<Option<ClipId>, String> {
        let track = self
            .audio_take
            .lock()
            .ok()
            .and_then(|mut t| t.take())
            .ok_or("nothing is recording")?;
        let clip = {
            let mut recorder = self
                .recorder
                .lock()
                .map_err(|_| "recording state is unavailable")?;
            match recorder.as_mut() {
                Some(r) => daw_control::end_take(self, r, track)?,
                None => None,
            }
        };
        if let Some(e) = self.engine() {
            e.stop();
        }
        Ok(clip)
    }

    fn metronome_on(&self) -> bool {
        self.metronome_on.load(Ordering::Relaxed)
    }

    fn count_in_bars(&self) -> u32 {
        self.settings.lock().map_or(0, |s| s.count_in_bars)
    }

    fn note_claude_edit(&self) -> Option<Option<Instant>> {
        let mut last = self.last_claude_edit.lock().ok()?;
        Some(last.replace(Instant::now()))
    }

    fn diagnostic_info(&self) -> DiagnosticInfo {
        let snap = self.engine().map(|e| e.status()).unwrap_or_default();
        let (output_device, sample_rate_hz) = self
            .output
            .lock()
            .ok()
            .and_then(|o| {
                o.as_ref()
                    .map(|o| (Some(o.device_name().to_owned()), Some(o.sample_rate_hz())))
            })
            .unwrap_or((None, None));
        let claude = match (
            self.control.lock().is_ok_and(|c| c.is_some()),
            self.last_remote.lock().ok().and_then(|t| *t),
        ) {
            (false, _) => self
                .control_error
                .lock()
                .ok()
                .and_then(|e| e.clone())
                .unwrap_or_else(|| "not listening".into()),
            (true, None) => "listening, no requests yet".into(),
            (true, Some(t)) => format!("listening, last request {} s ago", t.elapsed().as_secs()),
        };
        DiagnosticInfo {
            app_version: env!("CARGO_PKG_VERSION").into(),
            os: os_version(),
            output_device,
            sample_rate_hz,
            buffer_setting: self.settings.lock().ok().and_then(|s| s.buffer_frames),
            buffer_frames: snap.buffer_frames,
            audio_error: self.audio_error.lock().ok().and_then(|e| e.clone()),
            input_device: self.input_name(),
            input_open: self.input.lock().is_ok_and(|i| i.is_some()),
            input_error: self.input_error.lock().ok().and_then(|e| e.clone()),
            recording_offset_ms: self.recording_offset_ms(),
            cpu_load: snap.cpu_load,
            cpu_peak: snap.cpu_peak,
            overloads: snap.overloads,
            midi_inputs: self
                .midi
                .lock()
                .ok()
                .and_then(|m| m.as_ref().map(|m| m.port_names().to_vec()))
                .unwrap_or_default(),
            count_in_bars: self.count_in_bars(),
            claude,
        }
    }

    fn set_count_in_bars(&self, bars: u32) -> Result<u32, String> {
        let mut s = self
            .settings
            .lock()
            .map_err(|_| "settings are unavailable")?;
        let bars = s.set_count_in_bars(bars);
        diagnostics::log(&format!("Count-in: {bars} bar(s)"));
        s.save(&settings::settings_path())?;
        Ok(bars)
    }

    fn recording_offset_ms(&self) -> f64 {
        let device = self.input_name();
        self.settings
            .lock()
            .ok()
            .zip(device)
            .map_or(0.0, |(s, d)| s.recording_offset_ms(&d))
    }

    fn recording_delay(&self, set_ms: Option<f64>) -> Result<RecordingDelay, String> {
        let device = self.input_name();
        if let Some(ms) = set_ms {
            let d = device
                .as_deref()
                .ok_or("there is no microphone to set a delay for")?;
            let mut s = self
                .settings
                .lock()
                .map_err(|_| "settings are unavailable")?;
            s.set_recording_offset_ms(d, ms);
            s.save(&settings::settings_path())?;
        }
        Ok(RecordingDelay {
            offset_ms: self.recording_offset_ms(),
            bluetooth: device.as_deref().is_some_and(settings::looks_bluetooth),
            device,
        })
    }

    fn calibrate_recording(&self) -> Result<CalibrationResult, String> {
        self.ensure_input()?;
        let device = self.input_name();
        let measured = {
            let mut recorder = self
                .recorder
                .lock()
                .map_err(|_| "recording state is unavailable")?;
            let recorder = recorder
                .as_mut()
                .ok_or("no microphone is open; check the input in the Audio tab")?;
            daw_control::calibrate::calibrate_recording(self, recorder)?
        };
        let saved = measured.is_reliable() && device.is_some();
        if saved {
            self.recording_delay(Some(measured.offset_ms))?;
        }
        Ok(CalibrationResult {
            measured,
            saved,
            device,
        })
    }
}

impl AppState {
    fn engine(&self) -> Option<Arc<Engine>> {
        self.engine.read().ok().and_then(|e| e.clone())
    }

    fn session(&self) -> Result<MutexGuard<'_, Session>, String> {
        self.session
            .lock()
            .map_err(|_| "project state is unavailable".to_owned())
    }

    /// (Re)opens audio output on `device_name` (None = system default) with
    /// the buffer size from settings. The playhead stays where it was. Keeps
    /// the app running without sound on failure.
    fn start_audio(&self, device_name: Option<&str>) -> Result<(), String> {
        if let Ok(mut choice) = self.output_choice.lock() {
            *choice = device_name.map(str::to_owned);
        }
        let buffer_frames = self.settings.lock().ok().and_then(|s| s.buffer_frames);
        let position = self.engine().map(|e| e.status().position_beats);
        let project = self.session()?.project().clone();
        let mut output = self
            .output
            .lock()
            .map_err(|_| "audio state is unavailable".to_owned())?;
        // Close the old stream first: some drivers allow only one at a time.
        *output = None;
        if let Ok(mut slot) = self.engine.write() {
            *slot = None;
        }
        let result = AudioOutput::start(
            device_name,
            buffer_frames,
            &project,
            Arc::clone(&self.audio),
        );
        let mut error = self
            .audio_error
            .lock()
            .map_err(|_| "audio state is unavailable")?;
        match result {
            Ok((engine, new_output)) => {
                diagnostics::log(&format!(
                    "Audio output: {}, {} Hz, buffer {}",
                    new_output.device_name(),
                    new_output.sample_rate_hz(),
                    new_output
                        .buffer_frames()
                        .map_or("default".to_owned(), |f| f.to_string())
                ));
                if buffer_frames.is_some() && new_output.buffer_frames().is_none() {
                    diagnostics::log_error(&format!(
                        "the sound card refused a {} frame buffer; using its default",
                        buffer_frames.unwrap_or(0)
                    ));
                }
                engine.set_metronome(self.metronome_on.load(Ordering::Relaxed));
                if let Some(beats) = position {
                    engine.locate(beats);
                }
                if let Ok(mut slot) = self.engine.write() {
                    *slot = Some(Arc::new(engine));
                }
                *output = Some(new_output);
                *error = None;
                Ok(())
            }
            Err(e) => {
                let message = e.to_string();
                diagnostics::log_error(&format!("audio output failed: {message}"));
                *error = Some(message.clone());
                Err(message)
            }
        }
    }

    /// Forgets any A/B comparison (the caller resyncs the engine).
    fn end_comparison(&self) {
        if let Ok(mut c) = self.comparison.lock() {
            *c = None;
        }
    }

    /// Opens the chosen microphone if it isn't open yet.
    fn ensure_input(&self) -> Result<(), String> {
        let mut input = self
            .input
            .lock()
            .map_err(|_| "input state is unavailable")?;
        if input.is_some() {
            return Ok(());
        }
        let name = self.input_device.lock().ok().and_then(|n| n.clone());
        let result = AudioInput::start(name.as_deref());
        let mut error = self
            .input_error
            .lock()
            .map_err(|_| "input state is unavailable")?;
        match result {
            Ok((stream, recorder)) => {
                diagnostics::log(&format!(
                    "Microphone open: {}, {} Hz",
                    stream.device_name(),
                    stream.sample_rate_hz()
                ));
                *input = Some(stream);
                if let Ok(mut r) = self.recorder.lock() {
                    *r = Some(recorder);
                }
                *error = None;
                Ok(())
            }
            Err(e) => {
                let message = e.to_string();
                diagnostics::log_error(&format!("microphone failed: {message}"));
                *error = Some(message.clone());
                Err(message)
            }
        }
    }

    /// The microphone in use: the open one, else the chosen or default one.
    fn input_name(&self) -> Option<String> {
        let open = self
            .input
            .lock()
            .ok()
            .and_then(|i| i.as_ref().map(|i| i.device_name().to_owned()));
        open.or_else(|| self.input_device.lock().ok().and_then(|n| n.clone()))
            .or_else(device::default_input_device_name)
    }

    /// Writes unsaved work to the autosave if it changed.
    fn autosave(&self) {
        if let Ok(mut slot) = self.autosave.lock() {
            let _ = slot.update(self);
        }
    }

    /// Forgets the autosave once the work is saved or deliberately replaced.
    fn discard_autosave(&self) {
        if let Ok(mut slot) = self.autosave.lock() {
            slot.discard();
        }
    }

    /// Closes the microphone (unless a take is being recorded).
    fn close_input(&self) {
        if self.audio_take.lock().is_ok_and(|t| t.is_some()) {
            return;
        }
        if let Ok(mut r) = self.recorder.lock() {
            *r = None;
        }
        if let Ok(mut i) = self.input.lock() {
            *i = None;
        }
    }
}

/// What the UI needs to draw the project, undo/redo, and the title bar.
#[derive(Serialize)]
struct ProjectView {
    project: Project,
    can_undo: bool,
    can_redo: bool,
    /// Unsaved changes since the last save or open.
    dirty: bool,
    file_path: Option<String>,
    /// Audio files clips refer to that can't be found (those clips are silent).
    missing_audio: Vec<String>,
    /// Frozen tracks whose rendering is up to date (others play live).
    frozen_current: Vec<TrackId>,
}

fn view(state: &AppState, session: &Session) -> ProjectView {
    let mut missing_audio: Vec<String> = session
        .project()
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .filter_map(|c| c.audio.as_ref())
        .filter(|a| !state.audio.has(&a.file))
        .map(|a| a.file.clone())
        .collect();
    missing_audio.dedup();
    let project = session.project();
    let frozen_current = project
        .tracks
        .iter()
        .filter(|t| t.frozen.is_some() && daw_engine::plays_frozen(project, t, &state.audio))
        .map(|t| t.id)
        .collect();
    ProjectView {
        missing_audio,
        frozen_current,
        project: session.project().clone(),
        can_undo: session.can_undo(),
        can_redo: session.can_redo(),
        dirty: session.is_dirty(),
        file_path: state
            .project_path
            .lock()
            .ok()
            .and_then(|p| p.as_ref().map(|p| p.display().to_string())),
    }
}

/// Pushes the current project to the audio engine and returns the view.
fn after_edit(state: &AppState, session: &Session) -> ProjectView {
    state.end_comparison();
    if let Some(engine) = state.engine() {
        engine.sync(session.project());
    }
    view(state, session)
}

#[derive(Serialize)]
struct AppInfo {
    name: &'static str,
    version: &'static str,
    license: &'static str,
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        name: "Nunc Pro Tune",
        version: env!("CARGO_PKG_VERSION"),
        license: env!("CARGO_PKG_LICENSE"),
    }
}

// ---- Project edits ----

#[tauri::command]
fn get_project(state: State<'_, AppState>) -> Result<ProjectView, String> {
    let session = state.session()?;
    Ok(view(&state, &session))
}

// ---- Files ----

// The same functions serve the UI and Claude, so both behave identically.

/// Starts a fresh project with the default Keys, Bass, and Drums tracks.
#[tauri::command]
fn new_project(state: State<'_, AppState>) -> Result<ProjectView, String> {
    daw_control::new_project(&*state)?;
    diagnostics::log("New song");
    state.discard_autosave();
    get_project(state)
}

#[tauri::command]
fn open_project(state: State<'_, AppState>, path: String) -> Result<ProjectView, String> {
    daw_control::open_project(&*state, Path::new(&path))?;
    diagnostics::log(&format!("Opened {path}"));
    state.discard_autosave();
    get_project(state)
}

/// Saves to `path`, or to the current file when `path` is omitted.
#[tauri::command]
fn save_project(state: State<'_, AppState>, path: Option<String>) -> Result<ProjectView, String> {
    daw_control::save_project(&*state, path.as_deref().map(Path::new))?;
    diagnostics::log("Saved");
    state.discard_autosave();
    get_project(state)
}

/// Unsaved work from a run that didn't close properly, if any.
#[tauri::command]
fn recovery_check(state: State<'_, AppState>) -> Option<Recoverable> {
    state.recoverable.lock().ok().and_then(|r| r.clone())
}

/// Opens the recovered work (`recover`), or deletes it.
#[tauri::command]
fn recovery_resolve(state: State<'_, AppState>, recover: bool) -> Result<ProjectView, String> {
    let found = state.recoverable.lock().ok().and_then(|mut r| r.take());
    if let Some(found) = found {
        if recover {
            autosave::recover(&*state, &found)?;
        } else {
            autosave::discard_recoverable(&found);
        }
    }
    get_project(state)
}

#[tauri::command]
fn execute(state: State<'_, AppState>, command: Command) -> Result<ProjectView, String> {
    let mut session = state.session()?;
    session.execute(command).map_err(|e| e.to_string())?;
    Ok(after_edit(&state, &session))
}

/// Marks the end of a slider drag, so the next edit is a new undo step.
#[tauri::command]
fn end_gesture(state: State<'_, AppState>) -> Result<(), String> {
    state.session()?.end_gesture();
    Ok(())
}

#[tauri::command]
fn undo(state: State<'_, AppState>) -> Result<ProjectView, String> {
    let mut session = state.session()?;
    session.undo();
    Ok(after_edit(&state, &session))
}

#[tauri::command]
fn redo(state: State<'_, AppState>) -> Result<ProjectView, String> {
    let mut session = state.session()?;
    session.redo();
    Ok(after_edit(&state, &session))
}

#[tauri::command]
fn instrument_catalog() -> daw_instruments::Catalog {
    daw_instruments::catalog()
}

// ---- Live actions ----

#[tauri::command]
fn note_on(state: State<'_, AppState>, track_id: TrackId, note: u8, velocity: f32) {
    if let Some(engine) = state.engine() {
        engine.note_on(track_id, note, velocity);
    }
}

#[tauri::command]
fn note_off(state: State<'_, AppState>, track_id: TrackId, note: u8) {
    if let Some(engine) = state.engine() {
        engine.note_off(track_id, note);
    }
}

#[tauri::command]
fn all_notes_off(state: State<'_, AppState>) {
    if let Some(engine) = state.engine() {
        engine.all_notes_off();
    }
}

/// Chooses which track a MIDI keyboard plays.
#[tauri::command]
fn select_track(state: State<'_, AppState>, track_id: TrackId) {
    state.selected_track.store(track_id, Ordering::Relaxed);
}

#[tauri::command]
fn play(state: State<'_, AppState>) {
    if let Some(engine) = state.engine() {
        engine.play();
    }
}

#[tauri::command]
fn stop(state: State<'_, AppState>) {
    if let Some(engine) = state.engine() {
        engine.stop();
    }
}

/// Moves the playhead to a position in beats.
#[tauri::command]
fn locate(state: State<'_, AppState>, beats: f64) {
    if let Some(engine) = state.engine() {
        engine.locate(beats);
    }
}

/// Starts recording what you play on `track_id` (and starts playback).
#[tauri::command]
fn record_start(state: State<'_, AppState>, track_id: TrackId) -> Result<(), String> {
    let engine = state.engine().ok_or("no audio output; check the device")?;
    let is_audio = state
        .session()?
        .project()
        .track(track_id)
        .ok_or_else(|| format!("there is no track with id {track_id}"))?
        .instrument
        .kind
        .is_audio();
    if is_audio {
        return state.start_audio_recording(track_id);
    }
    let count_in_beats =
        f64::from(state.count_in_bars()) * state.session()?.project().beats_per_bar();
    if let Ok(mut r) = state.recording_track.lock() {
        *r = Some(track_id);
    }
    engine.start_recording(track_id);
    engine.play_with_count_in(count_in_beats);
    Ok(())
}

/// Stops recording and playback, and turns what was played into a clip.
#[tauri::command]
fn record_stop(state: State<'_, AppState>) -> Result<ProjectView, String> {
    if state.audio_take.lock().is_ok_and(|t| t.is_some()) {
        state.stop_audio_recording()?;
        return get_project(state);
    }
    let track = state.recording_track.lock().ok().and_then(|mut r| r.take());
    let mut session = state.session()?;
    let Some(engine) = state.engine() else {
        return Ok(view(&state, &session));
    };
    let stop_beats = engine.status().position_beats;
    let events = engine.stop_recording();
    engine.stop();
    if let Some(track_id) = track
        && let Some(clip) = daw_engine::recording::clip_from_recording(
            &events,
            stop_beats,
            session.project().beats_per_bar(),
        )
    {
        session
            .execute(Command::CreateClip {
                track_id,
                start_beats: clip.start_beats,
                length_beats: clip.length_beats,
                name: Some("Recording".into()),
                notes: clip.notes,
            })
            .map_err(|e| e.to_string())?;
        session.end_gesture();
    }
    Ok(after_edit(&state, &session))
}

#[tauri::command]
fn set_metronome(state: State<'_, AppState>, on: bool) {
    state.metronome_on.store(on, Ordering::Relaxed);
    if let Some(engine) = state.engine() {
        engine.set_metronome(on);
    }
}

/// Sets how many bars of clicks play before recording starts (0-2).
#[tauri::command]
fn set_count_in(state: State<'_, AppState>, bars: u32) -> Result<u32, String> {
    state.set_count_in_bars(bars)
}

/// Freezes a track: renders its sound and plays that instead (saves CPU).
/// Renders, so off the main thread.
#[tauri::command(async)]
fn freeze_track(state: State<'_, AppState>, track_id: TrackId) -> Result<ProjectView, String> {
    daw_control::freeze_track(&*state, track_id)?;
    get_project(state)
}

/// Checks a Godot export without writing it. Renders, so off the main
/// thread.
#[tauri::command(async)]
fn inspect_export(
    state: State<'_, AppState>,
    options: daw_control::GodotOptions,
) -> Result<daw_export::Inspection, String> {
    daw_control::inspect_export(&*state, &options)
}

/// Plays the end of a loop into its start, over and over (Stop ends it).
#[tauri::command]
fn audition_seam(
    state: State<'_, AppState>,
    start_beats: f64,
    end_beats: f64,
) -> Result<(), String> {
    let engine = state.engine().ok_or("no audio output; check the device")?;
    let bar = state.session()?.project().beats_per_bar();
    engine.audition_seam(start_beats, end_beats, bar);
    Ok(())
}

/// Free instruments the app can download, and which are installed.
#[tauri::command]
fn sample_library() -> Vec<daw_control::library::PackState> {
    daw_control::library::list(&daw_control::library::library_dir())
}

/// Starts downloading a library instrument (watch it with sample_library).
#[tauri::command]
fn download_sample_pack(id: String) -> Result<(), String> {
    daw_control::library::start_download(&daw_control::library::library_dir(), &id)
}

/// The user's own presets.
#[tauri::command]
fn user_presets(state: State<'_, AppState>) -> Vec<daw_control::presets::UserPreset> {
    daw_control::presets::load(&state.presets_path())
}

/// Saves a track's sound as the user's preset `name`.
#[tauri::command]
fn save_preset(
    state: State<'_, AppState>,
    track_id: TrackId,
    name: String,
) -> Result<Vec<daw_control::presets::UserPreset>, String> {
    daw_control::save_user_preset(&*state, track_id, &name)
}

/// Deletes one of the user's presets.
#[tauri::command]
fn delete_preset(
    state: State<'_, AppState>,
    kind: InstrumentKind,
    name: String,
) -> Result<Vec<daw_control::presets::UserPreset>, String> {
    daw_control::presets::delete(&state.presets_path(), kind, &name)
}

/// Gives a track one of the user's presets (one undo step).
#[tauri::command]
fn load_user_preset(
    state: State<'_, AppState>,
    track_id: TrackId,
    name: String,
) -> Result<ProjectView, String> {
    daw_control::load_user_preset(&*state, track_id, &name)?;
    get_project(state)
}

/// Game preview: loops section `index` with every layer up.
#[tauri::command]
fn preview_start(
    state: State<'_, AppState>,
    index: usize,
) -> Result<daw_control::game_preview::PreviewPlan, String> {
    state.end_comparison();
    daw_control::game_preview::start(&*state, index)
}

/// Game preview: changes to section `index` at the next bar line.
#[tauri::command]
fn preview_switch(state: State<'_, AppState>, index: usize) -> Result<(), String> {
    daw_control::game_preview::switch(&*state, index)
}

/// Game preview: fades a track in or out over `fade_beats`.
#[tauri::command]
fn preview_layer(
    state: State<'_, AppState>,
    track_id: TrackId,
    on: bool,
    fade_beats: f64,
) -> Result<(), String> {
    daw_control::game_preview::fade(&*state, track_id, on, fade_beats)
}

/// Ends the game preview.
#[tauri::command]
fn preview_stop(state: State<'_, AppState>) -> Result<(), String> {
    daw_control::game_preview::stop(&*state)
}

/// Measures the song and saved version `snapshot_id` and starts A/B
/// listening on the song. Rendering takes a moment, so this runs off the
/// main thread.
#[tauri::command(async)]
fn compare_start(state: State<'_, AppState>, snapshot_id: u32) -> Result<Comparison, String> {
    let project = state.session()?.project().clone();
    let comparison = compare::measure(&project, &state.audio, snapshot_id)?;
    diagnostics::log(&format!(
        "A/B with \"{}\": song {:?} LUFS, version {:?} LUFS",
        comparison.name, comparison.current_lufs, comparison.version_lufs
    ));
    if let Ok(mut c) = state.comparison.lock() {
        *c = Some((comparison.clone(), Side::Current));
    }
    compare_listen(state, Side::Current)?;
    Ok(comparison)
}

/// Plays the song or the saved version being compared, at matched loudness.
#[tauri::command]
fn compare_listen(state: State<'_, AppState>, side: Side) -> Result<(), String> {
    let project = state.session()?.project().clone();
    let mut slot = state
        .comparison
        .lock()
        .map_err(|_| "comparison is unavailable")?;
    let Some((comparison, current)) = slot.as_mut() else {
        return Err("not comparing versions".into());
    };
    let playing =
        compare::listening_project(&project, comparison, side).ok_or("that version was deleted")?;
    *current = side;
    if let Some(engine) = state.engine() {
        engine.sync(&playing);
    }
    Ok(())
}

/// Ends A/B listening; the song plays as it is again.
#[tauri::command]
fn compare_stop(state: State<'_, AppState>) -> Result<(), String> {
    state.end_comparison();
    let session = state.session()?;
    if let Some(engine) = state.engine() {
        engine.sync(session.project());
    }
    Ok(())
}

/// The diagnostic report as plain text, for the user to copy.
#[tauri::command]
fn diagnostic_report(state: State<'_, AppState>) -> Result<String, String> {
    daw_control::diagnostic_report(&*state)
}

/// Records an error the UI showed, for the diagnostic report.
#[tauri::command]
fn log_ui_error(message: String) {
    diagnostics::log_error(&message);
}

/// "Windows 10.0.26100" (Windows 11 reports 10.0 with a build of 22000+).
fn os_version() -> String {
    #[cfg(windows)]
    {
        let v = windows_version::OsVersion::current();
        format!("Windows {}.{}.{}", v.major, v.minor, v.build)
    }
    #[cfg(not(windows))]
    {
        std::env::consts::OS.to_owned()
    }
}

#[derive(Serialize)]
struct ComparingStatus {
    snapshot_id: u32,
    side: Side,
}

/// Audio CPU load above which sound is about to break up.
const BUSY_CPU_LOAD: f32 = 0.8;
/// How long the bigger-buffer hint stays up after a crackle.
const CRACKLE_HINT: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Serialize)]
struct TransportStatus {
    playing: bool,
    position_beats: f64,
    /// Beats of count-in left before the song starts (0 when not counting in).
    count_in_beats: f64,
    /// Bars of clicks before recording starts.
    count_in_bars: u32,
    metronome_on: bool,
    peak_left: f32,
    peak_right: f32,
    cpu_load: f32,
    buffer_frames: u32,
    /// Callbacks that ran out of time since audio started (heard as crackles).
    overloads: u32,
    /// CPU near its limit, or a crackle in the last few seconds.
    struggling: bool,
    /// A/B listening: the saved version compared, and which side plays.
    comparing: Option<ComparingStatus>,
    /// Game preview: where a queued section change happens.
    jump_at_beats: Option<f64>,
    /// Peak level per track, in track order.
    track_peaks: Vec<f32>,
    /// Peak level per bus, in bus order.
    bus_peaks: Vec<f32>,
    recording: bool,
}

#[tauri::command]
fn transport_status(state: State<'_, AppState>) -> TransportStatus {
    let snap = state.engine().map(|e| e.status()).unwrap_or_default();
    let crackled = state.overload_watch.lock().is_ok_and(|mut w| {
        // A new engine starts counting from 0 again.
        if snap.overloads > w.0 {
            w.1 = Some(Instant::now());
        }
        w.0 = snap.overloads;
        w.1.is_some_and(|t| t.elapsed() < CRACKLE_HINT)
    });
    TransportStatus {
        playing: snap.playing,
        position_beats: snap.position_beats,
        count_in_beats: snap.count_in_beats,
        count_in_bars: state.count_in_bars(),
        metronome_on: state.metronome_on.load(Ordering::Relaxed),
        peak_left: snap.peak_left,
        peak_right: snap.peak_right,
        cpu_load: snap.cpu_load,
        buffer_frames: snap.buffer_frames,
        overloads: snap.overloads,
        struggling: crackled || snap.cpu_load > BUSY_CPU_LOAD,
        jump_at_beats: snap.jump_at_beats,
        comparing: state.comparison.lock().ok().and_then(|c| {
            c.as_ref().map(|(c, side)| ComparingStatus {
                snapshot_id: c.snapshot_id,
                side: *side,
            })
        }),
        track_peaks: snap.track_peaks,
        bus_peaks: snap.bus_peaks,
        recording: state.recording_track.lock().is_ok_and(|r| r.is_some())
            || state.audio_take.lock().is_ok_and(|t| t.is_some()),
    }
}

// ---- Audio files and the microphone ----

/// Brings an audio file into the project (on `track_id`, or a new audio
/// track) at `start_beats`.
// Off the main thread: decoding and rendering can take seconds.
#[tauri::command(async)]
fn import_audio(
    state: State<'_, AppState>,
    path: String,
    track_id: Option<TrackId>,
    start_beats: Option<f64>,
) -> Result<ProjectView, String> {
    daw_control::import_audio(&*state, Path::new(&path), track_id, start_beats)?;
    get_project(state)
}

/// Reads a .mid file into new tracks.
// Off the main thread: decoding and rendering can take seconds.
#[tauri::command(async)]
fn import_midi(state: State<'_, AppState>, path: String) -> Result<ProjectView, String> {
    daw_control::notation::import_midi_file(&*state, Path::new(&path))?;
    get_project(state)
}

// Off the main thread: decoding and rendering can take seconds.
#[tauri::command(async)]
fn export_midi(state: State<'_, AppState>, path: String) -> Result<(), String> {
    daw_control::notation::export_midi_file(&*state, Path::new(&path))
}

/// Reads sheet music (.musicxml, .xml, .mxl) into new tracks.
// Off the main thread: decoding and rendering can take seconds.
#[tauri::command(async)]
fn import_musicxml(state: State<'_, AppState>, path: String) -> Result<ProjectView, String> {
    daw_control::notation::import_musicxml_file(&*state, Path::new(&path))?;
    get_project(state)
}

#[tauri::command(async)]
fn export_musicxml(
    state: State<'_, AppState>,
    path: String,
    track_ids: Option<Vec<TrackId>>,
) -> Result<(), String> {
    daw_control::notation::export_musicxml_file(&*state, Path::new(&path), track_ids.as_deref())
}

/// The song (or some tracks) as MusicXML, for the sheet music view.
#[tauri::command]
fn sheet_music(
    state: State<'_, AppState>,
    track_ids: Option<Vec<TrackId>>,
) -> Result<String, String> {
    daw_control::notation::sheet_music(&*state, track_ids.as_deref())
}

/// Renders loops (and stems, adaptive-music resources) into a Godot project.
#[tauri::command(async)]
fn export_godot(
    state: State<'_, AppState>,
    options: daw_control::GodotOptions,
) -> Result<daw_export::ExportReport, String> {
    daw_control::export_godot(&*state, &options)
}

/// Whether a sampler's pack has loaded (or why not).
#[tauri::command]
fn sample_pack_status(path: String) -> serde_json::Value {
    daw_control::sample_pack_status(Path::new(&path))
}

/// Renders the song to a WAV file.
// Off the main thread: decoding and rendering can take seconds.
#[tauri::command(async)]
fn export_wav(state: State<'_, AppState>, path: String) -> Result<(), String> {
    daw_control::export_song_wav(&*state, Path::new(&path)).map(|_| ())
}

/// Waveform overview of an audio file in the project.
// Off the main thread: decoding and rendering can take seconds.
#[tauri::command(async)]
fn audio_peaks(state: State<'_, AppState>, file: String) -> Result<daw_audio::Peaks, String> {
    state
        .audio
        .peaks(&file)
        .map(|p| (*p).clone())
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct InputStatus {
    devices: Vec<String>,
    default_device: Option<String>,
    /// Open input, if any.
    active: Option<String>,
    sample_rate_hz: Option<u32>,
    /// Peak level since the last call, 0.0–1.0.
    level: f32,
    error: Option<String>,
    /// The microphone's recording delay correction.
    delay: Option<RecordingDelay>,
}

#[tauri::command]
fn input_status(state: State<'_, AppState>) -> InputStatus {
    let input = state.input.lock().ok();
    let input = input.as_ref().and_then(|i| i.as_ref());
    InputStatus {
        devices: device::input_device_names(),
        default_device: device::default_input_device_name(),
        active: input.map(|i| i.device_name().to_owned()),
        sample_rate_hz: input.map(AudioInput::sample_rate_hz),
        level: state
            .recorder
            .lock()
            .ok()
            .and_then(|r| r.as_ref().map(AudioRecorder::take_level))
            .unwrap_or(0.0),
        error: state.input_error.lock().ok().and_then(|e| e.clone()),
        delay: state.recording_delay(None).ok(),
    }
}

/// Sets how late the current microphone's recordings arrive (ms).
#[tauri::command]
fn set_recording_offset(state: State<'_, AppState>, ms: f64) -> Result<RecordingDelay, String> {
    state.recording_delay(Some(ms))
}

/// The user claps along with clicks; measures (and keeps) the delay.
#[tauri::command(async)]
fn calibrate_recording(state: State<'_, AppState>) -> Result<CalibrationResult, String> {
    state.calibrate_recording()
}

/// Opens (true) or closes (false) the microphone, for the input meter.
#[tauri::command]
fn monitor_input(state: State<'_, AppState>, on: bool) -> InputStatus {
    if on {
        // Errors show up in the returned status.
        let _ = state.ensure_input();
    } else {
        state.close_input();
    }
    input_status(state)
}

/// Switches microphone (`None` = system default).
#[tauri::command]
fn set_input_device(state: State<'_, AppState>, name: Option<String>) -> InputStatus {
    if let Ok(mut n) = state.input_device.lock() {
        *n = name;
    }
    let was_open = state.input.lock().is_ok_and(|i| i.is_some());
    state.close_input();
    if was_open {
        let _ = state.ensure_input();
    }
    input_status(state)
}

#[derive(Serialize)]
struct AudioStatus {
    output_devices: Vec<String>,
    default_output: Option<String>,
    active_output: Option<String>,
    sample_rate_hz: Option<u32>,
    /// The buffer size asked for in frames (None = the device's default).
    buffer_setting: Option<u32>,
    /// The fixed buffer size in use (None = the device's default).
    buffer_active: Option<u32>,
    buffer_options: Vec<u32>,
    error: Option<String>,
    midi_inputs: Vec<String>,
}

fn audio_status_of(state: &AppState) -> AudioStatus {
    let output = state.output.lock().ok();
    let output = output.as_ref().and_then(|o| o.as_ref());
    AudioStatus {
        output_devices: device::output_device_names(),
        default_output: device::default_output_device_name(),
        active_output: output.map(|o| o.device_name().to_owned()),
        sample_rate_hz: output.map(AudioOutput::sample_rate_hz),
        buffer_setting: state.settings.lock().ok().and_then(|s| s.buffer_frames),
        buffer_active: output.and_then(AudioOutput::buffer_frames),
        buffer_options: device::BUFFER_SIZES.to_vec(),
        error: state.audio_error.lock().ok().and_then(|e| e.clone()),
        midi_inputs: state
            .midi
            .lock()
            .ok()
            .and_then(|m| m.as_ref().map(|m| m.port_names().to_vec()))
            .unwrap_or_default(),
    }
}

#[tauri::command]
fn audio_status(state: State<'_, AppState>) -> AudioStatus {
    audio_status_of(&state)
}

/// Switches to another output device (`None` = system default).
#[tauri::command]
fn set_output_device(state: State<'_, AppState>, name: Option<String>) -> AudioStatus {
    // The error, if any, is reported in the returned status.
    let _ = state.start_audio(name.as_deref());
    audio_status_of(&state)
}

/// Changes the sound card buffer size (`None` = the device's default) and
/// restarts audio with it. Refused while recording.
#[tauri::command]
fn set_buffer_size(state: State<'_, AppState>, frames: Option<u32>) -> Result<AudioStatus, String> {
    let recording = state.recording_track.lock().is_ok_and(|r| r.is_some())
        || state.audio_take.lock().is_ok_and(|t| t.is_some());
    if recording {
        return Err("stop recording before changing the buffer size".into());
    }
    {
        let mut s = state
            .settings
            .lock()
            .map_err(|_| "settings are unavailable")?;
        s.buffer_frames = frames.filter(|f| device::BUFFER_SIZES.contains(f));
        s.save(&settings::settings_path())?;
        diagnostics::log(&format!(
            "Buffer setting: {}",
            s.buffer_frames
                .map_or("default".to_owned(), |f| f.to_string())
        ));
    }
    let choice = state.output_choice.lock().ok().and_then(|c| c.clone());
    // The error, if any, is reported in the returned status.
    let _ = state.start_audio(choice.as_deref());
    Ok(audio_status_of(&state))
}

/// Reconnects to MIDI keyboards (after plugging one in).
#[tauri::command]
fn refresh_midi(app: AppHandle, state: State<'_, AppState>) -> AudioStatus {
    connect_midi(&app, &state);
    audio_status_of(&state)
}

/// A note from a MIDI keyboard, so the on-screen piano can light up.
#[derive(Clone, Serialize)]
struct MidiNoteEvent {
    note: u8,
    on: bool,
}

fn connect_midi(app: &AppHandle, state: &AppState) {
    let Ok(mut midi) = state.midi.lock() else {
        return;
    };
    // Close existing ports before reopening them.
    *midi = None;
    let engine = Arc::clone(&state.engine);
    let selected = Arc::clone(&state.selected_track);
    let app = app.clone();
    let on_event = Arc::new(move |event: MidiEvent| {
        let Some(engine) = engine.read().ok().and_then(|e| e.clone()) else {
            return;
        };
        let track = selected.load(Ordering::Relaxed);
        match event {
            MidiEvent::NoteOn { note, velocity, .. } => {
                engine.note_on(track, note, f32::from(velocity) / 127.0);
                let _ = app.emit("midi-note", MidiNoteEvent { note, on: true });
            }
            MidiEvent::NoteOff { note, .. } => {
                engine.note_off(track, note);
                let _ = app.emit("midi-note", MidiNoteEvent { note, on: false });
            }
            MidiEvent::ControlChange {
                controller, value, ..
            } => {
                engine.send(daw_engine::EngineMessage::ControlChange {
                    track_id: track,
                    controller,
                    value,
                });
            }
            MidiEvent::PitchBend { value, .. } => {
                engine.send(daw_engine::EngineMessage::PitchBend {
                    track_id: track,
                    semitones: f32::from(value) / 8192.0 * 2.0,
                });
            }
        }
    });
    *midi = Some(MidiInputs::connect_all(on_event));
}

// ---- Claude ----

/// Where the bridge program Claude launches lives: next to the app when
/// installed, or in the build folder during development.
fn bridge_path(app: &AppHandle) -> PathBuf {
    let name = if cfg!(windows) {
        "npt-mcp.exe"
    } else {
        "npt-mcp"
    };
    let candidates = [
        app.path().resource_dir().ok().map(|d| d.join(name)),
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join(name))),
    ];
    candidates
        .iter()
        .flatten()
        .find(|p| p.exists())
        .cloned()
        .or_else(|| candidates.into_iter().flatten().next())
        .unwrap_or_else(|| PathBuf::from(name))
}

#[derive(Serialize)]
struct ClaudeStatus {
    /// The control server is running, so Claude can connect.
    listening: bool,
    error: Option<String>,
    /// Seconds since Claude last did something (None = not this session).
    last_activity_secs: Option<u64>,
    activity: Vec<RemoteActivity>,
    bridge_path: String,
    bridge_found: bool,
    desktop_config_path: String,
    desktop_configured: bool,
    claude_code_command: String,
}

#[tauri::command]
fn claude_status(app: AppHandle, state: State<'_, AppState>) -> ClaudeStatus {
    let bridge = bridge_path(&app);
    let config = claude_setup::desktop_config_path();
    ClaudeStatus {
        listening: state.control.lock().is_ok_and(|c| c.is_some()),
        error: state.control_error.lock().ok().and_then(|e| e.clone()),
        last_activity_secs: state
            .last_remote
            .lock()
            .ok()
            .and_then(|t| t.map(|t| t.elapsed().as_secs())),
        activity: state
            .remote_log
            .lock()
            .map(|l| l.iter().rev().cloned().collect())
            .unwrap_or_default(),
        bridge_found: bridge.exists(),
        desktop_configured: claude_setup::desktop_configured(&config, &bridge),
        claude_code_command: claude_setup::claude_code_command(&bridge),
        bridge_path: bridge.display().to_string(),
        desktop_config_path: config.display().to_string(),
    }
}

/// Adds Nunc Pro Tune to Claude Desktop's settings (backing them up first).
#[tauri::command]
fn claude_install_desktop(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ClaudeStatus, String> {
    let bridge = bridge_path(&app);
    if !bridge.exists() {
        return Err(format!(
            "the Claude bridge program is missing ({})",
            bridge.display()
        ));
    }
    claude_setup::install_desktop(&claude_setup::desktop_config_path(), &bridge)?;
    Ok(claude_status(app, state))
}

/// Starts the control server Claude's bridge connects to.
fn start_control(app: &AppHandle) {
    let handle = app.clone();
    let result = ControlServer::start(&daw_control::control_file_path(), move |request| {
        let state = handle.state::<AppState>();
        if let Ok(mut t) = state.last_remote.lock() {
            *t = Some(Instant::now());
        }
        daw_control::handle(&*state, request)
    });
    let state = app.state::<AppState>();
    match result {
        Ok(server) => {
            if let Ok(mut c) = state.control.lock() {
                *c = Some(server);
            }
        }
        Err(e) => {
            diagnostics::log_error(&format!("control server failed: {e}"));
            if let Ok(mut err) = state.control_error.lock() {
                *err = Some(format!("Claude can't connect: {e}"));
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(AppState::default())
        .setup(|app| {
            let state = app.state::<AppState>();
            let _ = state.app.set(app.handle().clone());
            if let Ok(slot) = state.autosave.lock()
                && let Ok(mut r) = state.recoverable.lock()
            {
                *r = autosave::find_recoverable(&slot);
            }
            let handle = app.handle().clone();
            let _ = std::thread::Builder::new()
                .name("npt-autosave".into())
                .spawn(move || {
                    loop {
                        std::thread::sleep(autosave::AUTOSAVE_INTERVAL);
                        handle.state::<AppState>().autosave();
                    }
                });
            start_control(app.handle());
            // Sound problems are shown in the status bar, not fatal.
            let _ = state.start_audio(None);
            connect_midi(app.handle(), &state);
            // A project file passed on the command line (or by double-clicking
            // it, once file associations exist) opens at startup.
            if let Some(path) = std::env::args_os().nth(1).map(PathBuf::from).filter(|p| {
                p.extension()
                    .is_some_and(|e| e == daw_model::PROJECT_EXTENSION)
            }) && let Err(e) = daw_control::open_project(&*state, &path)
            {
                eprintln!("could not open {}: {e}", path.display());
            }
            // Keep the first track's kind sensible for MIDI routing.
            if let Ok(session) = state.session()
                && let Some(first) = session
                    .project()
                    .tracks
                    .iter()
                    .find(|t| t.instrument.kind == InstrumentKind::Synth)
            {
                state.selected_track.store(first.id, Ordering::Relaxed);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            get_project,
            new_project,
            open_project,
            save_project,
            locate,
            record_start,
            record_stop,
            execute,
            end_gesture,
            undo,
            redo,
            instrument_catalog,
            note_on,
            note_off,
            all_notes_off,
            select_track,
            play,
            stop,
            set_metronome,
            set_count_in,
            set_buffer_size,
            diagnostic_report,
            log_ui_error,
            compare_start,
            compare_listen,
            compare_stop,
            inspect_export,
            audition_seam,
            freeze_track,
            user_presets,
            sample_library,
            download_sample_pack,
            save_preset,
            delete_preset,
            load_user_preset,
            preview_start,
            preview_switch,
            preview_layer,
            preview_stop,
            transport_status,
            audio_status,
            set_output_device,
            refresh_midi,
            claude_status,
            claude_install_desktop,
            import_audio,
            audio_peaks,
            input_status,
            monitor_input,
            set_input_device,
            import_midi,
            export_midi,
            export_wav,
            import_musicxml,
            export_musicxml,
            sheet_music,
            export_godot,
            sample_pack_status,
            recovery_check,
            recovery_resolve,
            set_recording_offset,
            calibrate_recording
        ])
        .build(tauri::generate_context!());
    match result {
        Ok(app) => app.run(|handle, event| {
            // A clean exit leaves nothing to recover (the user already
            // chose to save or discard).
            if let tauri::RunEvent::Exit = event {
                handle.state::<AppState>().discard_autosave();
            }
        }),
        Err(e) => {
            eprintln!("Nunc Pro Tune failed to start: {e}");
            std::process::exit(1);
        }
    }
}
