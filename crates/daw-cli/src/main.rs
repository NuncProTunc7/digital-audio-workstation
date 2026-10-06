//! `npt`: Nunc Pro Tune without the UI.
//!
//! Used by tests and CI today; later by Claude for render-and-analyze tasks.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use daw_engine::DEFAULT_SAMPLE_RATE_HZ;

#[derive(Parser)]
#[command(name = "npt", version, about = "Nunc Pro Tune command-line tool")]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Subcommand)]
enum CliCommand {
    /// Render a sine test tone to a WAV file.
    RenderTestTone {
        /// Output WAV path.
        #[arg(long)]
        out: PathBuf,
        /// Tone frequency in hertz.
        #[arg(long, default_value_t = 440.0)]
        freq_hz: f64,
        /// Length in seconds.
        #[arg(long, default_value_t = 2.0)]
        seconds: f64,
        /// Peak level, 0.0–1.0.
        #[arg(long, default_value_t = 0.5)]
        gain: f32,
    },
    /// Print the JSON Schema of every project Command (what Claude can do).
    Schema,
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        CliCommand::RenderTestTone {
            out,
            freq_hz,
            seconds,
            gain,
        } => render_test_tone(&out, freq_hz, seconds, gain),
        CliCommand::Schema => {
            println!("{:#}", daw_model::command_schema());
            Ok(())
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn render_test_tone(out: &Path, freq_hz: f64, seconds: f64, gain: f32) -> Result<(), String> {
    if !(seconds.is_finite() && (0.0..=600.0).contains(&seconds)) {
        return Err(format!("seconds must be between 0 and 600, got {seconds}"));
    }
    let samples =
        daw_engine::offline::render_test_tone(DEFAULT_SAMPLE_RATE_HZ, freq_hz, gain, seconds);
    write_wav(out, &samples, DEFAULT_SAMPLE_RATE_HZ).map_err(|e| e.to_string())?;
    println!("wrote {} ({seconds} s, {freq_hz} Hz)", out.display());
    Ok(())
}

/// Writes interleaved stereo f32 samples as a 32-bit float WAV file.
fn write_wav(path: &Path, samples: &[f32], sample_rate_hz: u32) -> Result<(), hound::Error> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: sample_rate_hz,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &sample in samples {
        writer.write_sample(sample)?;
    }
    writer.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_wav_reads_back_identically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tone.wav");
        render_test_tone(&path, 440.0, 0.5, 0.5).expect("render");

        let mut reader = hound::WavReader::open(&path).expect("open");
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.spec().sample_rate, DEFAULT_SAMPLE_RATE_HZ);
        let read: Vec<f32> = reader
            .samples::<f32>()
            .map(|s| s.expect("sample"))
            .collect();
        let expected =
            daw_engine::offline::render_test_tone(DEFAULT_SAMPLE_RATE_HZ, 440.0, 0.5, 0.5);
        assert_eq!(read, expected);
    }

    #[test]
    fn rejects_absurd_lengths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tone.wav");
        assert!(render_test_tone(&path, 440.0, 1e9, 0.5).is_err());
        assert!(render_test_tone(&path, 440.0, f64::NAN, 0.5).is_err());
    }
}
