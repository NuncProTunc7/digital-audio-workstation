//! Recording audio from a microphone (or any input).
//!
//! The input stream's callback owns an [`InputCapture`]: it mixes the input
//! to mono, meters it, and, while armed, pushes samples into a lock-free
//! ring. An [`AudioRecorder`] on the control side runs a writer thread that
//! drains the ring into a WAV file, so the disk never blocks audio.
//!
//! Lining a take up with the song: the output callback publishes a
//! [`ClockAnchor`] (which beat the speakers play at which instant) and the
//! input callback stamps the instant its first recorded sample was captured.
//! Both instants are on one app-wide clock (see `device::clock_ns`), so the
//! take starts at the beat the performer was hearing, whatever the buffer
//! sizes.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use daw_audio::{AudioError, WavWriter};

use crate::status::{ClockAnchor, EngineStatus};

/// Ring size in samples: several seconds even at 192 kHz, so a slow disk
/// moment never drops audio.
const RING_SAMPLES: usize = 1 << 21;
/// How often the writer thread drains the ring.
const WRITER_PERIOD: Duration = Duration::from_millis(15);
/// Timestamps further apart than this are from different clocks; fall back.
const MAX_CLOCK_GAP_NS: f64 = 5e9;

/// State shared by the input callback and the recorder. Atomics only.
#[derive(Debug)]
pub struct InputShared {
    sample_rate_hz: u32,
    armed: AtomicBool,
    started: AtomicBool,
    first_capture_ns: AtomicU64,
    /// Peak input level since last read (f32 bits).
    level: AtomicU32,
    /// Samples lost because the ring was full.
    dropped: AtomicU64,
}

/// The real-time half: lives in the input stream's callback.
pub struct InputCapture {
    producer: rtrb::Producer<f32>,
    shared: Arc<InputShared>,
}

/// A pair for one input stream at `sample_rate_hz`.
pub fn input_pair(sample_rate_hz: u32) -> (InputCapture, AudioRecorder) {
    let (producer, consumer) = rtrb::RingBuffer::new(RING_SAMPLES);
    let shared = Arc::new(InputShared {
        sample_rate_hz,
        armed: AtomicBool::new(false),
        started: AtomicBool::new(false),
        first_capture_ns: AtomicU64::new(0),
        level: AtomicU32::new(0),
        dropped: AtomicU64::new(0),
    });
    (
        InputCapture {
            producer,
            shared: Arc::clone(&shared),
        },
        AudioRecorder {
            shared,
            consumer: Some(consumer),
            take: None,
        },
    )
}

impl InputCapture {
    /// Handles one buffer of interleaved input whose first frame was
    /// captured at `capture_ns` on the shared clock (`device::clock_ns`).
    // RT-SAFE
    pub fn process(&mut self, interleaved: &[f32], channels: usize, capture_ns: u64) {
        let channels = channels.max(1);
        let shared = &*self.shared;
        let armed = shared.armed.load(Ordering::Acquire);
        if armed && !shared.started.load(Ordering::Acquire) && !interleaved.is_empty() {
            shared.first_capture_ns.store(capture_ns, Ordering::Release);
            shared.started.store(true, Ordering::Release);
        }
        let scale = 1.0 / channels as f32;
        let mut peak = 0.0f32;
        let mut dropped = 0u64;
        for frame in interleaved.chunks(channels) {
            let s = frame.iter().sum::<f32>() * scale;
            let s = if s.is_finite() { s } else { 0.0 };
            peak = peak.max(s.abs());
            if armed && self.producer.push(s).is_err() {
                dropped += 1;
            }
        }
        shared.level.fetch_max(peak.to_bits(), Ordering::Relaxed);
        if dropped > 0 {
            shared.dropped.fetch_add(dropped, Ordering::Relaxed);
        }
    }
}

/// A finished take.
#[derive(Debug, Clone, PartialEq)]
pub struct FinishedTake {
    /// File name inside the audio folder.
    pub file: String,
    pub path: PathBuf,
    pub seconds: f64,
    /// Where the take's first sample belongs on the timeline. Can be
    /// negative when capture began just before the song's start; the part
    /// before beat 0 should then be trimmed, not the take moved.
    pub start_beats: f64,
    /// Where recording was asked to start (the playhead when it began);
    /// sound before it, such as a count-in, isn't part of the take.
    pub requested_beats: f64,
    /// Samples lost to an overloaded disk (should be 0).
    pub dropped_samples: u64,
}

