//! Autosave and crash recovery.
//!
//! While a project has unsaved changes, the app keeps a copy of it in the
//! app-data `autosave` folder, rewritten whenever it changes (at most every
//! [`AUTOSAVE_INTERVAL`]). A save, New, Open, or a clean exit removes the
//! copy, so one that is still there at launch means the app didn't close
//! properly, and its work can be recovered.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::discovery::APP_ID;
use crate::host::{Host, replace};

/// How often the app checks for changes to autosave.
pub const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(30);

/// Autosaves older than this are from a session long gone.
const STALE: Duration = Duration::from_secs(14 * 24 * 3600);

/// The app-data folder holding autosaves
/// (`%APPDATA%\io.github.nuncprotunc7.nuncprotune\autosave` on Windows).
pub fn autosave_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_ID)
        .join("autosave")
}

/// What an autosave belongs to, stored beside the project copy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Meta {
    /// When it was written, in milliseconds since the Unix epoch.
    saved_at_ms: u64,
    /// The project's file, if it had been saved.
    project_path: Option<PathBuf>,
    /// Where its audio was being written (the project's audio folder, or
    /// that run's unsaved-audio folder), so recovered clips still play.
    audio_folder: PathBuf,
    name: String,
}

/// This run's autosave: `<dir>/<stem>.nptune` plus `<stem>.json`.
#[derive(Debug)]
pub struct AutosaveSlot {
    dir: PathBuf,
    stem: String,
    /// The session revision last written, if the files exist.
    written: Option<u64>,
}

impl AutosaveSlot {
    /// A slot unique to this run.
    pub fn new(dir: PathBuf) -> Self {
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        Self::with_stem(dir, format!("{started}-{}", std::process::id()))
    }

    fn with_stem(dir: PathBuf, stem: String) -> Self {
        Self {
            dir,
            stem,
            written: None,
        }
    }

    fn project_file(&self) -> PathBuf {
        self.dir
            .join(format!("{}.{}", self.stem, daw_model::PROJECT_EXTENSION))
    }

    fn meta_file(&self) -> PathBuf {
        self.dir.join(format!("{}.json", self.stem))
    }

    /// Writes the project if it changed since the last autosave; removes the
    /// autosave once there is nothing unsaved. Returns whether it wrote.
    pub fn update<H: Host>(&mut self, host: &H) -> Result<bool, String> {
        let (project, revision, name) = {
            let session = host.session()?;
            if !session.is_dirty() {
                drop(session);
                self.discard();
                return Ok(false);
            }
            if self.written == Some(session.revision()) {
                return Ok(false);
            }
            let p = session.project();
            (p.clone(), session.revision(), p.name.clone())
        };
        // Written outside the lock so editing never waits on the disk.
        let meta = Meta {
            saved_at_ms: now_ms(),
            project_path: host.project_path(),
            audio_folder: host.audio().write_folder(),
            name,
        };
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        daw_model::save_project(&project, &self.project_file()).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
        // The project goes first: the meta file is what marks it complete.
        let tmp = self.meta_file().with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, self.meta_file()).map_err(|e| e.to_string())?;
        self.written = Some(revision);
        Ok(true)
    }

    /// Removes this run's autosave (after a save, New, Open, or on exit).
    pub fn discard(&mut self) {
        let _ = std::fs::remove_file(self.meta_file());
        let _ = std::fs::remove_file(self.project_file());
        self.written = None;
    }
}

/// Unsaved work left by a run that didn't close properly.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Recoverable {
    pub name: String,
    /// The song's file, if it had been saved before.
    pub project_path: Option<PathBuf>,
    /// When the work was autosaved, in milliseconds since the Unix epoch.
    pub saved_at_ms: u64,
    #[serde(skip)]
    meta_file: PathBuf,
    #[serde(skip)]
    project_file: PathBuf,
    #[serde(skip)]
    audio_folder: PathBuf,
}

/// The newest autosave not belonging to `own` (this run), if any. Autosaves
/// that are older than two weeks, damaged, or older than their song's file
/// (it was saved since) are deleted along the way.
pub fn find_recoverable(own: &AutosaveSlot) -> Option<Recoverable> {
    let mut found: Vec<Recoverable> = Vec::new();
    for entry in std::fs::read_dir(&own.dir).ok()?.flatten() {
        let path = entry.path();
        let Some(stem) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".json"))
        else {
            continue;
        };
        if stem == own.stem {
            continue;
        }
        let slot = AutosaveSlot::with_stem(own.dir.clone(), stem.to_owned());
        let meta: Option<Meta> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok());
        let keep = meta.filter(|m| {
            let age = Duration::from_millis(now_ms().saturating_sub(m.saved_at_ms));
            let saved_since = m
                .project_path
                .as_deref()
                .is_some_and(|p| modified_ms(p).is_some_and(|file_ms| file_ms >= m.saved_at_ms));
            age < STALE && !saved_since && slot.project_file().is_file()
        });
        match keep {
            Some(m) => found.push(Recoverable {
                name: m.name,
                project_path: m.project_path,
                saved_at_ms: m.saved_at_ms,
                meta_file: slot.meta_file(),
                project_file: slot.project_file(),
                audio_folder: m.audio_folder,
            }),
            None => {
                let _ = std::fs::remove_file(slot.meta_file());
                let _ = std::fs::remove_file(slot.project_file());
            }
        }
    }
    found.into_iter().max_by_key(|r| r.saved_at_ms)
}

