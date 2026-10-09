//! Sound card output and microphone input.
//!
//! The cpal stream lives on its own thread for its whole life, because some
//! platforms do not allow moving a stream between threads. The audio callback
//! owns the [`AudioProcessor`]; the rest of the app talks to it through the
//! [`Engine`] handle.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use daw_model::Project;
use thiserror::Error;

use crate::capture::{AudioRecorder, InputCapture, input_pair};
use crate::{AudioPool, AudioProcessor, Engine, MAX_BLOCK_FRAMES};
use std::sync::Arc;

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("no audio output device found")]
    NoOutputDevice,
    #[error("no microphone or audio input found")]
    NoInputDevice,
    #[error("audio output device \"{0}\" was not found")]
    DeviceNotFound(String),
    #[error("audio output format {0} is not supported")]
    UnsupportedFormat(String),
    #[error("audio device error: {0}")]
    Backend(String),
    #[error("audio thread stopped unexpectedly")]
    ThreadDied,
    #[error(
        "Microphone unavailable: it didn't answer within {0} seconds. Check that it is connected and not in use by another app, or choose another input."
    )]
    InputTimedOut(u64),
    #[error(
        "Sound output unavailable: the device didn't answer within {0} seconds. Check that it is connected, or choose another output."
    )]
    OutputTimedOut(u64),
}

/// How long opening a sound card or microphone may take. Bluetooth headsets
/// can take a few seconds to switch modes; one that never answers must not
/// freeze the app.
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(8);

/// Opens a stream with `open` on a thread of its own (some platforms don't
/// allow moving a stream between threads) and keeps it there until the
/// returned sender is dropped. Gives up waiting after `timeout` with
/// `timed_out`; a stream that opens later is closed again at once.
fn open_on_thread<S, T: Send + 'static>(
    name: &str,
    timeout: Duration,
    timed_out: DeviceError,
    open: impl FnOnce() -> Result<(S, T), DeviceError> + Send + 'static,
) -> Result<(T, mpsc::Sender<()>, JoinHandle<()>), DeviceError> {
    let (ready_tx, ready_rx) = mpsc::channel::<Result<T, DeviceError>>();
    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
    let thread = std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let stream = match open() {
                Ok((stream, ready)) => {
                    if ready_tx.send(Ok(ready)).is_err() {
                        // Nobody waited any longer: close it again.
                        return;
                    }
                    stream
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            // Park until the owner is dropped; the stream runs meanwhile.
            let _ = shutdown_rx.recv();
            drop(stream);
        })
        .map_err(|e| DeviceError::Backend(e.to_string()))?;
    match ready_rx.recv_timeout(timeout) {
        Ok(Ok(ready)) => Ok((ready, shutdown_tx, thread)),
        Ok(Err(e)) => Err(e),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(timed_out),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(DeviceError::ThreadDied),
    }
}

/// Nanoseconds on one clock shared by every stream in the app.
///
/// Sound cards timestamp their buffers, but some systems count from each
/// stream's own start, so an input and an output timestamp can't be compared
/// directly. Each callback instead measures how far its buffer is from "now"
/// on its own stream's clock and applies that to this shared clock.
// RT-SAFE: reads the monotonic clock; the epoch is set before streams open.
pub fn clock_ns() -> u64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

