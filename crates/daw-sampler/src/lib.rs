//! Sample packs for the sampler instrument.
//!
//! [`load_pack`] reads an SFZ file and its recordings into memory as 16-bit
//! audio. Big packs (a concert grand can be a gigabyte) are thinned to fit a
//! memory budget by keeping evenly spaced velocity layers, so every key
//! still plays and soft-to-loud still changes the tone. [`load_cached`]
//! loads in the background and shares one copy between tracks.

pub mod sfz;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use thiserror::Error;

pub use sfz::{Region, parse_note, parse_sfz, parse_sfz_file};

/// Memory a pack may use once loaded (16-bit samples).
pub const DEFAULT_BUDGET_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Error)]
pub enum SamplerError {
    #[error("could not read {path}: {message}")]
    Io { path: PathBuf, message: String },
    #[error("this SFZ file can't be used: {0}")]
    Parse(String),
    #[error("a sample couldn't be decoded: {0}")]
    Decode(String),
}

/// A region with its audio.
#[derive(Debug)]
pub struct Zone {
    pub region: Region,
    pub sample_rate_hz: u32,
    pub channels: usize,
    /// Interleaved 16-bit samples, shared between zones using one file.
    pub data: Arc<[i16]>,
}

impl Zone {
    pub fn frames(&self) -> usize {
        self.data.len() / self.channels.max(1)
    }

    /// Whether this zone plays `note` at `velocity`.
    pub fn matches(&self, note: u8, velocity: u8) -> bool {
        let r = &self.region;
        (r.lokey..=r.hikey).contains(&note) && (r.lovel..=r.hivel).contains(&velocity)
    }
}

/// A loaded pack, ready to play.
#[derive(Debug)]
pub struct SamplePack {
    pub name: String,
    pub zones: Vec<Zone>,
    /// Memory used by the audio.
    pub bytes: u64,
    /// Velocity layers kept out of those in the pack.
    pub layers_kept: usize,
    pub layers_total: usize,
}

impl SamplePack {
    /// Lowest and highest notes that play a recording, as MIDI notes.
    pub fn note_range(&self) -> (u8, u8) {
        let low = self.zones.iter().map(|z| z.region.lokey).min().unwrap_or(0);
        let high = self
            .zones
            .iter()
            .map(|z| z.region.hikey)
            .max()
            .unwrap_or(127);
        (low, high)
    }
}

/// Velocity layers as (lovel, hivel), ordered soft to loud.
fn layers(regions: &[Region]) -> Vec<(u8, u8)> {
    let mut l: Vec<(u8, u8)> = regions.iter().map(|r| (r.lovel, r.hivel)).collect();
    l.sort_unstable();
    l.dedup();
    l
}

/// Drops velocity layers until the pack's estimated size fits `budget`,
/// keeping the loudest and evenly spaced softer ones, and widens what's
/// left so every velocity still plays something.
fn thin_layers(mut regions: Vec<Region>, budget: u64) -> (Vec<Region>, usize, usize) {
    let all = layers(&regions);
    let size = |rs: &[Region]| -> u64 {
        let mut files: Vec<&Path> = rs.iter().map(|r| r.sample.as_path()).collect();
        files.sort_unstable();
        files.dedup();
        // 16-bit decoded is roughly the size of a FLAC of 24-bit audio.
        files
            .iter()
            .filter_map(|f| std::fs::metadata(f).ok())
            .map(|m| m.len())
            .sum()
    };
    let estimate = size(&regions);
    if all.len() <= 1 || estimate <= budget {
        return (regions, all.len(), all.len());
    }
    let keep = ((all.len() as u64 * budget / estimate.max(1)) as usize).clamp(1, all.len());
    // Evenly spaced, always including the loudest layer.
    let step = all.len() as f64 / keep as f64;
    let mut kept: Vec<(u8, u8)> = (0..keep)
        .map(|i| all[(all.len() - 1) - ((i as f64 * step).floor() as usize).min(all.len() - 1)])
        .collect();
    kept.sort_unstable();
    kept.dedup();
    regions.retain(|r| kept.contains(&(r.lovel, r.hivel)));
    // Kept layer i now covers from just above layer i-1 up to its own top.
    for r in &mut regions {
        let i = kept
            .iter()
            .position(|&l| l == (r.lovel, r.hivel))
            .unwrap_or(0);
        r.lovel = if i == 0 {
            1
        } else {
            kept[i - 1].1.saturating_add(1)
        };
        if i + 1 == kept.len() {
            r.hivel = 127;
        }
    }
    (regions, kept.len(), all.len())
}

