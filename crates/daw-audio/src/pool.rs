//! Where a project's audio lives, and the cache of decoded audio.
//!
//! A saved project `Song.nptune` keeps its audio in `Song Audio/` next to
//! it. Before the first save, imports and recordings go to a scratch folder
//! and are copied over when the project is saved.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{AudioBuffer, AudioData, AudioError, Peaks, decode_file, peaks, read_wav, resample};

/// The audio folder that belongs to a project file: `Song.nptune` → `Song Audio`.
pub fn audio_folder_for(project_path: &Path) -> PathBuf {
    let stem = project_path
        .file_stem()
        .map_or_else(|| "Untitled".into(), |s| s.to_string_lossy().into_owned());
    project_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{stem} Audio"))
}

/// A file brought into the project by [`AudioPool::import`].
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedAudio {
    /// File name inside the audio folder.
    pub file: String,
    pub seconds: f64,
    /// Display name: the original file name without its extension.
    pub name: String,
}

/// Finds, loads, and caches a project's audio.
pub struct AudioPool {
    scratch: PathBuf,
    project_folder: RwLock<Option<PathBuf>>,
    /// Audio added in memory (tests, freshly finished recordings).
    memory: Mutex<HashMap<String, Arc<AudioData>>>,
    /// Keyed by file, sample rate, and stretch ratio (in 1/10000ths).
    buffers: Mutex<HashMap<(String, u32, i64), Arc<AudioBuffer>>>,
    peaks: Mutex<HashMap<String, Arc<Peaks>>>,
}

impl AudioPool {
    /// A pool whose unsaved audio goes in `scratch`.
    pub fn new(scratch: PathBuf) -> Self {
        Self {
            scratch,
            project_folder: RwLock::new(None),
            memory: Mutex::new(HashMap::new()),
            buffers: Mutex::new(HashMap::new()),
            peaks: Mutex::new(HashMap::new()),
        }
    }

    /// A pool for headless use (tests, renders of note-only songs).
    pub fn in_temp_dir() -> Arc<Self> {
        Arc::new(Self::new(
            std::env::temp_dir().join("nunc-pro-tune-scratch"),
        ))
    }

    /// Points the pool at a saved project's audio folder (None when unsaved).
    pub fn set_project_folder(&self, folder: Option<PathBuf>) {
        if let Ok(mut f) = self.project_folder.write() {
            *f = folder;
        }
    }

    pub fn project_folder(&self) -> Option<PathBuf> {
        self.project_folder.read().ok().and_then(|f| f.clone())
    }

    /// Where new imports and recordings are written.
    pub fn write_folder(&self) -> PathBuf {
        self.project_folder()
            .unwrap_or_else(|| self.scratch.clone())
    }

    /// Path of an audio file, looking in the project folder, then scratch.
    pub fn locate(&self, file: &str) -> Option<PathBuf> {
        let folders = self
            .project_folder()
            .into_iter()
            .chain([self.scratch.clone()]);
        folders.map(|d| d.join(file)).find(|p| p.is_file())
    }

    /// Whether the file can be played.
    pub fn has(&self, file: &str) -> bool {
        self.memory.lock().is_ok_and(|m| m.contains_key(file)) || self.locate(file).is_some()
    }

    /// Adds audio held in memory under a file name (used by tests).
    pub fn insert(&self, file: &str, data: AudioData) {
        if let Ok(mut m) = self.memory.lock() {
            m.insert(file.to_owned(), Arc::new(data));
        }
    }

    fn load(&self, file: &str) -> Result<Arc<AudioData>, AudioError> {
        if let Some(d) = self.memory.lock().ok().and_then(|m| m.get(file).cloned()) {
            return Ok(d);
        }
        let path = self
            .locate(file)
            .ok_or_else(|| AudioError::Missing(file.to_owned()))?;
        let is_wav = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
        let data = if is_wav {
            read_wav(&path)?
        } else {
            decode_file(&path)?
        };
        Ok(Arc::new(data))
    }

    /// The file's audio at `sample_rate_hz`, ready for the engine. Cached.
    pub fn buffer(&self, file: &str, sample_rate_hz: u32) -> Result<Arc<AudioBuffer>, AudioError> {
        self.stretched(file, sample_rate_hz, 1.0)
    }

    /// The file's audio at `sample_rate_hz`, stretched to `ratio` times its
    /// length with its pitch unchanged (for clips that follow the tempo).
    /// Cached, so a tempo change costs one stretch per clip file.
    pub fn stretched(
        &self,
        file: &str,
        sample_rate_hz: u32,
        ratio: f64,
    ) -> Result<Arc<AudioBuffer>, AudioError> {
        let ratio_key = (ratio * 10_000.0).round() as i64;
        let key = (file.to_owned(), sample_rate_hz, ratio_key);
        if let Some(b) = self.buffers.lock().ok().and_then(|c| c.get(&key).cloned()) {
            return Ok(b);
        }
        let data = resample((*self.load(file)?).clone(), sample_rate_hz)?;
        let data = if ratio_key == 10_000 {
            data
        } else {
            crate::time_stretch(&data, ratio_key as f64 / 10_000.0)
        };
        let buffer = Arc::new(AudioBuffer::from_data(data));
        if let Ok(mut c) = self.buffers.lock() {
            c.insert(key, Arc::clone(&buffer));
        }
        Ok(buffer)
    }

