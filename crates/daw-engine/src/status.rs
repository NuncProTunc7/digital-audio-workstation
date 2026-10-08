use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use daw_model::MAX_TRACKS;

/// Live engine readings, written by the audio thread and read by the UI.
/// All fields are atomics so neither side ever waits on the other.
#[derive(Debug)]
pub struct EngineStatus {
    playing: AtomicBool,
    position_beats: AtomicU64,
    count_in_beats: AtomicU64,
    // Where a queued section change happens (f64 bits; NaN = none).
    jump_at_beats: AtomicU64,
    // Peak levels since the UI last read them (f32 bits).
    peak_left: AtomicU32,
    peak_right: AtomicU32,
    // Fraction of the available time the last callback used (f32 bits).
    cpu_load: AtomicU32,
    sample_rate_hz: AtomicU32,
    // Frames in the most recent sound card callback.
    buffer_frames: AtomicU32,
    // Callbacks that took longer than the sound they made (heard as a
    // crackle or dropout), and the highest load seen (f32 bits).
    overloads: AtomicU32,
    cpu_peak: AtomicU32,
    // Post-fader peak per track slot since the UI last read them (f32 bits).
    track_peaks: [AtomicU32; MAX_TRACKS],
    // Clock anchor, as a seqlock: odd `clock_seq` means a write is underway.
    clock_seq: AtomicU64,
    clock_ns: AtomicU64,
    clock_beats: AtomicU64,
    clock_tempo: AtomicU64,
    clock_playing: AtomicBool,
}

/// What the listener hears when: at `playback_ns` on the app's shared clock,
/// the speakers play beat `position_beats`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockAnchor {
    pub playback_ns: u64,
    pub position_beats: f64,
    pub tempo_bpm: f64,
    pub playing: bool,
}

impl ClockAnchor {
    /// The beat being heard at `ns` (same clock), assuming steady playback.
    pub fn beats_at(&self, ns: u64) -> f64 {
        let dt_s = (ns as f64 - self.playback_ns as f64) / 1e9;
        self.position_beats + dt_s * self.tempo_bpm / 60.0
    }
}

impl Default for EngineStatus {
    fn default() -> Self {
        Self {
            playing: AtomicBool::new(false),
            position_beats: AtomicU64::new(0),
            count_in_beats: AtomicU64::new(0),
            jump_at_beats: AtomicU64::new(f64::NAN.to_bits()),
            peak_left: AtomicU32::new(0),
            peak_right: AtomicU32::new(0),
            cpu_load: AtomicU32::new(0),
            sample_rate_hz: AtomicU32::new(0),
            buffer_frames: AtomicU32::new(0),
            overloads: AtomicU32::new(0),
            cpu_peak: AtomicU32::new(0),
            track_peaks: std::array::from_fn(|_| AtomicU32::new(0)),
            clock_seq: AtomicU64::new(0),
            clock_ns: AtomicU64::new(0),
            clock_beats: AtomicU64::new(0),
            clock_tempo: AtomicU64::new(0),
            clock_playing: AtomicBool::new(false),
        }
    }
}

/// A copy of [`EngineStatus`] at one moment.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatusSnapshot {
    pub playing: bool,
    pub position_beats: f64,
    /// Beats of count-in left before the song starts (0 when not counting in).
    pub count_in_beats: f64,
    /// Where a queued game-preview section change happens, if one is waiting.
    pub jump_at_beats: Option<f64>,
    pub peak_left: f32,
    pub peak_right: f32,
    pub cpu_load: f32,
    pub sample_rate_hz: u32,
    /// Sound card buffer size; 0 until audio has started.
    pub buffer_frames: u32,
    /// Callbacks that ran out of time since audio started (likely dropouts).
    pub overloads: u32,
    /// Highest audio CPU load since audio started (1.0 = all the time there was).
    pub cpu_peak: f32,
    /// Peak level per track, in project track order.
    pub track_peaks: Vec<f32>,
}

impl EngineStatus {
    // RT-SAFE
    // RT-SAFE
    pub(crate) fn publish_jump(&self, at_beats: Option<f64>) {
        self.jump_at_beats
            .store(at_beats.unwrap_or(f64::NAN).to_bits(), Ordering::Relaxed);
    }

