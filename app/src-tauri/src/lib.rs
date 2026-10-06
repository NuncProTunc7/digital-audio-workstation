//! Nunc Pro Tune desktop shell: connects the React UI to the Rust engine.
//!
//! Every project edit arrives here as a `daw_model::Command`, the same type
//! Claude will send over MCP in Phase 3.

use std::sync::Mutex;

use daw_engine::device::{self, TestTonePlayer};
use daw_model::{Command, Project, Session};
use serde::Serialize;
use tauri::State;

#[derive(Default)]
struct AppState {
    session: Mutex<Session>,
    // Opened on first use so the app starts even with no sound card.
    tone_player: Mutex<Option<TestTonePlayer>>,
}

/// What the UI needs to draw the project and the undo/redo buttons.
#[derive(Serialize)]
struct ProjectView {
    project: Project,
    can_undo: bool,
    can_redo: bool,
}

#[derive(Serialize)]
struct AudioStatus {
    output_devices: Vec<String>,
    default_output: Option<String>,
    active_output: Option<String>,
    sample_rate_hz: Option<u32>,
    test_tone_on: bool,
}

#[derive(Serialize)]
struct AppInfo {
    name: &'static str,
    version: &'static str,
    license: &'static str,
}

fn view(session: &Session) -> ProjectView {
    ProjectView {
        project: session.project().clone(),
        can_undo: session.can_undo(),
        can_redo: session.can_redo(),
    }
}

fn lock_session(state: &AppState) -> Result<std::sync::MutexGuard<'_, Session>, String> {
    state
        .session
        .lock()
        .map_err(|_| "project state is unavailable".to_owned())
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        name: "Nunc Pro Tune",
        version: env!("CARGO_PKG_VERSION"),
        license: env!("CARGO_PKG_LICENSE"),
    }
}

#[tauri::command]
fn get_project(state: State<'_, AppState>) -> Result<ProjectView, String> {
    let session = lock_session(&state)?;
    Ok(view(&session))
}

#[tauri::command]
fn execute(state: State<'_, AppState>, command: Command) -> Result<ProjectView, String> {
    let mut session = lock_session(&state)?;
    session.execute(command).map_err(|e| e.to_string())?;
    Ok(view(&session))
}

#[tauri::command]
fn undo(state: State<'_, AppState>) -> Result<ProjectView, String> {
    let mut session = lock_session(&state)?;
    session.undo();
    Ok(view(&session))
}

#[tauri::command]
fn redo(state: State<'_, AppState>) -> Result<ProjectView, String> {
    let mut session = lock_session(&state)?;
    session.redo();
    Ok(view(&session))
}

fn audio_status_of(player: Option<&TestTonePlayer>) -> AudioStatus {
    AudioStatus {
        output_devices: device::output_device_names(),
        default_output: device::default_output_device_name(),
        active_output: player.map(|p| p.device_name().to_owned()),
        sample_rate_hz: player.map(TestTonePlayer::sample_rate_hz),
        test_tone_on: player.is_some_and(TestTonePlayer::is_tone_on),
    }
}

#[tauri::command]
fn audio_status(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    let player = state
        .tone_player
        .lock()
        .map_err(|_| "audio state is unavailable".to_owned())?;
    Ok(audio_status_of(player.as_ref()))
}

/// Starts or stops the 440 Hz test tone on the default output device.
#[tauri::command]
fn set_test_tone(state: State<'_, AppState>, on: bool) -> Result<AudioStatus, String> {
    let mut player = state
        .tone_player
        .lock()
        .map_err(|_| "audio state is unavailable".to_owned())?;
    if player.is_none() {
        if !on {
            return Ok(audio_status_of(None));
        }
        *player = Some(TestTonePlayer::open_default().map_err(|e| e.to_string())?);
    }
    if let Some(p) = player.as_ref() {
        p.set_tone_on(on);
    }
    Ok(audio_status_of(player.as_ref()))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            app_info,
            get_project,
            execute,
            undo,
            redo,
            audio_status,
            set_test_tone
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("Nunc Pro Tune failed to start: {e}");
        std::process::exit(1);
    }
}
