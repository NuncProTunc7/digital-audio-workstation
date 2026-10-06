//! Sound card output.
//!
//! The cpal stream lives on its own thread for its whole life, because some
//! platforms do not allow moving a stream between threads. The rest of the app
//! talks to it through atomics, which the audio callback can read without
//! locking.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use thiserror::Error;

use crate::ToneGenerator;

const TEST_TONE_FREQ_HZ: f64 = 440.0;
const TEST_TONE_GAIN: f32 = 0.2;

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("no audio output device found")]
    NoOutputDevice,
    #[error("audio output device does not support 32-bit float output (format: {0})")]
    UnsupportedFormat(String),
    #[error("audio device error: {0}")]
    Backend(String),
    #[error("audio thread stopped unexpectedly")]
    ThreadDied,
}

/// Names of the available output devices on the default host (WASAPI on Windows).
pub fn output_device_names() -> Vec<String> {
    let host = cpal::default_host();
    match host.output_devices() {
        Ok(devices) => devices.map(|d| d.to_string()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Name of the system's default output device, if there is one.
pub fn default_output_device_name() -> Option<String> {
    cpal::default_host()
        .default_output_device()
        .map(|d| d.to_string())
}

/// Plays a test tone on the default output device.
///
/// Dropping the player stops the stream and joins its thread.
pub struct TestTonePlayer {
    tone_on: Arc<AtomicBool>,
    shutdown_tx: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    device_name: String,
    sample_rate_hz: u32,
}

impl TestTonePlayer {
    /// Opens the default output device. The tone starts silent.
    pub fn open_default() -> Result<Self, DeviceError> {
        let tone_on = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel();
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
        let tone_flag = Arc::clone(&tone_on);

        let thread = std::thread::Builder::new()
            .name("npt-audio-output".into())
            .spawn(move || {
                let stream = match build_stream(tone_flag) {
                    Ok((stream, name, rate)) => {
                        let _ = ready_tx.send(Ok((name, rate)));
                        stream
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                // Park until the player is dropped; the stream runs meanwhile.
                let _ = shutdown_rx.recv();
                drop(stream);
            })
            .map_err(|e| DeviceError::Backend(e.to_string()))?;

        let (device_name, sample_rate_hz) =
            ready_rx.recv().map_err(|_| DeviceError::ThreadDied)??;
        Ok(Self {
            tone_on,
            shutdown_tx: Some(shutdown_tx),
            thread: Some(thread),
            device_name,
            sample_rate_hz,
        })
    }

    pub fn set_tone_on(&self, on: bool) {
        self.tone_on.store(on, Ordering::Relaxed);
    }

    pub fn is_tone_on(&self) -> bool {
        self.tone_on.load(Ordering::Relaxed)
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn sample_rate_hz(&self) -> u32 {
        self.sample_rate_hz
    }
}

impl Drop for TestTonePlayer {
    fn drop(&mut self) {
        drop(self.shutdown_tx.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn build_stream(tone_on: Arc<AtomicBool>) -> Result<(cpal::Stream, String, u32), DeviceError> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or(DeviceError::NoOutputDevice)?;
    let supported = device
        .default_output_config()
        .map_err(|e| DeviceError::Backend(e.to_string()))?;
    if supported.sample_format() != cpal::SampleFormat::F32 {
        // WASAPI shared mode always mixes in f32, so this only triggers on
        // unusual hosts. Other formats arrive with the full engine in Phase 1.
        return Err(DeviceError::UnsupportedFormat(
            supported.sample_format().to_string(),
        ));
    }
    let config: cpal::StreamConfig = supported.into();
    let channels = usize::from(config.channels);
    let sample_rate_hz = config.sample_rate;

    let mut tone = ToneGenerator::new(sample_rate_hz, TEST_TONE_FREQ_HZ, TEST_TONE_GAIN);
    let stream = device
        .build_output_stream(
            config,
            // RT-SAFE: reads one atomic and runs the generator.
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                tone.set_on(tone_on.load(Ordering::Relaxed));
                tone.process(data, channels);
            },
            // Called off the audio thread; logging belongs here, not in the callback.
            |err| eprintln!("audio stream error: {err}"),
            None,
        )
        .map_err(|e| DeviceError::Backend(e.to_string()))?;
    stream
        .play()
        .map_err(|e| DeviceError::Backend(e.to_string()))?;
    Ok((stream, device.to_string(), sample_rate_hz))
}