    pub(crate) fn publish(&self, playing: bool, position_beats: f64, count_in_beats: f64) {
        self.playing.store(playing, Ordering::Relaxed);
        self.position_beats
            .store(position_beats.to_bits(), Ordering::Relaxed);
        self.count_in_beats
            .store(count_in_beats.to_bits(), Ordering::Relaxed);
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
    pub(crate) fn publish_clock(
        &self,
        ns: u64,
        position_beats: f64,
        tempo_bpm: f64,
        playing: bool,
    ) {
        self.clock_seq.fetch_add(1, Ordering::SeqCst);
        self.clock_ns.store(ns, Ordering::SeqCst);
        self.clock_beats
            .store(position_beats.to_bits(), Ordering::SeqCst);
        self.clock_tempo
            .store(tempo_bpm.to_bits(), Ordering::SeqCst);
        self.clock_playing.store(playing, Ordering::SeqCst);
        self.clock_seq.fetch_add(1, Ordering::SeqCst);
    }

    /// The latest clock anchor, or None before the sound card reported one.
    pub fn clock(&self) -> Option<ClockAnchor> {
        for _ in 0..1000 {
            let before = self.clock_seq.load(Ordering::SeqCst);
            if before % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let anchor = ClockAnchor {
                playback_ns: self.clock_ns.load(Ordering::SeqCst),
                position_beats: f64::from_bits(self.clock_beats.load(Ordering::SeqCst)),
                tempo_bpm: f64::from_bits(self.clock_tempo.load(Ordering::SeqCst)),
                playing: self.clock_playing.load(Ordering::SeqCst),
            };
            if self.clock_seq.load(Ordering::SeqCst) == before {
                return (before > 0).then_some(anchor);
            }
        }
        None
    }

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
        if load > 1.0 {
            self.overloads.fetch_add(1, Ordering::Relaxed);
        }
        if load.is_finite() {
            self.cpu_peak
                .fetch_max(load.max(0.0).to_bits(), Ordering::Relaxed);
        }
    }

    pub(crate) fn set_sample_rate(&self, sample_rate_hz: u32) {
        self.sample_rate_hz.store(sample_rate_hz, Ordering::Relaxed);
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn callback_for_test(&self, load: f32) {
        self.set_callback_stats(load, 256);
    }

    /// Reads current values and resets the peak meters.
    pub fn take_snapshot(&self) -> StatusSnapshot {
        StatusSnapshot {
            playing: self.playing.load(Ordering::Relaxed),
            position_beats: f64::from_bits(self.position_beats.load(Ordering::Relaxed)),
            count_in_beats: f64::from_bits(self.count_in_beats.load(Ordering::Relaxed)),
            jump_at_beats: Some(f64::from_bits(self.jump_at_beats.load(Ordering::Relaxed)))
                .filter(|b| !b.is_nan()),
            peak_left: f32::from_bits(self.peak_left.swap(0, Ordering::Relaxed)),
            peak_right: f32::from_bits(self.peak_right.swap(0, Ordering::Relaxed)),
            cpu_load: f32::from_bits(self.cpu_load.load(Ordering::Relaxed)),
            sample_rate_hz: self.sample_rate_hz.load(Ordering::Relaxed),
            buffer_frames: self.buffer_frames.load(Ordering::Relaxed),
            overloads: self.overloads.load(Ordering::Relaxed),
            cpu_peak: f32::from_bits(self.cpu_peak.load(Ordering::Relaxed)),
            track_peaks: self
                .track_peaks
                .iter()
                .map(|p| f32::from_bits(p.swap(0, Ordering::Relaxed)))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_callbacks_that_ran_out_of_time() {
        let status = EngineStatus::default();
        for load in [0.3, 1.4, 0.5, 2.0, 0.9] {
            status.callback_for_test(load);
        }
        let snap = status.take_snapshot();
        assert_eq!(snap.overloads, 2);
        assert_eq!(snap.cpu_peak, 2.0);
        assert_eq!(snap.cpu_load, 0.9);
    }
}
