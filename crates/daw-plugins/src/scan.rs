//! Finding the VST3 plugins installed on this computer.
//!
//! A bundle's `moduleinfo.json` describes it without running its code. For
//! older plugins without one, the module is loaded in a child process (the
//! app re-runs itself with [`SCAN_FLAG`]), so a plugin that crashes while
//! being looked at can't take the app down. Results are cached by path and
//! modification time.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{PluginInfo, PluginKind};

/// The command-line flag that makes the app scan one module and exit.
pub const SCAN_FLAG: &str = "--scan-vst3";

/// How long a module may take to describe itself.
const SCAN_TIMEOUT: Duration = Duration::from_secs(60);

/// Extra plugin folders (separated like `PATH`), looked through first.
pub const FOLDERS_ENV: &str = "NPT_VST3_PATH";

/// The standard VST3 folders for this platform, after any in
/// [`FOLDERS_ENV`].
pub fn default_folders() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::env::var_os(FOLDERS_ENV)
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default();
    #[cfg(windows)]
    {
        if let Some(common) = std::env::var_os("CommonProgramFiles") {
            out.push(PathBuf::from(common).join("VST3"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            out.push(
                PathBuf::from(local)
                    .join("Programs")
                    .join("Common")
                    .join("VST3"),
            );
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(home) = std::env::var_os("HOME") {
            out.push(PathBuf::from(home).join(".vst3"));
        }
        out.push(PathBuf::from("/usr/lib/vst3"));
        out.push(PathBuf::from("/usr/local/lib/vst3"));
    }
    out
}

/// Every `.vst3` module (bundle folder or file) under `folders`.
pub fn find_modules(folders: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = folders.to_vec();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_vst3 = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("vst3"));
            if is_vst3 {
                out.push(path);
            } else if path.is_dir() {
                stack.push(path);
            }
        }
    }
    out.sort();
    out
}

#[derive(Deserialize)]
struct ModuleInfo {
    #[serde(rename = "Factory Info", default)]
    factory: Option<FactoryInfo>,
    #[serde(rename = "Classes", default)]
    classes: Vec<ClassEntry>,
}

#[derive(Deserialize)]
struct FactoryInfo {
    #[serde(rename = "Vendor", default)]
    vendor: String,
}

#[derive(Deserialize)]
struct ClassEntry {
    #[serde(rename = "CID")]
    cid: String,
    #[serde(rename = "Category", default)]
    category: String,
    #[serde(rename = "Name", default)]
    name: String,
    #[serde(rename = "Vendor", default)]
    vendor: String,
    #[serde(rename = "Version", default)]
    version: String,
    #[serde(rename = "Sub Categories", default)]
    sub_categories: Vec<String>,
}

/// Reads a bundle's `moduleinfo.json`, if it has a usable one.
pub fn read_moduleinfo(module: &Path) -> Option<Vec<PluginInfo>> {
    let file = module
        .join("Contents")
        .join("Resources")
        .join("moduleinfo.json");
    let text = std::fs::read_to_string(file).ok()?;
    let info: ModuleInfo = serde_json::from_str(&text).ok()?;
    let factory_vendor = info.factory.map(|f| f.vendor).unwrap_or_default();
    let plugins: Vec<PluginInfo> = info
        .classes
        .into_iter()
        .filter(|c| c.category == "Audio Module Class" && crate::parse_uid(&c.cid).is_some())
        .map(|c| {
            let categories = c.sub_categories.join("|");
            PluginInfo {
                uid: c.cid.to_ascii_uppercase(),
                kind: PluginKind::from_subcategories(&categories),
                name: c.name,
                vendor: if c.vendor.is_empty() {
                    factory_vendor.clone()
                } else {
                    c.vendor
                },
                version: c.version,
                categories,
                path: module.display().to_string(),
            }
        })
        .collect();
    (!plugins.is_empty()).then_some(plugins)
}

/// Loads the module here and lists its plugins (runs the plugin's code).
pub fn scan_in_process(module: &Path) -> Result<Vec<PluginInfo>, String> {
    crate::Module::open(module)?.plugins()
}

/// What the child process prints.
#[derive(Serialize, Deserialize)]
struct ChildResult {
    plugins: Option<Vec<PluginInfo>>,
    error: Option<String>,
}

/// If the arguments ask for a scan (`<exe> --scan-vst3 <path>`), does it,
/// prints the result as JSON, and returns the exit code. Call it first
/// thing in `main`.
pub fn child_main(args: &[String]) -> Option<i32> {
    let i = args.iter().position(|a| a == SCAN_FLAG)?;
    let path = PathBuf::from(args.get(i + 1)?);
    let result = match scan_in_process(&path) {
        Ok(p) => ChildResult {
            plugins: Some(p),
            error: None,
        },
        Err(e) => ChildResult {
            plugins: None,
            error: Some(e),
        },
    };
    println!("{}", serde_json::to_string(&result).unwrap_or_default());
    Some(0)
}

