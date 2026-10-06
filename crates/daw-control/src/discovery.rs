use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The app's identifier; also its data folder name.
pub const APP_ID: &str = "io.github.nuncprotunc7.nuncprotune";

/// Written by the running app so the bridge can find and authenticate to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlFile {
    pub port: u16,
    /// Secret the bridge must send; only readable by the same user.
    pub token: String,
    pub pid: u32,
    pub version: String,
}

/// Where the discovery file lives: the user's app-data folder
/// (`%APPDATA%\io.github.nuncprotunc7.nuncprotune\control.json` on Windows).
/// `NPT_CONTROL_FILE` overrides it (used by tests).
pub fn control_file_path() -> PathBuf {
    if let Some(p) = std::env::var_os("NPT_CONTROL_FILE") {
        return PathBuf::from(p);
    }
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_ID)
        .join("control.json")
}

/// Unsaved audio older than this is from a session long gone.
const STALE_SCRATCH: std::time::Duration = std::time::Duration::from_secs(14 * 24 * 3600);

/// A fresh folder for this run's recordings and imports until the project
/// is saved (`%APPDATA%\io.github.nuncprotunc7.nuncprotune\unsaved-audio\<run>`).
/// Folders left by runs more than two weeks old are removed.
pub fn unsaved_audio_dir() -> PathBuf {
    let root = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_ID)
        .join("unsaved-audio");
    if let Ok(entries) = std::fs::read_dir(&root) {
        for e in entries.flatten() {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > STALE_SCRATCH);
            if old {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    root.join(format!("{stamp}-{}", std::process::id()))
}

impl ControlFile {
    pub fn read(path: &Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Writes the file readable only by the current user.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }
}
