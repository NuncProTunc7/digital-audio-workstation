//! Audio files for Nunc Pro Tune.
//!
//! - [`decode_file`] reads whatever a phone or computer produces: WAV, MP3,
//!   M4A/AAC (iPhone and Android voice memos), ALAC, FLAC, OGG Vorbis.
//! - [`resample`] converts between sample rates (offline, high quality).
//! - [`peaks`] summarizes audio for waveform drawing.
//! - [`AudioPool`] manages the project's audio folder and hands the engine
//!   ready-to-play buffers at its sample rate.

mod decode;
mod peaks;
mod pool;
mod resample;
mod wav;

use std::path::PathBuf;

use thiserror::Error;

pub use decode::decode_file;
pub use peaks::{PEAKS_PER_SECOND, Peaks, peaks};
pub use pool::{AudioPool, ImportedAudio, audio_folder_for};
pub use resample::resample;
pub use wav::{WavWriter, read_wav, write_wav};

/// File extensions [`decode_file`] understands, lower case.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "wav", "wave", "mp3", "m4a", "mp4", "aac", "alac", "caf", "flac", "ogg", "oga",
];

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("could not read or write {path}: {message}")]
    Io { path: PathBuf, message: String },
    #[error(
        "{path} isn't an audio file this app can read ({message}); try WAV, MP3, M4A, FLAC or OGG"
    )]
    Unsupported { path: PathBuf, message: String },
    #[error("{0} has no sound in it")]
    Empty(PathBuf),
    #[error("{0} is longer than the one-hour limit")]
    TooLong(PathBuf),
    #[error("the audio file \"{0}\" is missing from the project's audio folder")]
    Missing(String),
    #[error("resampling failed: {0}")]
    Resample(String),
}

/// Decoded audio: one or two channels of 32-bit float samples.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioData {
    pub sample_rate_hz: u32,
    /// One `Vec` per channel (1 = mono, 2 = stereo), all the same length.
    pub channels: Vec<Vec<f32>>,
}

impl AudioData {
    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }

    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / f64::from(self.sample_rate_hz.max(1))
    }

    /// Highest absolute sample value.
    pub fn peak(&self) -> f32 {
        self.channels
            .iter()
            .flatten()
            .fold(0.0f32, |m, s| m.max(s.abs()))
    }
}

/// Audio ready for the engine: at the engine's sample rate, never resized,
/// shared with the audio thread through an `Arc`.
#[derive(Debug, PartialEq)]
pub struct AudioBuffer {
    pub left: Box<[f32]>,
    /// `None` for mono.
    pub right: Option<Box<[f32]>>,
}

impl AudioBuffer {
    pub fn frames(&self) -> usize {
        self.left.len()
    }

    /// Converts decoded audio (already at the right rate).
    pub fn from_data(data: AudioData) -> Self {
        let mut channels = data.channels.into_iter();
        let left = channels.next().unwrap_or_default().into_boxed_slice();
        let right = channels.next().map(Vec::into_boxed_slice);
        Self { left, right }
    }
}
