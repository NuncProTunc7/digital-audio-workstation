use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// Live engine readings, written by the audio thread and read by the UI.
/// All fields are atomics so neither side ever waits on the other.
#[derive(Debug, Default)]
pub struct EngineStatus {
    playing: AtomicBool,
    position_beats: AtomicU64,
    // Peak levels since the UI last read them (f32 bits).
    peak_left: AtomicU32,
    peak_right: AtomicU32,
    // Fraction of the available time the last callback used (f32 bits).
    cpu_load: AtomicU32,
    sample_rate_hz: AtomicU32,
    // Frames in the most recent sound card callback.
    buffer_frames: AtomicU32,
}

/// A copy of [`EngineStatus`] at one moment.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StatusSnapshot {
    pub playing: bool,
    pub position_beats: f64,
    pub peak_left: f32,
    pub peak_right: f32,
    pub cpu_load: f32,
    pub sample_rate_hz: u32,
    /// Sound card buffer size; 0 until audio has started.
    pub buffer_frames: u32,
}

impl EngineStatus {
    // RT-SAFE
    pub(crate) fn publish(&self, playing: bool, position_beats: f64) {
        self.playing.store(playing, Ordering::Relaxed);
        self.position_beats
            .store(position_beats.to_bits(), Ordering::Relaxed);
    }

    // RT-SAFE
    pub(crate) fn add_peaks(&self, left: f32, right: f32) {
        // fetch_max on the bit pattern works because positive f32 values
        // order the same as their bits.
        self.peak_left
            .fetch_max(left.abs().to_bits(), Ordering::Relaxed);
        self.peak_right
            .fetch_max(right.abs().to_bits(), Ordering::Relaxed);
    }

    // RT-SAFE
    #[cfg_attr(not(feature = "device"), allow(dead_code))]
    pub(crate) fn set_callback_stats(&self, load: f32, frames: u32) {
        self.cpu_load.store(load.to_bits(), Ordering::Relaxed);
        self.buffer_frames.store(frames, Ordering::Relaxed);
    }

    pub(crate) fn set_sample_rate(&self, sample_rate_hz: u32) {
        self.sample_rate_hz.store(sample_rate_hz, Ordering::Relaxed);
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    /// Reads current values and resets the peak meters.
    pub fn take_snapshot(&self) -> StatusSnapshot {
        StatusSnapshot {
            playing: self.playing.load(Ordering::Relaxed),
            position_beats: f64::from_bits(self.position_beats.load(Ordering::Relaxed)),
            peak_left: f32::from_bits(self.peak_left.swap(0, Ordering::Relaxed)),
            peak_right: f32::from_bits(self.peak_right.swap(0, Ordering::Relaxed)),
            cpu_load: f32::from_bits(self.cpu_load.load(Ordering::Relaxed)),
            sample_rate_hz: self.sample_rate_hz.load(Ordering::Relaxed),
            buffer_frames: self.buffer_frames.load(Ordering::Relaxed),
        }
    }
}