/// Opens the recovered work in place of the current project, marked as
/// unsaved so the user saves it (to its original file, if it had one).
pub fn recover<H: Host>(host: &H, found: &Recoverable) -> Result<(), String> {
    let project = daw_model::load_project(&found.project_file).map_err(|e| e.to_string())?;
    let audio = host.audio();
    audio.clear_cache();
    audio.set_project_folder(Some(match &found.project_path {
        Some(p) => daw_audio::audio_folder_for(p),
        // An unsaved song's recordings are still in that run's folder.
        None => found.audio_folder.clone(),
    }));
    replace(host, project, found.project_path.clone())?;
    host.session()?.mark_unsaved();
    discard_recoverable(found);
    Ok(())
}

/// Deletes recovered (or declined) work.
pub fn discard_recoverable(found: &Recoverable) {
    let _ = std::fs::remove_file(&found.meta_file);
    let _ = std::fs::remove_file(&found.project_file);
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn modified_ms(path: &Path) -> Option<u64> {
    let t = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(t.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::tests::TestHost;
    use daw_model::Command;

    fn edit(host: &TestHost, bpm: f64) {
        host.session()
            .expect("session")
            .execute(Command::SetTempo { bpm })
            .expect("edit");
    }

    #[test]
    fn unsaved_work_survives_a_crash_and_is_recovered() {
        let dir = tempfile::tempdir().expect("tmp");
        let host = TestHost::default();
        let mut slot = AutosaveSlot::with_stem(dir.path().to_owned(), "run1".into());
        // Nothing unsaved: nothing written.
        assert!(!slot.update(&host).expect("update"));
        edit(&host, 133.0);
        assert!(slot.update(&host).expect("update"));
        // Unchanged since: not rewritten.
        assert!(!slot.update(&host).expect("update"));
        // The app "crashes" (the slot is never discarded) and starts again.
        let next_run = AutosaveSlot::with_stem(dir.path().to_owned(), "run2".into());
        let fresh = TestHost::default();
        let found = find_recoverable(&next_run).expect("work to recover");
        assert_eq!(found.project_path, None);
        recover(&fresh, &found).expect("recover");
        let session = fresh.session().expect("session");
        assert_eq!(session.project().tempo_bpm, 133.0);
        assert!(session.is_dirty(), "recovered work still needs saving");
        drop(session);
        // Offered once.
        assert_eq!(find_recoverable(&next_run), None);
    }

    #[test]
    fn saving_or_closing_removes_the_autosave() {
        let dir = tempfile::tempdir().expect("tmp");
        let host = TestHost::default();
        let mut slot = AutosaveSlot::with_stem(dir.path().to_owned(), "run1".into());
        edit(&host, 99.0);
        slot.update(&host).expect("update");
        host.session().expect("session").mark_saved();
        slot.update(&host).expect("update");
        let other = AutosaveSlot::with_stem(dir.path().to_owned(), "run2".into());
        assert_eq!(find_recoverable(&other), None);
        edit(&host, 98.0);
        slot.update(&host).expect("update");
        slot.discard();
        assert_eq!(find_recoverable(&other), None);
    }

    #[test]
    fn an_autosave_older_than_its_saved_song_is_dropped() {
        let dir = tempfile::tempdir().expect("tmp");
        let song = dir.path().join("Song.nptune");
        let host = TestHost::default();
        host.set_project_path(Some(song.clone()));
        let mut slot = AutosaveSlot::with_stem(dir.path().join("autosave"), "run1".into());
        edit(&host, 101.0);
        slot.update(&host).expect("update");
        // The song was saved after the autosave (e.g. by another copy of
        // the app): the autosave is out of date.
        std::thread::sleep(Duration::from_millis(20));
        daw_model::save_project(host.session().expect("s").project(), &song).expect("save");
        let other = AutosaveSlot::with_stem(dir.path().join("autosave"), "run2".into());
        assert_eq!(find_recoverable(&other), None);
        assert!(!slot.project_file().exists(), "stale autosave deleted");
    }
}
