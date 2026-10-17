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

/// Writes 16-bit stereo WAV. With `loop_frames` (begin, end), adds a `smpl`
/// chunk looping that span; the end is written exclusive, as Godot reads it.
pub fn write_wav(
    path: &Path,
    stereo: &[f32],
    sample_rate_hz: u32,
    loop_frames: Option<(usize, usize)>,
) -> Result<(), ExportError> {
    write_wav_with(path, stereo, sample_rate_hz, loop_frames, None)
}

/// [`write_wav`], plus an optional comment (a `LIST`/`INFO`/`ICMT` chunk
/// after the audio, which players and Godot ignore).
pub(crate) fn write_wav_with(
    path: &Path,
    stereo: &[f32],
    sample_rate_hz: u32,
    loop_frames: Option<(usize, usize)>,
    comment: Option<&str>,
) -> Result<(), ExportError> {
    let frames = stereo.len() / 2;
    let data_bytes = (frames * 4) as u32;
    let mut out = Vec::with_capacity(44 + data_bytes as usize + 128);
    out.extend_from_slice(b"RIFF");
    // The size goes in once every chunk is written.
    out.extend_from_slice(&0u32.to_le_bytes());
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
    if let Some((begin, end)) = loop_frames {
        out.extend_from_slice(b"smpl");
        out.extend_from_slice(&(36u32 + 24).to_le_bytes());
        let period_ns = (1e9 / f64::from(sample_rate_hz.max(1))).round() as u32;
        // manufacturer, product, sample period, MIDI unity note, pitch
        // fraction, SMPTE format, SMPTE offset, loop count, sampler data.
        for v in [0u32, 0, period_ns, 60, 0, 0, 0, 1, 0] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        // One forward loop: cue id, type, begin, end, fraction, play count.
        for v in [0u32, 0, begin as u32, end as u32, 0, 0] {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    if let Some(text) = comment {
        // Zero-terminated, padded to an even length.
        let mut text = text.as_bytes().to_vec();
        text.push(0);
        if text.len() % 2 == 1 {
            text.push(0);
        }
        out.extend_from_slice(b"LIST");
        out.extend_from_slice(&(4 + 8 + text.len() as u32).to_le_bytes());
        out.extend_from_slice(b"INFOICMT");
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(&text);
    }
    let riff_size = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&riff_size.to_le_bytes());
    let tmp = path.with_extension("wav.tmp");
    std::fs::write(&tmp, out).map_err(|e| io_err(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| io_err(path, e))
}

/// The comment [`write_wav_with`] wrote into a WAV file, if any.
pub(crate) fn wav_comment(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().ok()?) as usize;
        let body = bytes.get(at + 8..(at + 8).checked_add(size)?)?;
        if id == b"LIST" && body.get(0..8) == Some(b"INFOICMT") {
            let len = u32::from_le_bytes(body.get(8..12)?.try_into().ok()?) as usize;
            let text = body.get(12..12 + len)?;
            let text = text.split(|&b| b == 0).next().unwrap_or_default();
            return String::from_utf8(text.to_vec()).ok();
        }
        at += 8 + size + size % 2;
    }
    None
}
