//! Writing OGG Vorbis and looping WAV files.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::num::{NonZeroU8, NonZeroU32};
use std::path::Path;

use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoderBuilder};

use crate::{ExportError, io_err};

/// Vorbis quality (-0.2 to 1.0); 0.6 is about 190 kbit/s, transparent for music.
const OGG_QUALITY: f32 = 0.6;

/// Writes interleaved stereo as OGG Vorbis.
pub fn write_ogg(path: &Path, stereo: &[f32], sample_rate_hz: u32) -> Result<(), ExportError> {
    let enc = |e: vorbis_rs::VorbisError| ExportError::Encode(e.to_string());
    let tmp = path.with_extension("ogg.tmp");
    let file = BufWriter::new(File::create(&tmp).map_err(|e| io_err(&tmp, e))?);
    let rate = NonZeroU32::new(sample_rate_hz.max(1)).unwrap_or(NonZeroU32::MIN);
    let channels = NonZeroU8::new(2).unwrap_or(NonZeroU8::MIN);
    // A fixed stream serial makes exports reproducible.
    let mut builder = VorbisEncoderBuilder::new_with_serial(rate, channels, file, 0x4E50_5455);
    builder.bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr {
        target_quality: OGG_QUALITY,
    });
    let mut encoder = builder.build().map_err(enc)?;
    let (mut left, mut right) = (Vec::with_capacity(4096), Vec::with_capacity(4096));
    for block in stereo.chunks(8192) {
        left.clear();
        right.clear();
        for &[l, r] in block.as_chunks::<2>().0 {
            left.push(l);
            right.push(r);
        }
        encoder.encode_audio_block([&left, &right]).map_err(enc)?;
    }
    let mut file = encoder.finish().map_err(enc)?;
    file.flush().map_err(|e| io_err(&tmp, e))?;
    drop(file);
    std::fs::rename(&tmp, path).map_err(|e| io_err(path, e))
}

/// Writes 16-bit stereo WAV. With `loop_end_frame`, adds a `smpl` chunk
/// looping the whole file; the end is written exclusive, as Godot reads it.
pub fn write_wav(
    path: &Path,
    stereo: &[f32],
    sample_rate_hz: u32,
    loop_end_frame: Option<usize>,
) -> Result<(), ExportError> {
    let frames = stereo.len() / 2;
    let data_bytes = (frames * 4) as u32;
    let smpl_bytes: u32 = if loop_end_frame.is_some() { 36 + 24 } else { 0 };
    let mut out = Vec::with_capacity(44 + data_bytes as usize + 68);
    let riff_size =
        4 + (8 + 16) + (8 + data_bytes) + if smpl_bytes > 0 { 8 + smpl_bytes } else { 0 };
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&2u16.to_le_bytes()); // stereo
    out.extend_from_slice(&sample_rate_hz.to_le_bytes());
    out.extend_from_slice(&(sample_rate_hz * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for s in &stereo[..frames * 2] {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    if let Some(end) = loop_end_frame {
        out.extend_from_slice(b"smpl");
        out.extend_from_slice(&smpl_bytes.to_le_bytes());
        let period_ns = (1e9 / f64::from(sample_rate_hz.max(1))).round() as u32;
        // manufacturer, product, sample period, MIDI unity note, pitch
        // fraction, SMPTE format, SMPTE offset, loop count, sampler data.
        for v in [0u32, 0, period_ns, 60, 0, 0, 0, 1, 0] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        // One forward loop over the whole file.
        for v in [0u32, 0, 0, end as u32, 0, 0] {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    let tmp = path.with_extension("wav.tmp");
    std::fs::write(&tmp, out).map_err(|e| io_err(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| io_err(path, e))
}