    /// Waveform overview of the file. Cached.
    pub fn peaks(&self, file: &str) -> Result<Arc<Peaks>, AudioError> {
        if let Some(p) = self.peaks.lock().ok().and_then(|c| c.get(file).cloned()) {
            return Ok(p);
        }
        let p = Arc::new(peaks(&*self.load(file)?));
        if let Ok(mut c) = self.peaks.lock() {
            c.insert(file.to_owned(), Arc::clone(&p));
        }
        Ok(p)
    }

    /// Decodes any supported file (a phone's m4a, an mp3, a wav...) and
    /// stores it in the audio folder as WAV. Importing the same audio twice
    /// reuses the first copy.
    pub fn import(&self, source: &Path) -> Result<ImportedAudio, AudioError> {
        let data = decode_file(source)?;
        let name = source
            .file_stem()
            .map_or_else(|| "Audio".into(), |s| s.to_string_lossy().trim().to_owned());
        let file = format!("{}-{:08x}.wav", safe_stem(&name), content_hash(&data));
        let folder = self.write_folder();
        let path = folder.join(&file);
        if !path.is_file() {
            std::fs::create_dir_all(&folder).map_err(|e| AudioError::Io {
                path: folder.clone(),
                message: e.to_string(),
            })?;
            crate::write_wav(&path, &data)?;
        }
        let seconds = data.seconds();
        if let Ok(mut c) = self.peaks.lock() {
            c.insert(file.clone(), Arc::new(peaks(&data)));
        }
        Ok(ImportedAudio {
            file,
            seconds,
            name,
        })
    }

    /// A fresh path for a new recording in the write folder.
    pub fn new_recording_path(&self) -> Result<(String, PathBuf), AudioError> {
        let folder = self.write_folder();
        std::fs::create_dir_all(&folder).map_err(|e| AudioError::Io {
            path: folder.clone(),
            message: e.to_string(),
        })?;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        for salt in 0u32.. {
            let hash = fnv1a(&[nanos.to_le_bytes().as_slice(), &salt.to_le_bytes()]);
            let file = format!("Recording-{:08x}.wav", hash as u32);
            let path = folder.join(&file);
            if !path.exists() {
                return Ok((file, path));
            }
        }
        unreachable!("u32 salts exhausted")
    }

    /// Makes sure every file in `files` is in `dest` (the project's audio
    /// folder), copying from wherever it is now. Returns files that could
    /// not be found anywhere.
    pub fn gather<'a>(
        &self,
        files: impl IntoIterator<Item = &'a str>,
        dest: &Path,
    ) -> Result<Vec<String>, AudioError> {
        let mut missing = Vec::new();
        for file in files {
            let target = dest.join(file);
            if target.is_file() {
                continue;
            }
            std::fs::create_dir_all(dest).map_err(|e| AudioError::Io {
                path: dest.to_owned(),
                message: e.to_string(),
            })?;
            if let Some(src) = self.locate(file) {
                std::fs::copy(&src, &target).map_err(|e| AudioError::Io {
                    path: target.clone(),
                    message: e.to_string(),
                })?;
            } else if let Some(d) = self.memory.lock().ok().and_then(|m| m.get(file).cloned()) {
                crate::write_wav(&target, &d)?;
            } else {
                missing.push(file.to_owned());
            }
        }
        Ok(missing)
    }

    /// Drops cached audio (after opening another project).
    pub fn clear_cache(&self) {
        if let Ok(mut c) = self.buffers.lock() {
            c.clear();
        }
        if let Ok(mut c) = self.peaks.lock() {
            c.clear();
        }
        if let Ok(mut m) = self.memory.lock() {
            m.clear();
        }
    }
}

/// File-system-safe version of a display name.
fn safe_stem(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '_' | '-' | '(' | ')') {
                c
            } else {
                '_'
            }
        })
        .take(60)
        .collect();
    let s = s.trim().trim_start_matches('.').to_owned();
    if s.is_empty() { "Audio".into() } else { s }
}

fn fnv1a(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for &b in *part {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// Short fingerprint of the audio, so re-importing a file is a no-op.
fn content_hash(data: &AudioData) -> u32 {
    let mut h: u64 = fnv1a(&[&data.sample_rate_hz.to_le_bytes()]);
    for ch in &data.channels {
        for s in ch {
            for b in s.to_bits().to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    (h ^ (h >> 32)) as u32
}
