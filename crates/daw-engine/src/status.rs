use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use daw_model::MAX_TRACKS;

/// Live engine readings, written by the audio thread and read by the UI.
/// All fields are atomics so neither side ever waits on the other.
#[derive(Debug)]
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
    // Post-fader peak per track slot since the UI last read them (f32 bits).
    track_peaks: [AtomicU32; MAX_TRACKS],
}

impl Default for EngineStatus {
    fn default() -> Self {
        Self {
            playing: AtomicBool::new(false),
            position_beats: AtomicU64::new(0),
            peak_left: AtomicU32::new(0),
            peak_right: AtomicU32::new(0),
            cpu_load: AtomicU32::new(0),
            sample_rate_hz: AtomicU32::new(0),
            buffer_frames: AtomicU32::new(0),
            track_peaks: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    }
}

/// A copy of [`EngineStatus`] at one moment.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatusSnapshot {
    pub playing: bool,
    pub position_beats: f64,
    pub peak_left: f32,
    pub peak_right: f32,
    pub cpu_load: f32,
    pub sample_rate_hz: u32,
    /// Sound card buffer size; 0 until audio has started.
    pub buffer_frames: u32,
    /// Peak level per track, in project track order.
    pub track_peaks: Vec<f32>,
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
    // RT-SAFE
    pub(crate) fn add_track_peak(&self, index: usize, peak: f32) {
        if let Some(slot) = self.track_peaks.get(index) {
            slot.fetch_max(peak.abs().to_bits(), Ordering::Relaxed);
        }
    }

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
            track_peaks: self
                .track_peaks
                .iter()
                .map(|p| f32::from_bits(p.swap(0, Ordering::Relaxed)))
                .collect(),
        }
    }
}