struct Take {
    file: String,
    fallback_beats: f64,
    stop: Arc<AtomicBool>,
    thread: JoinHandle<WriterResult>,
}

type WriterResult = (
    rtrb::Consumer<f32>,
    Result<(PathBuf, f64), AudioError>,
    Option<ClockAnchor>,
);

/// The control half: starts and stops takes, reads the input meter.
pub struct AudioRecorder {
    shared: Arc<InputShared>,
    consumer: Option<rtrb::Consumer<f32>>,
    take: Option<Take>,
}

impl AudioRecorder {
    pub fn sample_rate_hz(&self) -> u32 {
        self.shared.sample_rate_hz
    }

    /// Peak input level since the last call, 0.0–1.0.
    pub fn take_level(&self) -> f32 {
        f32::from_bits(self.shared.level.swap(0, Ordering::Relaxed))
    }

    pub fn is_recording(&self) -> bool {
        self.take.is_some()
    }

    /// Starts writing input to `path` (named `file` in the audio folder).
    /// `status` supplies the playback clock; `fallback_beats` is used as the
    /// take's start if the clocks can't be compared.
    pub fn start(
        &mut self,
        file: String,
        path: PathBuf,
        status: Arc<EngineStatus>,
        fallback_beats: f64,
    ) -> Result<(), AudioError> {
        if self.take.is_some() {
            return Ok(());
        }
        let Some(mut consumer) = self.consumer.take() else {
            return Err(AudioError::Io {
                path,
                message: "the recorder is busy".into(),
            });
        };
        // Anything captured before now isn't part of this take.
        while consumer.pop().is_ok() {}
        let writer = match WavWriter::create(&path, self.shared.sample_rate_hz) {
            Ok(w) => w,
            Err(e) => {
                self.consumer = Some(consumer);
                return Err(e);
            }
        };
        self.shared.started.store(false, Ordering::Release);
        self.shared.dropped.store(0, Ordering::Relaxed);
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = Arc::clone(&stop);
            let shared = Arc::clone(&self.shared);
            std::thread::Builder::new()
                .name("npt-recorder".into())
                .spawn(move || write_loop(consumer, writer, &stop, &shared, &status))
                .map_err(|e| AudioError::Io {
                    path: path.clone(),
                    message: e.to_string(),
                })?
        };
        self.shared.armed.store(true, Ordering::Release);
        self.take = Some(Take {
            file,
            fallback_beats,
            stop,
            thread,
        });
        Ok(())
    }

    /// Stops the take and finishes its file. `None` if nothing was recording.
    pub fn stop(&mut self) -> Option<Result<FinishedTake, AudioError>> {
        let take = self.take.take()?;
        self.shared.armed.store(false, Ordering::Release);
        take.stop.store(true, Ordering::Release);
        let Ok((consumer, written, anchor)) = take.thread.join() else {
            return Some(Err(AudioError::Io {
                path: PathBuf::from(&take.file),
                message: "the recording thread stopped unexpectedly".into(),
            }));
        };
        self.consumer = Some(consumer);
        let first_capture_ns = self
            .shared
            .started
            .load(Ordering::Acquire)
            .then(|| self.shared.first_capture_ns.load(Ordering::Acquire));
        Some(written.map(|(path, seconds)| FinishedTake {
            file: take.file,
            path,
            seconds,
            start_beats: take_start_beats(anchor, first_capture_ns, take.fallback_beats),
            requested_beats: take.fallback_beats,
            dropped_samples: self.shared.dropped.load(Ordering::Relaxed),
        }))
    }
}

impl Drop for AudioRecorder {
    fn drop(&mut self) {
        if let Some(take) = self.take.take() {
            self.shared.armed.store(false, Ordering::Release);
            take.stop.store(true, Ordering::Release);
            let _ = take.thread.join();
        }
    }
}

fn write_loop(
    mut consumer: rtrb::Consumer<f32>,
    mut writer: WavWriter,
    stop: &AtomicBool,
    shared: &InputShared,
    status: &EngineStatus,
) -> WriterResult {
    let mut chunk = Vec::with_capacity(RING_SAMPLES);
    let mut anchor = None;
    let mut error = None;
    loop {
        let stopping = stop.load(Ordering::Acquire);
        // Read the clock once the take and playback have both started: the
        // mapping from device time to beats is exact while playback runs
        // steadily. (Recording often starts a moment before the transport.)
        if !anchor.is_some_and(|a: ClockAnchor| a.playing) && shared.started.load(Ordering::Acquire)
        {
            anchor = status.clock();
        }
        chunk.clear();
        while let Ok(s) = consumer.pop() {
            chunk.push(s);
        }
        if error.is_none()
            && let Err(e) = writer.write(&chunk)
        {
            error = Some(e);
        }
        if stopping {
            break;
        }
        std::thread::sleep(WRITER_PERIOD);
    }
    let result = match error {
        Some(e) => {
            writer.discard();
            Err(e)
        }
        None => writer.finish(),
    };
    (consumer, result, anchor)
}