/// Names of the available output devices on the default host (WASAPI on Windows).
pub fn output_device_names() -> Vec<String> {
    match cpal::default_host().output_devices() {
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

/// Buffer sizes offered to the user, in frames. Smaller answers sooner but
/// needs more spare CPU.
pub const BUFFER_SIZES: [u32; 5] = [128, 256, 512, 1024, 2048];

/// A running output stream. Dropping it stops the sound and joins its thread.
pub struct AudioOutput {
    shutdown_tx: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    device_name: String,
    sample_rate_hz: u32,
    buffer_frames: Option<u32>,
}

struct Opened {
    engine: Engine,
    device_name: String,
    sample_rate_hz: u32,
    buffer_frames: Option<u32>,
}

impl AudioOutput {
    /// Opens `device_name` (or the system default) and starts an engine for
    /// `project` at the device's sample rate. `buffer_frames` asks for a
    /// fixed buffer size; if the device refuses it, its default is used.
    pub fn start(
        device_name: Option<&str>,
        buffer_frames: Option<u32>,
        project: &Project,
        audio: Arc<AudioPool>,
    ) -> Result<(Engine, AudioOutput), DeviceError> {
        // Fix the shared clock's epoch here, not in a callback.
        clock_ns();
        let device_name = device_name.map(str::to_owned);
        let project = project.clone();
        let (opened, shutdown_tx, thread) = open_on_thread(
            "npt-audio-output",
            OPEN_TIMEOUT,
            DeviceError::OutputTimedOut(OPEN_TIMEOUT.as_secs()),
            move || open_stream(device_name.as_deref(), buffer_frames, &project, audio),
        )?;
        let output = AudioOutput {
            shutdown_tx: Some(shutdown_tx),
            thread: Some(thread),
            device_name: opened.device_name,
            sample_rate_hz: opened.sample_rate_hz,
            buffer_frames: opened.buffer_frames,
        };
        Ok((opened.engine, output))
    }

    /// The fixed buffer size in use, or None for the device's default.
    pub fn buffer_frames(&self) -> Option<u32> {
        self.buffer_frames
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn sample_rate_hz(&self) -> u32 {
        self.sample_rate_hz
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        drop(self.shutdown_tx.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn find_device(name: Option<&str>) -> Result<cpal::Device, DeviceError> {
    let host = cpal::default_host();
    match name {
        None => host
            .default_output_device()
            .ok_or(DeviceError::NoOutputDevice),
        Some(name) => host
            .output_devices()
            .map_err(|e| DeviceError::Backend(e.to_string()))?
            .find(|d| d.to_string() == name)
            .ok_or_else(|| DeviceError::DeviceNotFound(name.to_owned())),
    }
}

/// The buffer size to ask for: `wanted` fitted into what the device says it
/// supports (None = the device's default).
fn fixed_buffer(wanted: Option<u32>, supported: &cpal::SupportedBufferSize) -> Option<u32> {
    let frames = wanted?.clamp(1, BUFFER_SIZES[BUFFER_SIZES.len() - 1]);
    match *supported {
        cpal::SupportedBufferSize::Range { min, max } => Some(frames.clamp(min, max.max(min))),
        cpal::SupportedBufferSize::Unknown => Some(frames),
    }
}

fn open_stream(
    device_name: Option<&str>,
    buffer_frames: Option<u32>,
    project: &Project,
    audio: Arc<AudioPool>,
) -> Result<(cpal::Stream, Opened), DeviceError> {
    let device = find_device(device_name)?;
    let supported = device
        .default_output_config()
        .map_err(|e| DeviceError::Backend(e.to_string()))?;
    let format = supported.sample_format();
    let fixed = fixed_buffer(buffer_frames, supported.buffer_size());
    let mut config: cpal::StreamConfig = supported.into();
    let sample_rate_hz = config.sample_rate;
    let open = |config: &cpal::StreamConfig| {
        let (engine, processor) = Engine::with_audio(project, sample_rate_hz, Arc::clone(&audio));
        let stream = match format {
            cpal::SampleFormat::F32 => build::<f32>(&device, config, processor),
            cpal::SampleFormat::I16 => build::<i16>(&device, config, processor),
            cpal::SampleFormat::I32 => build::<i32>(&device, config, processor),
            cpal::SampleFormat::U16 => build::<u16>(&device, config, processor),
            cpal::SampleFormat::F64 => build::<f64>(&device, config, processor),
            other => Err(DeviceError::UnsupportedFormat(other.to_string())),
        }?;
        stream
            .play()
            .map_err(|e| DeviceError::Backend(e.to_string()))?;
        Ok::<_, DeviceError>((stream, engine))
    };
    let mut used = None;
    let (stream, engine) = match fixed {
        Some(frames) => {
            config.buffer_size = cpal::BufferSize::Fixed(frames);
            match open(&config) {
                Ok(o) => {
                    used = Some(frames);
                    o
                }
                // Some drivers refuse sizes they claim to support: fall back.
                Err(_) => {
                    config.buffer_size = cpal::BufferSize::Default;
                    open(&config)?
                }
            }
        }
        None => open(&config)?,
    };
    Ok((
        stream,
        Opened {
            engine,
            device_name: device.to_string(),
            sample_rate_hz,
            buffer_frames: used,
        },
    ))
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut processor: AudioProcessor,
) -> Result<cpal::Stream, DeviceError>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels).max(1);
    let sample_rate = config.sample_rate as f32;
    // Scratch for format conversion; sized once, here, off the audio thread.
    let mut scratch = vec![0.0f32; MAX_BLOCK_FRAMES * channels];
    let status = processor.status_handle();

    device
        .build_output_stream(
            *config,
            // RT-SAFE: renders into preallocated scratch and converts.
            move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                let started = Instant::now();
                let t = info.timestamp();
                let ahead = t.playback.saturating_duration_since(t.callback).as_nanos() as u64;
                processor.set_output_time(clock_ns() + ahead);
                for chunk in data.chunks_mut(scratch.len()) {
                    let buf = &mut scratch[..chunk.len()];
                    processor.process_interleaved(buf, channels);
                    for (out, &s) in chunk.iter_mut().zip(buf.iter()) {
                        *out = T::from_sample(s);
                    }
                }
                let frames = data.len() / channels;
                if frames > 0 {
                    let budget_s = frames as f32 / sample_rate;
                    let load = started.elapsed().as_secs_f32() / budget_s;
                    status.set_callback_stats(load, frames as u32);
                }
            },
            // Called off the audio thread; logging belongs here, not in the callback.
            |err| eprintln!("audio stream error: {err}"),
            None,
        )
        .map_err(|e| DeviceError::Backend(e.to_string()))
}

/// Names of the available inputs (microphones, line-ins).
pub fn input_device_names() -> Vec<String> {
    match cpal::default_host().input_devices() {
        Ok(devices) => devices.map(|d| d.to_string()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Name of the system's default input, if there is one.
pub fn default_input_device_name() -> Option<String> {
    cpal::default_host()
        .default_input_device()
        .map(|d| d.to_string())
}

/// Whether an open microphone should be closed and opened again: it
/// follows the system default (`chosen` is None) and Windows switched the
/// default (say a Bluetooth headset was turned on), or it disappeared from
/// the device list, or its stream reported an error (`lost`). An empty
/// `devices` list means the listing failed, so it isn't taken as "gone".
pub fn input_should_reopen(
    open: &str,
    chosen: Option<&str>,
    default: Option<&str>,
    devices: &[String],
    lost: bool,
) -> bool {
    let default_moved = chosen.is_none() && default.is_some_and(|d| d != open);
    let gone = !devices.is_empty() && !devices.iter().any(|d| d == open);
    lost || default_moved || gone
}

/// A running input stream. Dropping it closes the microphone.
pub struct AudioInput {
    shutdown_tx: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    device_name: String,
    sample_rate_hz: u32,
    /// Set when the stream reports an error (the device went away).
    lost: Arc<AtomicBool>,
}

impl AudioInput {
    /// Opens `device_name` (or the default input). Returns the stream and
    /// the recorder that turns its sound into takes.
    pub fn start(device_name: Option<&str>) -> Result<(AudioInput, AudioRecorder), DeviceError> {
        clock_ns();
        let device_name = device_name.map(str::to_owned);
        let lost = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&lost);
        let ((recorder, device_name, sample_rate_hz), shutdown_tx, thread) = open_on_thread(
            "npt-audio-input",
            OPEN_TIMEOUT,
            DeviceError::InputTimedOut(OPEN_TIMEOUT.as_secs()),
            move || {
                open_input(device_name.as_deref(), flag)
                    .map(|(stream, recorder, name, rate)| (stream, (recorder, name, rate)))
            },
        )?;
        Ok((
            AudioInput {
                shutdown_tx: Some(shutdown_tx),
                thread: Some(thread),
                device_name,
                sample_rate_hz,
                lost,
            },
            recorder,
        ))
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn sample_rate_hz(&self) -> u32 {
        self.sample_rate_hz
    }

    /// True once the stream has reported an error, such as its device
    /// being switched off; it then delivers no more sound.
    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Relaxed)
    }
}

impl Drop for AudioInput {
    fn drop(&mut self) {
        drop(self.shutdown_tx.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn open_input(
    device_name: Option<&str>,
    lost: Arc<AtomicBool>,
) -> Result<(cpal::Stream, AudioRecorder, String, u32), DeviceError> {
    let host = cpal::default_host();
    let device = match device_name {
        None => host
            .default_input_device()
            .ok_or(DeviceError::NoInputDevice)?,
        Some(name) => host
            .input_devices()
            .map_err(|e| DeviceError::Backend(e.to_string()))?
            .find(|d| d.to_string() == name)
            .ok_or_else(|| DeviceError::DeviceNotFound(name.to_owned()))?,
    };
    let supported = device
        .default_input_config()
        .map_err(|e| DeviceError::Backend(e.to_string()))?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let (capture, recorder) = input_pair(config.sample_rate);
    let stream = match format {
        cpal::SampleFormat::F32 => build_input::<f32>(&device, &config, capture, Arc::clone(&lost)),
        cpal::SampleFormat::I16 => build_input::<i16>(&device, &config, capture, Arc::clone(&lost)),
        cpal::SampleFormat::I32 => build_input::<i32>(&device, &config, capture, Arc::clone(&lost)),
        cpal::SampleFormat::U16 => build_input::<u16>(&device, &config, capture, Arc::clone(&lost)),
        cpal::SampleFormat::U8 => build_input::<u8>(&device, &config, capture, Arc::clone(&lost)),
        cpal::SampleFormat::F64 => build_input::<f64>(&device, &config, capture, Arc::clone(&lost)),
        other => Err(DeviceError::UnsupportedFormat(other.to_string())),
    }?;
    stream
        .play()
        .map_err(|e| DeviceError::Backend(e.to_string()))?;
    Ok((stream, recorder, device.to_string(), config.sample_rate))
}

fn build_input<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut capture: InputCapture,
    lost: Arc<AtomicBool>,
) -> Result<cpal::Stream, DeviceError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels).max(1);
    let ns_per_frame = 1e9 / f64::from(config.sample_rate.max(1));
    // Conversion scratch, sized once here, off the audio thread.
    let mut scratch = vec![0.0f32; 4096 * channels];
    device
        .build_input_stream(
            *config,
            // RT-SAFE: converts into preallocated scratch and pushes to a ring.
            move |data: &[T], info: &cpal::InputCallbackInfo| {
                let t = info.timestamp();
                let behind = t.callback.saturating_duration_since(t.capture).as_nanos() as u64;
                let base_ns = clock_ns().saturating_sub(behind);
                let mut frames_done = 0usize;
                for chunk in data.chunks(scratch.len()) {
                    let buf = &mut scratch[..chunk.len()];
                    for (o, &s) in buf.iter_mut().zip(chunk) {
                        *o = <f32 as FromSample<T>>::from_sample_(s);
                    }
                    let ns = base_ns + (frames_done as f64 * ns_per_frame) as u64;
                    capture.process(buf, channels, ns);
                    frames_done += chunk.len() / channels;
                }
            },
            // Runs on cpal's own thread, not the audio callback.
            move |err| {
                eprintln!("audio input error: {err}");
                lost.store(true, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| DeviceError::Backend(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_requests_fit_what_the_device_supports() {
        let range = cpal::SupportedBufferSize::Range {
            min: 256,
            max: 4096,
        };
        assert_eq!(fixed_buffer(None, &range), None);
        assert_eq!(fixed_buffer(Some(512), &range), Some(512));
        assert_eq!(fixed_buffer(Some(128), &range), Some(256));
        assert_eq!(fixed_buffer(Some(99_999), &range), Some(2048));
        let unknown = cpal::SupportedBufferSize::Unknown;
        assert_eq!(fixed_buffer(Some(1024), &unknown), Some(1024));
    }

    #[test]
    fn a_device_that_never_answers_gives_up_instead_of_freezing() {
        let started = Instant::now();
        let result = open_on_thread(
            "test-slow-device",
            Duration::from_millis(100),
            DeviceError::InputTimedOut(0),
            || {
                std::thread::sleep(Duration::from_secs(2));
                Ok(((), 7u32))
            },
        );
        assert!(matches!(result, Err(DeviceError::InputTimedOut(_))));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(
            DeviceError::InputTimedOut(8)
                .to_string()
                .starts_with("Microphone unavailable")
        );
        // A device that answers in time is kept open until dropped.
        let (value, shutdown, thread) = open_on_thread(
            "test-device",
            Duration::from_secs(5),
            DeviceError::ThreadDied,
            || Ok(((), 7u32)),
        )
        .expect("opens");
        assert_eq!(value, 7);
        drop(shutdown);
        thread.join().expect("closes");
    }

    #[test]
    fn the_microphone_reopens_when_its_device_changes() {
        let devices = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        let both = devices(&["Realtek Mic", "Headset Mic"]);
        // Following the system default: a headset turned on becomes the
        // default, so the open Realtek mic must give way to it.
        assert!(input_should_reopen(
            "Realtek Mic",
            None,
            Some("Headset Mic"),
            &both,
            false
        ));
        assert!(!input_should_reopen(
            "Realtek Mic",
            None,
            Some("Realtek Mic"),
            &both,
            false
        ));
        // A microphone the user picked stays, whatever the default does.
        assert!(!input_should_reopen(
            "Realtek Mic",
            Some("Realtek Mic"),
            Some("Headset Mic"),
            &both,
            false
        ));
        // The open microphone was unplugged or switched off.
        assert!(input_should_reopen(
            "Headset Mic",
            Some("Headset Mic"),
            Some("Realtek Mic"),
            &devices(&["Realtek Mic"]),
            false
        ));
        // An empty list means Windows didn't answer, not that it's gone.
        assert!(!input_should_reopen(
            "Realtek Mic",
            Some("Realtek Mic"),
            None,
            &[],
            false
        ));
        // The stream reported an error (the device went away mid-stream).
        assert!(input_should_reopen(
            "Realtek Mic",
            Some("Realtek Mic"),
            Some("Realtek Mic"),
            &both,
            true
        ));
    }
}
