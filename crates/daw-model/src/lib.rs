//! Project data model for Nunc Pro Tune.
//!
//! Every change to a project goes through a [`Command`]. The UI, keyboard
//! shortcuts, and Claude (via MCP) all use the same Commands, which is what
//! makes every edit undoable and every button scriptable.

mod command;
pub mod effect;
mod file;
pub mod instrument;
pub mod music;
pub mod plugin;
mod project;
mod session;
pub mod sound;
pub mod summary;
mod tempo;

pub use command::{
    Command, CommandError, MAX_BUSES, MAX_SNAPSHOTS, NoteEdit, NoteInput, command_schema,
};
pub use effect::{Effect, EffectKind};
pub use file::{
    FileError, PROJECT_EXTENSION, load_project, project_from_json, project_to_json, save_project,
};
pub use instrument::{Instrument, InstrumentKind};
pub use project::{
    AudioRegion, AutomationLane, AutomationPoint, AutomationTarget, Bus, Clip, ClipId, EffectId,
    FORMAT_VERSION, Frozen, GuitarNote, Id, LaneId, LoopRegion, MAX_BEATS, MAX_TEMPO_BPM,
    MAX_TRACKS, MAX_VOLUME_DB, MIN_LENGTH_BEATS, MIN_TEMPO_BPM, MIN_VOLUME_DB, Marker, MasterBus,
    Mixer, Note, NoteId, Project, Send, Snapshot, SongSection, SongState, Swing, TimeSignature,
    Track, TrackId,
};
pub use session::Session;
pub use sound::{SoundCategory, SoundChanges, SoundEffect, SoundRecipe, Wave};
pub use tempo::{MAX_TEMPO_CHANGES, TempoChange, TempoMap};
