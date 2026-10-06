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

/// Reads an SFZ file and every sample it uses, within `budget_bytes`.
pub fn load_pack(path: &Path, budget_bytes: u64) -> Result<SamplePack, SamplerError> {
    let regions = parse_sfz_file(path)?;
    let (regions, layers_kept, layers_total) = thin_layers(regions, budget_bytes);
    let mut decoded: HashMap<PathBuf, (Arc<[i16]>, u32, usize)> = HashMap::new();
    let mut zones = Vec::with_capacity(regions.len());
    let mut bytes = 0u64;
    for region in regions {
        let entry = match decoded.get(&region.sample) {
            Some(e) => e.clone(),
            None => {
                let audio = daw_audio::decode_file(&region.sample)
                    .map_err(|e| SamplerError::Decode(e.to_string()))?;
                let samples: Arc<[i16]> = to_i16(&audio).into();
                bytes += samples.len() as u64 * 2;
                let e = (samples, audio.sample_rate_hz, audio.channels.len().max(1));
                decoded.insert(region.sample.clone(), e.clone());
                e
            }
        };
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
