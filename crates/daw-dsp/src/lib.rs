//! DSP building blocks shared by instruments and effects.
//!
//! Everything here is real-time safe: no allocation, locking, or I/O after
//! construction. All processing is single-sample (`next`) so callers decide
//! their own block structure.

mod biquad;
mod envelope;
mod filter;
mod oscillator;
mod util;

pub use biquad::{Biquad, BiquadShape};
pub use envelope::{Adsr, AdsrParams};
pub use filter::{FilterMode, Svf};
pub use oscillator::{Lfo, Oscillator, Waveform};
pub use util::{Rng, Smoother, db_to_gain, midi_to_hz};