fn to_i16(data: &daw_audio::AudioData) -> Vec<i16> {
    let frames = data.frames();
    let ch = data.channels.len().max(1);
    let mut out = Vec::with_capacity(frames * ch);
    for i in 0..frames {
        for c in &data.channels {
            out.push((c[i].clamp(-1.0, 1.0) * 32767.0).round() as i16);
        }
    }
    out
}

/// Finds a steady stretch to loop in a held recording (flute, organ,
/// strings) so a long note keeps sounding past the end of the file, and
/// crossfades its end into its start so the jump is smooth. Returns the loop
/// in frames (inclusive), or None for sounds that fade away by themselves
/// (piano, guitar, drums), which are left as recorded.
fn sustain_loop(data: &mut [i16], channels: usize, sample_rate_hz: u32) -> Option<(u64, u64)> {
    let ch = channels.max(1);
    let frames = data.len() / ch;
    let window = (sample_rate_hz as usize / 20).max(1);
    // Short recordings are staccato or percussive.
    if frames < sample_rate_hz as usize * 3 / 2 {
        return None;
    }
    let level_db: Vec<f64> = data
        .chunks(window * ch)
        .map(|w| {
            let power = w.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>() / w.len() as f64;
            10.0 * power.max(1e-9).log10()
        })
        .collect();
    let n = level_db.len();
    // Past the attack, before any fade at the end of the recording.
    let (a, b) = (n * 3 / 10, n * 85 / 100);
    let steady = level_db.get(a..b).filter(|s| s.len() >= 6)?;
    let mean = |s: &[f64]| s.iter().sum::<f64>() / s.len() as f64;
    let third = steady.len() / 3;
    let loudest = level_db.iter().copied().fold(f64::MIN, f64::max);
    let lowest = steady.iter().copied().fold(f64::MAX, f64::min);
    let highest = steady.iter().copied().fold(f64::MIN, f64::max);
    let decaying = mean(&steady[..third]) - mean(&steady[steady.len() - third..]) > 3.0;
    if decaying || highest - lowest > 8.0 || mean(steady) < loudest - 20.0 {
        return None;
    }
    let start = a * window;
    let end = (b * window).min(frames) - 1;
    let fade = ((end - start) / 4).min(sample_rate_hz as usize / 10);
    if fade == 0 || start < fade {
        return None;
    }
    // Blend the frames before the loop end towards those before the loop
    // start, so the frame after `end` (which is `start`) follows on.
    for i in 0..fade {
        let t = (i + 1) as f64 / fade as f64 * std::f64::consts::FRAC_PI_2;
        let (out_gain, in_gain) = (t.cos(), t.sin());
        let (p, q) = (end + 1 - fade + i, start - fade + i);
        for c in 0..ch {
            let mixed =
                f64::from(data[p * ch + c]) * out_gain + f64::from(data[q * ch + c]) * in_gain;
            data[p * ch + c] = mixed.round().clamp(-32768.0, 32767.0) as i16;
        }
    }
    Some((start as u64, end as u64))
}

