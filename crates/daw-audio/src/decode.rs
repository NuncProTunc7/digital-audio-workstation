//! Decoding any supported file into float samples, using symphonia.

use std::fs::File;
use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::{AudioData, AudioError};

/// Longest file accepted, in seconds; matches the project limit.
const MAX_SECONDS: f64 = 3600.0;

/// Reads an audio file into memory. Files with more than two channels are
/// folded down to stereo.
pub fn decode_file(path: &Path) -> Result<AudioData, AudioError> {
    let unsupported = |message: String| AudioError::Unsupported {
        path: path.to_owned(),
        message,
    };
    let file = File::open(path).map_err(|e| AudioError::Io {
        path: path.to_owned(),
        message: e.to_string(),
    })?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| unsupported(e.to_string()))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| unsupported("no audio track".into()))?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| unsupported("no audio codec".into()))?
        .clone();
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| unsupported(e.to_string()))?;

    let mut sample_rate_hz = params.sample_rate.unwrap_or(0);
    let mut out: Vec<Vec<f32>> = Vec::new();
    let mut interleaved: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            // Some files end without a clean end-of-stream marker.
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(unsupported(e.to_string())),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(b) => b,
            // A damaged packet costs a few milliseconds, not the whole file.
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(unsupported(e.to_string())),
        };
        let spec = buf.spec();
        sample_rate_hz = spec.rate();
        let channels = spec.channels().count().max(1);
        if out.is_empty() {
            out = vec![Vec::new(); channels.min(2)];
        }
        interleaved.resize(buf.samples_interleaved(), 0.0);
        buf.copy_to_slice_interleaved(&mut interleaved);
        for frame in interleaved.chunks(channels) {
            fold_to(&mut out, frame);
        }
        if out[0].len() as f64 > MAX_SECONDS * f64::from(sample_rate_hz.max(1)) {
            return Err(AudioError::TooLong(path.to_owned()));
        }
    }
    if out.is_empty() || out[0].is_empty() || sample_rate_hz == 0 {
        return Err(AudioError::Empty(path.to_owned()));
    }
    Ok(AudioData {
        sample_rate_hz,
        channels: out,
    })
}

/// Appends one frame, folding extra channels into left/right.
fn fold_to(out: &mut [Vec<f32>], frame: &[f32]) {
    match (out.len(), frame) {
        (1, _) => out[0].push(frame.iter().sum::<f32>() / frame.len().max(1) as f32),
        (_, [l, r]) => {
            out[0].push(*l);
            out[1].push(*r);
        }
        (_, many) => {
            // Surround: even channels go left, odd go right.
            let (mut l, mut r) = (0.0, 0.0);
            for (i, s) in many.iter().enumerate() {
                if i % 2 == 0 {
                    l += s;
                } else {
                    r += s;
                }
            }
            let half = (many.len() as f32 / 2.0).max(1.0);
            out[0].push(l / half);
            out[1].push(r / half);
        }
    }
}