/// Where a take belongs on the timeline: the beat that was playing when its
/// first sample was captured (negative if that was before the song start).
/// Falls back when the clocks are unusable.
pub fn take_start_beats(
    anchor: Option<ClockAnchor>,
    first_capture_ns: Option<u64>,
    fallback_beats: f64,
) -> f64 {
    match (anchor, first_capture_ns) {
        (Some(a), Some(ns))
            if a.playing
                && a.tempo_bpm > 0.0
                && (ns as f64 - a.playback_ns as f64).abs() < MAX_CLOCK_GAP_NS =>
        {
            a.beats_at(ns)
        }
        _ => fallback_beats.max(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(ns: u64, beats: f64) -> ClockAnchor {
        ClockAnchor {
            playback_ns: ns,
            position_beats: beats,
            tempo_bpm: 120.0,
            playing: true,
        }
    }

    #[test]
    fn take_starts_at_the_beat_heard_when_capture_began() {
        // The speakers play beat 8 at t = 10 s. The mic captured its first
        // sample at 10.25 s, when beat 8.5 was being heard.
        let a = anchor(10_000_000_000, 8.0);
        let beats = take_start_beats(Some(a), Some(10_250_000_000), 0.0);
        assert!((beats - 8.5).abs() < 1e-9);
        // Captured before the anchor's buffer was heard: earlier beat.
        let beats = take_start_beats(Some(a), Some(9_900_000_000), 0.0);
        assert!((beats - 7.8).abs() < 1e-9);
        // Capture began 20 ms before the song started at beat 0: the take
        // starts before 0 (the caller trims that part, keeping alignment).
        let a = anchor(10_000_000_000, 0.0);
        let beats = take_start_beats(Some(a), Some(9_980_000_000), 0.0);
        assert!((beats + 0.04).abs() < 1e-9);
    }

    #[test]
    fn unusable_clocks_fall_back() {
        let a = anchor(10_000_000_000, 8.0);
        assert_eq!(take_start_beats(None, Some(1), 4.0), 4.0);
        assert_eq!(take_start_beats(Some(a), None, 4.0), 4.0);
        // Clocks from different epochs.
        assert_eq!(take_start_beats(Some(a), Some(999_000_000_000), 4.0), 4.0);
        let stopped = ClockAnchor {
            playing: false,
            ..a
        };
        assert_eq!(
            take_start_beats(Some(stopped), Some(10_000_000_000), 4.0),
            4.0
        );
    }

    #[test]
    fn records_mono_mix_of_input_while_armed() {
        let dir = tempfile::tempdir().expect("tmp");
        let (mut capture, mut recorder) = input_pair(48_000);
        let status = Arc::new(EngineStatus::default());
        // Not armed: metered, not recorded.
        capture.process(&[0.5, 0.5, -0.25, -0.25], 2, 0);
        assert!((recorder.take_level() - 0.5).abs() < 1e-6);
        let path = dir.path().join("take.wav");
        recorder
            .start("take.wav".into(), path.clone(), Arc::clone(&status), 2.0)
            .expect("start");
        capture.process(&[0.2, 0.4, 0.6, 0.8], 2, 123);
        std::thread::sleep(Duration::from_millis(40));
        capture.process(&[1.0, 0.0], 2, 456);
        let take = recorder.stop().expect("was recording").expect("ok");
        assert_eq!(take.file, "take.wav");
        // No playback clock in this test: starts at the fallback beat.
        assert_eq!(take.start_beats, 2.0);
        assert_eq!(take.dropped_samples, 0);
        let data = daw_audio::read_wav(&path).expect("read");
        assert_eq!(data.channels.len(), 1);
        let got: Vec<f32> = data.channels[0].clone();
        let want = [0.3, 0.7, 0.5];
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 1e-6, "{got:?}");
        }
        assert!(!recorder.is_recording());
        // A second take works after the first.
        recorder
            .start("t2.wav".into(), dir.path().join("t2.wav"), status, 0.0)
            .expect("again");
        assert!(recorder.stop().expect("take").is_ok());
    }
}
