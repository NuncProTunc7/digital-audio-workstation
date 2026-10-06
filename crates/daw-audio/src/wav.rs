//! The project's own audio files: 32-bit float WAV, mono or stereo.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use crate::{AudioData, AudioError};

fn io_err(path: &Path, e: impl ToString) -> AudioError {
    AudioError::Io {
        path: path.to_owned(),
        message: e.to_string(),
    }
}

fn spec(channels: u16, sample_rate_hz: u32) -> hound::WavSpec {
    hound::WavSpec {
        channels,
        sample_rate: sample_rate_hz,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    }
}

/// Writes audio as a float WAV, via a temporary file so a crash never
/// leaves a half-written file under the real name.
pub fn write_wav(path: &Path, data: &AudioData) -> Result<(), AudioError> {
    let channels = data.channels.len().clamp(1, 2);
    let tmp = path.with_extension("wav.tmp");
    let mut w = hound::WavWriter::create(&tmp, spec(channels as u16, data.sample_rate_hz))
        .map_err(|e| io_err(&tmp, e))?;
    for i in 0..data.frames() {
        for ch in &data.channels[..channels] {
            w.write_sample(ch[i]).map_err(|e| io_err(&tmp, e))?;
        }
    }
    w.finalize().map_err(|e| io_err(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| io_err(path, e))
}

/// Reads a WAV file (any bit depth) into float samples.
pub fn read_wav(path: &Path) -> Result<AudioData, AudioError> {
    let mut r = hound::WavReader::open(path).map_err(|e| io_err(path, e))?;
    let s = r.spec();
    let channels = usize::from(s.channels.max(1));
    let samples: Vec<f32> = match s.sample_format {
        hound::SampleFormat::Float => r
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(|e| io_err(path, e))?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (s.bits_per_sample.clamp(1, 32) - 1)) as f32;
            r.samples::<i32>()
                .map(|x| x.map(|v| v as f32 * scale))
                .collect::<Result<_, _>>()
                .map_err(|e| io_err(path, e))?
        }
    };
    let keep = channels.min(2);
    let mut out = vec![Vec::with_capacity(samples.len() / channels); keep];
    for frame in samples.chunks_exact(channels) {
        for (c, o) in out.iter_mut().enumerate() {
            o.push(frame[c]);
        }
    }
    Ok(AudioData {
        sample_rate_hz: s.sample_rate,
        channels: out,
    })
}

/// Streams a recording to disk as it arrives (mono float WAV). The file
/// gets its final name only when [`finish`](Self::finish) succeeds.
pub struct WavWriter {
    writer: hound::WavWriter<BufWriter<File>>,
    tmp: PathBuf,
    path: PathBuf,
    frames: u64,
    sample_rate_hz: u32,
}

impl WavWriter {
    pub fn create(path: &Path, sample_rate_hz: u32) -> Result<Self, AudioError> {
        let tmp = path.with_extension("wav.part");
        let writer =
            hound::WavWriter::create(&tmp, spec(1, sample_rate_hz)).map_err(|e| io_err(&tmp, e))?;
        Ok(Self {
            writer,
            tmp,
            path: path.to_owned(),
            frames: 0,
            sample_rate_hz,
        })
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<(), AudioError> {
        for &s in samples {
            self.writer
                .write_sample(s)
                .map_err(|e| io_err(&self.tmp, e))?;
        }
        self.frames += samples.len() as u64;
        Ok(())
    }

    pub fn seconds(&self) -> f64 {
        self.frames as f64 / f64::from(self.sample_rate_hz.max(1))
    }

    /// Finalizes the file. Returns its path and length in seconds.
    pub fn finish(self) -> Result<(PathBuf, f64), AudioError> {
        let seconds = self.seconds();
        self.writer.finalize().map_err(|e| io_err(&self.tmp, e))?;
        std::fs::rename(&self.tmp, &self.path).map_err(|e| io_err(&self.path, e))?;
        Ok((self.path, seconds))
    }

    /// Throws the recording away.
    pub fn discard(self) {
        let _ = self.writer.finalize();
        let _ = std::fs::remove_file(&self.tmp);
    }
}