/// Scans one module in a child process (`exe` must call [`child_main`]).
pub fn scan_in_child(exe: &Path, module: &Path) -> Result<Vec<PluginInfo>, String> {
    let name = module
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let mut child = Command::new(exe)
        .arg(SCAN_FLAG)
        .arg(module)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("couldn't check {name}: {e}"))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > SCAN_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{name} took too long to start"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut out);
    }
    let parsed = out
        .lines()
        .rev()
        .find_map(|l| serde_json::from_str::<ChildResult>(l).ok());
    match parsed {
        Some(ChildResult {
            plugins: Some(p), ..
        }) => Ok(p),
        Some(ChildResult { error: Some(e), .. }) => Err(e),
        _ if !status.success() => Err(format!("{name} crashed while loading")),
        _ => Err(format!("{name} didn't say what it contains")),
    }
}

/// One module's remembered scan.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheEntry {
    pub path: String,
    /// Seconds since 1970 of the module's last change.
    pub modified: u64,
    pub plugins: Vec<PluginInfo>,
    /// Why it couldn't be used (crashed, not a plugin...).
    pub error: Option<String>,
}

/// Remembered scans, so start-up doesn't reload every plugin.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanCache {
    pub modules: Vec<CacheEntry>,
}

impl ScanCache {
    /// Every usable plugin.
    pub fn plugins(&self) -> Vec<PluginInfo> {
        self.modules
            .iter()
            .flat_map(|m| m.plugins.iter().cloned())
            .collect()
    }
    /// Modules that failed, with the reason.
    pub fn failures(&self) -> Vec<(String, String)> {
        self.modules
            .iter()
            .filter_map(|m| m.error.clone().map(|e| (m.path.clone(), e)))
            .collect()
    }
}

fn modified_secs(path: &Path) -> u64 {
    // A bundle's folder time doesn't change when its binary is replaced.
    let target = crate::module::binary_path(path);
    std::fs::metadata(&target)
        .or_else(|_| std::fs::metadata(path))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// Brings the cache up to date with `folders`: new or changed modules are
/// described (by `moduleinfo.json`, else `scanner`), removed ones dropped.
pub fn rescan(
    folders: &[PathBuf],
    old: &ScanCache,
    scanner: impl Fn(&Path) -> Result<Vec<PluginInfo>, String>,
) -> ScanCache {
    let modules = find_modules(folders)
        .into_iter()
        .map(|path| {
            let key = path.display().to_string();
            let modified = modified_secs(&path);
            if let Some(hit) = old
                .modules
                .iter()
                .find(|m| m.path == key && m.modified == modified)
            {
                return hit.clone();
            }
            let result = read_moduleinfo(&path).map_or_else(|| scanner(&path), Ok);
            match result {
                Ok(plugins) => CacheEntry {
                    path: key,
                    modified,
                    plugins,
                    error: None,
                },
                Err(e) => CacheEntry {
                    path: key,
                    modified,
                    plugins: Vec::new(),
                    error: Some(e),
                },
            }
        })
        .collect();
    ScanCache { modules }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moduleinfo_describes_a_bundle_without_loading_it() {
        let dir = tempfile::tempdir().expect("tmp");
        let bundle = dir.path().join("Vendor").join("Big Reverb.vst3");
        let res = bundle.join("Contents").join("Resources");
        std::fs::create_dir_all(&res).expect("dirs");
        std::fs::write(
            res.join("moduleinfo.json"),
            r#"{
              "Name": "Big Reverb", "Version": "1.2.0",
              "Factory Info": { "Vendor": "Acme", "URL": "", "E-Mail": "" },
              "Classes": [
                { "CID": "0123456789ABCDEF0123456789ABCDEF", "Category": "Audio Module Class",
                  "Name": "Big Reverb", "Vendor": "", "Version": "1.2.0",
                  "Sub Categories": ["Fx", "Reverb"] },
                { "CID": "FEDCBA9876543210FEDCBA9876543210", "Category": "Component Controller Class",
                  "Name": "Big Reverb Controller" }
              ]
            }"#,
        )
        .expect("write");
        // Something that is not a plugin, and a plain folder to walk past.
        std::fs::write(dir.path().join("readme.txt"), "hi").expect("write");

        let cache = rescan(&[dir.path().to_path_buf()], &ScanCache::default(), |_| {
            panic!("moduleinfo should be enough")
        });
        let plugins = cache.plugins();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "Big Reverb");
        assert_eq!(plugins[0].vendor, "Acme");
        assert_eq!(plugins[0].kind, PluginKind::Effect);
        assert_eq!(plugins[0].categories, "Fx|Reverb");

        // Unchanged modules come from the cache.
        let again = rescan(&[dir.path().to_path_buf()], &cache, |_| panic!("cached"));
        assert_eq!(again.plugins(), plugins);
    }

    #[test]
    fn modules_without_moduleinfo_use_the_scanner_and_failures_are_kept() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("Broken.vst3"), b"not a dll").expect("write");
        let cache = rescan(&[dir.path().to_path_buf()], &ScanCache::default(), |p| {
            Err(format!("{} crashed while loading", p.display()))
        });
        assert!(cache.plugins().is_empty());
        assert_eq!(cache.failures().len(), 1);
    }
}
