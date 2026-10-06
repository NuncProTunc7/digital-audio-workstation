//! Project data model for Nunc Pro Tune.
//!
//! Every change to a project goes through a [`Command`]. The UI, keyboard
//! shortcuts, and Claude (via MCP) all use the same Commands, which is what
//! makes every edit undoable and every button scriptable.

mod command;
mod project;
mod session;

pub use command::{Command, CommandError, command_schema};
pub use project::{Project, TimeSignature};
pub use session::Session;
