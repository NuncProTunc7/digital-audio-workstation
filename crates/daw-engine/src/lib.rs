//! Nunc Pro Tune audio engine.
//!
//! Two halves:
//! - [`AudioProcessor`] runs on the real-time audio thread. It owns the
//!   instruments, transport, and metronome, and never allocates or locks.
//! - [`Engine`] is the handle the rest of the app uses. It sends messages to
//!   the processor through a lock-free queue and frees anything the processor
//!   hands back, off the audio thread.
//!
//! The same processor drives the sound card (`device`) and offline rendering
//! (`offline`), so tests hear exactly what the user hears.

#[cfg(feature = "device")]
pub mod device;
mod engine;
mod message;
mod metronome;
#[cfg(feature = "device")]
pub mod midi;
pub mod offline;
mod processor;
mod status;
mod tone;

pub use engine::Engine;
pub use message::EngineMessage;
pub use processor::{AudioProcessor, MAX_BLOCK_FRAMES};
pub use status::{EngineStatus, StatusSnapshot};
pub use tone::ToneGenerator;

/// Sample rate used when no sound card dictates one.
pub const DEFAULT_SAMPLE_RATE_HZ: u32 = 48_000;
