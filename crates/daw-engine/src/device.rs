//! Sound card output and microphone input.
//!
//! The cpal stream lives on its own thread for its whole life, because some
//! platforms do not allow moving a stream between threads. The audio callback
//! owns the [`AudioProcessor`]; the rest of the app talks to it through the
//! [`Engine`] handle.

use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Instant;

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

/// A running output stream. Dropping it stops the sound and joins its thread.
pub struct AudioOutput {
    shutdown_tx: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    device_name: String,
    sample_rate_hz: u32,
}

struct Opened {
    engine: Engine,
    device_name: String,
    sample_rate_hz: u32,
}

impl AudioOutput {
    /// Opens `device_name` (or the system default) and starts an engine for
    /// `project` at the device's sample rate.
    pub fn start(
        device_name: Option<&str>,
        project: &Project,
        audio: Arc<AudioPool>,
    ) -> Result<(Engine, AudioOutput), DeviceError> {
        let (ready_tx, ready_rx) = mpsc::channel::<Result<Opened, DeviceError>>();
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
        let device_name = device_name.map(str::to_owned);
        let project = project.clone();

        let thread = std::thread::Builder::new()
            .name("npt-audio-output".into())
            .spawn(move || {
                let stream = match open_stream(device_name.as_deref(), &project, audio) {
                    Ok((stream, opened)) => {
                        let _ = ready_tx.send(Ok(opened));
                        stream
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                // Park until the output is dropped; the stream runs meanwhile.
                let _ = shutdown_rx.recv();
                drop(stream);
            })
            .map_err(|e| DeviceError::Backend(e.to_string()))?;

        let opened = ready_rx.recv().map_err(|_| DeviceError::ThreadDied)??;
        let output = AudioOutput {
            shutdown_tx: Some(shutdown_tx),
            thread: Some(thread),
            device_name: opened.device_name,
            sample_rate_hz: opened.sample_rate_hz,
        };
        Ok((opened.engine, output))
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

fn open_stream(
    device_name: Option<&str>,
    project: &Project,
    audio: Arc<AudioPool>,
) -> Result<(cpal::Stream, Opened), DeviceError> {
    let device = find_device(device_name)?;
    let supported = device
        .default_output_config()
        .map_err(|e| DeviceError::Backend(e.to_string()))?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let sample_rate_hz = config.sample_rate;
    let (engine, processor) = Engine::with_audio(project, sample_rate_hz, audio);

    let stream = match format {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, processor),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, processor),
        cpal::SampleFormat::I32 => build::<i32>(&device, &config, processor),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, processor),
        cpal::SampleFormat::F64 => build::<f64>(&device, &config, processor),
        other => Err(DeviceError::UnsupportedFormat(other.to_string())),
    }?;
    stream
        .play()
        .map_err(|e| DeviceError::Backend(e.to_string()))?;
    Ok((
        stream,
        Opened {
            engine,
            device_name: device.to_string(),
            sample_rate_hz,
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
                processor.set_output_time(info.timestamp().playback.as_nanos() as u64);
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

/// A running input stream. Dropping it closes the microphone.
pub struct AudioInput {
    shutdown_tx: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    device_name: String,
    sample_rate_hz: u32,
}

impl AudioInput {
    /// Opens `device_name` (or the default input). Returns the stream and
    /// the recorder that turns its sound into takes.
    pub fn start(device_name: Option<&str>) -> Result<(AudioInput, AudioRecorder), DeviceError> {
        type Ready = Result<(AudioRecorder, String, u32), DeviceError>;
        let (ready_tx, ready_rx) = mpsc::channel::<Ready>();
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
        let device_name = device_name.map(str::to_owned);
        let thread = std::thread::Builder::new()
            .name("npt-audio-input".into())
            .spawn(move || {
                let stream = match open_input(device_name.as_deref()) {
                    Ok((stream, recorder, name, rate)) => {
                        let _ = ready_tx.send(Ok((recorder, name, rate)));
                        stream
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let _ = shutdown_rx.recv();
                drop(stream);
            })
            .map_err(|e| DeviceError::Backend(e.to_string()))?;
        let (recorder, device_name, sample_rate_hz) =
            ready_rx.recv().map_err(|_| DeviceError::ThreadDied)??;
        Ok((
            AudioInput {
                shutdown_tx: Some(shutdown_tx),
                thread: Some(thread),
                device_name,
                sample_rate_hz,
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
        cpal::SampleFormat::F32 => build_input::<f32>(&device, &config, capture),
        cpal::SampleFormat::I16 => build_input::<i16>(&device, &config, capture),
        cpal::SampleFormat::I32 => build_input::<i32>(&device, &config, capture),
        cpal::SampleFormat::U16 => build_input::<u16>(&device, &config, capture),
        cpal::SampleFormat::U8 => build_input::<u8>(&device, &config, capture),
        cpal::SampleFormat::F64 => build_input::<f64>(&device, &config, capture),
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
                let base_ns = info.timestamp().capture.as_nanos() as u64;
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
            |err| eprintln!("audio input error: {err}"),
            None,
        )
        .map_err(|e| DeviceError::Backend(e.to_string()))
}