/// Reads an SFZ file and every sample it uses, within `budget_bytes`.
pub fn load_pack(path: &Path, budget_bytes: u64) -> Result<SamplePack, SamplerError> {
    let regions = parse_sfz_file(path)?;
    let (regions, layers_kept, layers_total) = thin_layers(regions, budget_bytes);
    type Decoded = (Arc<[i16]>, u32, usize, Option<(u64, u64)>);
    let mut decoded: HashMap<PathBuf, Decoded> = HashMap::new();
    let mut zones = Vec::with_capacity(regions.len());
    let mut bytes = 0u64;
    for mut region in regions {
        let wants_loop = region.loop_frames.is_none() && !region.one_shot;
        let entry = match decoded.get(&region.sample) {
            Some(e) => e.clone(),
            None => {
                let audio = daw_audio::decode_file(&region.sample)
                    .map_err(|e| SamplerError::Decode(e.to_string()))?;
                let channels = audio.channels.len().max(1);
                let mut samples = to_i16(&audio);
                let auto_loop = if wants_loop {
                    sustain_loop(&mut samples, channels, audio.sample_rate_hz)
                } else {
                    None
                };
                let samples: Arc<[i16]> = samples.into();
                bytes += samples.len() as u64 * 2;
                let e = (samples, audio.sample_rate_hz, channels, auto_loop);
                decoded.insert(region.sample.clone(), e.clone());
                e
            }
        };
        if wants_loop {
            region.loop_frames = entry.3;
        }
        zones.push(Zone {
            region,
            sample_rate_hz: entry.1,
            channels: entry.2,
            data: entry.0,
        });
    }
    Ok(SamplePack {
        name: path.file_stem().map_or_else(
            || "Sample pack".into(),
            |s| s.to_string_lossy().into_owned(),
        ),
        zones,
        bytes,
        layers_kept,
        layers_total,
    })
}

/// A pack once loading finishes: the pack, or why it failed.
pub type PackResult = Result<Arc<SamplePack>, String>;

/// A pack that may still be loading: the audio thread checks it without
/// waiting (`OnceLock::get` never blocks).
pub type PackSlot = Arc<OnceLock<PackResult>>;

/// Packs in use, by SFZ path. Weak, so a pack is freed when no track uses it.
type Cache = Mutex<HashMap<PathBuf, Weak<OnceLock<PackResult>>>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The pack at `path`, shared with anything else using it; loading starts
/// in the background the first time. Call off the audio thread.
pub fn load_cached(path: &Path) -> PackSlot {
    let Ok(mut c) = cache().lock() else {
        let slot: PackSlot = Arc::new(OnceLock::new());
        let _ = slot.set(Err("sample cache unavailable".into()));
        return slot;
    };
    if let Some(slot) = c.get(path).and_then(Weak::upgrade) {
        return slot;
    }
    let slot: PackSlot = Arc::new(OnceLock::new());
    c.insert(path.to_owned(), Arc::downgrade(&slot));
    let (fill, path) = (Arc::clone(&slot), path.to_owned());
    let spawned = std::thread::Builder::new()
        .name("npt-sample-loader".into())
        .spawn(move || {
            let _ = fill.set(
                load_pack(&path, DEFAULT_BUDGET_BYTES)
                    .map(Arc::new)
                    .map_err(|e| e.to_string()),
            );
        });
    if spawned.is_err() {
        let _ = slot.set(Err("couldn't start loading".into()));
    }
    slot
}

/// How a pack is doing, for the UI and Claude.
#[derive(Debug, Clone, PartialEq)]
pub enum PackStatus {
    /// Nothing has asked for this pack.
    NotLoaded,
    Loading,
    Ready {
        name: String,
        zones: usize,
        megabytes: f64,
        layers_kept: usize,
        layers_total: usize,
        /// Notes that play something (MIDI), lowest and highest.
        lowest_note: u8,
        highest_note: u8,
    },
    Failed(String),
}

