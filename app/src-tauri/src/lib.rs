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

use daw_control::claude_setup;
use daw_control::{ControlServer, Host};
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

    /// (Re)opens audio output. Keeps the app running without sound on failure.
    fn start_audio(&self, device_name: Option<&str>) -> Result<(), String> {
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
        let result = AudioOutput::start(device_name, &project, Arc::clone(&self.audio));
        let mut error = self
            .audio_error
            .lock()
            .map_err(|_| "audio state is unavailable")?;
        match result {
            Ok((engine, new_output)) => {
                engine.set_metronome(self.metronome_on.load(Ordering::Relaxed));
                if let Ok(mut slot) = self.engine.write() {
                    *slot = Some(Arc::new(engine));
                }
                *output = Some(new_output);
                *error = None;
                Ok(())
            }
            Err(e) => {
                let message = e.to_string();
                *error = Some(message.clone());
                Err(message)
            }
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
                *input = Some(stream);
                if let Ok(mut r) = self.recorder.lock() {
                    *r = Some(recorder);
                }
                *error = None;
                Ok(())
            }
            Err(e) => {
                let message = e.to_string();
                *error = Some(message.clone());
                Err(message)
            }
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
    ProjectView {
        missing_audio,
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
    get_project(state)
}

#[tauri::command]
fn open_project(state: State<'_, AppState>, path: String) -> Result<ProjectView, String> {
    daw_control::open_project(&*state, Path::new(&path))?;
    get_project(state)
}

/// Saves to `path`, or to the current file when `path` is omitted.
#[tauri::command]
fn save_project(state: State<'_, AppState>, path: Option<String>) -> Result<ProjectView, String> {
    daw_control::save_project(&*state, path.as_deref().map(Path::new))?;
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
    if let Ok(mut r) = state.recording_track.lock() {
        *r = Some(track_id);
    }
    engine.start_recording(track_id);
    engine.play();
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

#[derive(Serialize)]
struct TransportStatus {
    playing: bool,
    position_beats: f64,
    metronome_on: bool,
    peak_left: f32,
    peak_right: f32,
    cpu_load: f32,
    buffer_frames: u32,
    /// Peak level per track, in track order.
    track_peaks: Vec<f32>,
    recording: bool,
}

#[tauri::command]
fn transport_status(state: State<'_, AppState>) -> TransportStatus {
    let snap = state.engine().map(|e| e.status()).unwrap_or_default();
    TransportStatus {
        playing: snap.playing,
        position_beats: snap.position_beats,
        metronome_on: state.metronome_on.load(Ordering::Relaxed),
        peak_left: snap.peak_left,
        peak_right: snap.peak_right,
        cpu_load: snap.cpu_load,
        buffer_frames: snap.buffer_frames,
        track_peaks: snap.track_peaks,
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
    }
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
        .manage(AppState::default())
        .setup(|app| {
            let state = app.state::<AppState>();
            let _ = state.app.set(app.handle().clone());
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
            export_wav
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("Nunc Pro Tune failed to start: {e}");
        std::process::exit(1);
    }
}
