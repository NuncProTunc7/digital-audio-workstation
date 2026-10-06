//! Local control protocol for Nunc Pro Tune.
//!
//! The desktop app runs a [`ControlServer`] on 127.0.0.1. The MCP bridge
//! (`npt-mcp`) finds it through a small discovery file and sends JSON
//! [`Request`]s, which the app handles with [`handle`]. The same handler
//! drives the app's own buttons for files (New/Open/Save), so Claude and the
//! user always go through identical code paths.

pub mod claude_setup;
mod client;
mod discovery;
mod host;
mod protocol;
mod server;

pub use client::{ClientError, ControlClient};
pub use discovery::{APP_ID, ControlFile, control_file_path};
pub use host::{Host, handle, new_project, open_project, save_project};
pub use protocol::{Request, Response};
pub use server::ControlServer;