pub fn pack_status(path: &Path) -> PackStatus {
    let slot = cache()
        .lock()
        .ok()
        .and_then(|c| c.get(path).and_then(Weak::upgrade));
    match slot.as_deref().map(OnceLock::get) {
        None => PackStatus::NotLoaded,
        Some(None) => PackStatus::Loading,
        Some(Some(Ok(p))) => PackStatus::Ready {
            name: p.name.clone(),
            zones: p.zones.len(),
            megabytes: p.bytes as f64 / 1_048_576.0,
            layers_kept: p.layers_kept,
            layers_total: p.layers_total,
            lowest_note: p.note_range().0,
            highest_note: p.note_range().1,
        },
        Some(Some(Err(e))) => PackStatus::Failed(e.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(lovel: u8, hivel: u8, file: &str) -> Region {
        Region {
            sample: PathBuf::from(file),
            lokey: 60,
            hikey: 60,
            keycenter: 60,
            lovel,
            hivel,
            volume_db: 0.0,
            tune_cents: 0.0,
            offset_frames: 0,
            attack_s: None,
            release_s: None,
            veltrack_percent: 100.0,
            one_shot: false,
            loop_frames: None,
        }
    }

    /// Mono 16-bit tone at 440 Hz, its level shaped by `gain(seconds)`.
    fn tone(seconds: f64, gain: impl Fn(f64) -> f64) -> Vec<i16> {
        (0..(seconds * 48_000.0) as usize)
            .map(|i| {
                let t = i as f64 / 48_000.0;
                (gain(t) * 20_000.0 * (std::f64::consts::TAU * 440.0 * t).sin()) as i16
            })
            .collect()
    }

    #[test]
    fn a_steady_held_note_loops_smoothly() {
        // Swells in, holds, fades at the end: like a flute recording.
        let mut data = tone(4.0, |t| (t / 0.3).min(1.0) * ((4.0 - t) / 0.4).min(1.0));
        let original = data.clone();
        let (start, end) = sustain_loop(&mut data, 1, 48_000).expect("loops");
        let (s, e) = (start as usize, end as usize);
        assert!(s > 48_000 / 2 && e < 4 * 48_000 * 9 / 10, "{s}..{e}");
        // After `end` comes `start`: the last loop frame now leads into it
        // as the frame before `start` does.
        assert_eq!(data[e], original[s - 1]);
        let jump = (i32::from(data[s]) - i32::from(data[e])).abs();
        let step = (i32::from(original[s]) - i32::from(original[s - 1])).abs();
        assert!(jump <= step + 2, "{jump} vs {step}");
        // Outside the crossfade nothing changes.
        assert_eq!(data[..e - 4_800], original[..e - 4_800]);
    }

    #[test]
    fn fading_and_short_sounds_are_not_looped() {
        let mut piano = tone(4.0, |t| (-t * 1.5).exp());
        assert_eq!(sustain_loop(&mut piano, 1, 48_000), None);
        let mut staccato = tone(0.5, |_| 1.0);
        assert_eq!(sustain_loop(&mut staccato, 1, 48_000), None);
    }

    #[test]
    fn thinning_keeps_the_loudest_layer_and_covers_every_velocity() {
        let dir = tempfile::tempdir().expect("tmp");
        let mut rs = Vec::new();
        for i in 0..8u8 {
            let f = dir.path().join(format!("v{i}.wav"));
            std::fs::write(&f, vec![0u8; 1000]).expect("write");
            rs.push(region(i * 16 + 1, i * 16 + 16, f.to_str().expect("utf8")));
        }
        // Room for about half.
        let (kept, k, total) = thin_layers(rs, 4_000);
        assert_eq!(total, 8);
        assert_eq!(k, 4);
        assert_eq!(kept.len(), 4);
        assert_eq!(kept[0].lovel, 1);
        assert_eq!(kept.last().expect("loudest").hivel, 127);
        for w in kept.windows(2) {
            assert_eq!(w[1].lovel, w[0].hivel + 1, "no velocity gaps");
        }
    }
}
