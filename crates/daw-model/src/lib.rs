//! Project data model for Nunc Pro Tune.
//!
//! Every change to a project goes through a [`Command`]. The UI, keyboard
//! shortcuts, and Claude (via MCP) all use the same Commands, which is what
//! makes every edit undoable and every button scriptable.

mod command;
pub mod effect;
mod file;
pub mod instrument;
mod project;
mod session;
pub mod summary;

pub use command::{Command, CommandError, NoteEdit, NoteInput, command_schema};
pub use effect::{Effect, EffectKind};
pub use file::{
    FileError, PROJECT_EXTENSION, load_project, project_from_json, project_to_json, save_project,
};
pub use instrument::{Instrument, InstrumentKind};
pub use project::{
    AudioRegion, Clip, ClipId, EffectId, FORMAT_VERSION, Id, LoopRegion, MAX_TRACKS, MAX_VOLUME_DB,
    MIN_VOLUME_DB, MasterBus, Mixer, Note, NoteId, Project, TimeSignature, Track, TrackId,
};
pub use session::Session;
