//! Nunc Pro Tune desktop shell: connects the React UI to the Rust engine.
//!
//! Two kinds of calls arrive from the UI:
//! - **Project edits** arrive as `daw_model::Command`s (the same type Claude
//!   will send over MCP in Phase 3). They are undoable and saved.
//! - **Live actions** (playing notes, play/stop, metronome, device choice)
//!   change what you hear right now but not the song itself.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use daw_engine::Engine;
use daw_engine::device::{self, AudioOutput};
use daw_engine::midi::{MidiEvent, MidiInputs};
use daw_model::{Command, InstrumentKind, Project, Session, TrackId};
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
}

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
        }
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
        let result = AudioOutput::start(device_name, &project);
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
}

/// What the UI needs to draw the project and the undo/redo buttons.
#[derive(Serialize)]
struct ProjectView {
    project: Project,
    can_undo: bool,
    can_redo: bool,
}

fn view(session: &Session) -> ProjectView {
    ProjectView {
        project: session.project().clone(),
        can_undo: session.can_undo(),
        can_redo: session.can_redo(),
    }
}

/// Pushes the current project to the audio engine and returns the view.
fn after_edit(state: &AppState, session: &Session) -> ProjectView {
    if let Some(engine) = state.engine() {
        engine.sync(session.project());
    }
    view(session)
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
    Ok(view(&session))
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
    }
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            let state = app.state::<AppState>();
            // Sound problems are shown in the status bar, not fatal.
            let _ = state.start_audio(None);
            connect_midi(app.handle(), &state);
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
            refresh_midi
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("Nunc Pro Tune failed to start: {e}");
        std::process::exit(1);
    }
}
