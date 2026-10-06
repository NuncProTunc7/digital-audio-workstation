//! Nunc Pro Tune audio engine.
//!
//! Phase 0 contains only a test tone: enough to prove that sound reaches the
//! speakers on a real machine and that headless rendering works in tests.

#[cfg(feature = "device")]
pub mod device;
pub mod offline;
mod tone;

pub use tone::ToneGenerator;

/// Sample rate used when no sound card dictates one.
pub const DEFAULT_SAMPLE_RATE_HZ: u32 = 48_000;
