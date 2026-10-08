//! Local control protocol for Nunc Pro Tune.
//!
//! The desktop app runs a [`ControlServer`] on 127.0.0.1. The MCP bridge
//! (`npt-mcp`) finds it through a small discovery file and sends JSON
//! [`Request`]s, which the app handles with [`handle`]. The same handler
//! drives the app's own buttons for files (New/Open/Save), so Claude and the
//! user always go through identical code paths.

pub mod autosave;
pub mod calibrate;
pub mod claude_setup;
mod client;
pub mod compare;
pub mod diagnostics;
mod discovery;
pub mod game_preview;
mod host;
pub mod notation;
pub mod presets;
mod protocol;
mod server;
pub mod settings;

pub use client::{ClientError, ControlClient};
pub use discovery::{APP_ID, ControlFile, control_file_path, unsaved_audio_dir};
pub use host::{
    CalibrationResult, Host, RecordingDelay, SavedProject, begin_take, diagnostic_report, end_take,
    export_godot, export_song_wav, freeze_track, handle, import_audio, inspect_export,
    load_user_preset, new_project, open_project, sample_pack_status, save_project,
    save_user_preset,
};
pub use protocol::{GodotOptions, Request, Response};
pub use server::ControlServer;
