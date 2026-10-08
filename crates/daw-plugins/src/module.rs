//! Loading a VST3 module (a `.vst3` bundle folder or single file) and
//! reading the plugins its factory offers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use vst3::ComPtr;
use vst3::Steinberg::*;

use crate::com::{Shared, from_c};
use crate::{PluginInfo, PluginKind};

/// A loaded plugin module. Instances hold an `Arc` so the code stays loaded
/// while any of them lives.
pub struct Module {
    factory: Option<Shared<IPluginFactory>>,
    library: Option<libloading::Library>,
    /// The `.vst3` path the user's plugin folder lists.
    pub path: PathBuf,
}

impl Drop for Module {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // The factory must be released while its code is still loaded.
        self.factory = None;
        #[cfg(windows)]
        if let Some(lib) = &self.library {
            // SAFETY: ExitDll is the documented VST3 module exit on Windows,
            // taking no arguments.
            unsafe {
                if let Ok(exit) = lib.get::<unsafe extern "system" fn() -> bool>(b"ExitDll\0") {
                    exit();
                }
            }
        }
        #[cfg(target_os = "linux")]
        if let Some(lib) = &self.library {
            // SAFETY: ModuleExit is the documented VST3 module exit on Linux.
            unsafe {
                if let Ok(exit) = lib.get::<unsafe extern "system" fn() -> bool>(b"ModuleExit\0") {
                    exit();
                }
            }
        }
    }
}

/// The code file inside a `.vst3` bundle, or the path itself for a
/// single-file module.
pub fn binary_path(path: &Path) -> PathBuf {
    if !path.is_dir() {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    #[cfg(windows)]
    let (arch, ext) = ("x86_64-win", "vst3");
    #[cfg(not(windows))]
    let (arch, ext) = ("x86_64-linux", "so");
    let mut file = stem;
    file.push(".");
    file.push(ext);
    path.join("Contents").join(arch).join(file)
}

fn cache() -> &'static Mutex<HashMap<PathBuf, Weak<Module>>> {
    static C: OnceLock<Mutex<HashMap<PathBuf, Weak<Module>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

impl Module {
    /// Loads (or reuses) the module at `path`. Runs the plugin's own code.
    pub fn open(path: &Path) -> Result<Arc<Module>, String> {
        let mut map = cache().lock().map_err(|e| e.to_string())?;
        if let Some(m) = map.get(path).and_then(Weak::upgrade) {
            return Ok(m);
        }
        let module = Arc::new(Self::load(path)?);
        map.insert(path.to_path_buf(), Arc::downgrade(&module));
        Ok(module)
    }

    #[allow(unsafe_code)]
    fn load(path: &Path) -> Result<Module, String> {
        let binary = binary_path(path);
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        // SAFETY: loading a plugin runs its initialisers; that is what the
        // user asked for by installing and choosing it.
        let library = unsafe { libloading::Library::new(&binary) }
            .map_err(|e| format!("couldn't load {name}: {e}"))?;
        // SAFETY: the VST3 entry points take no arguments.
        unsafe {
            #[cfg(windows)]
            if let Ok(init) = library.get::<unsafe extern "system" fn() -> bool>(b"InitDll\0")
                && !init()
            {
                return Err(format!("{name} refused to start"));
            }
            #[cfg(target_os = "linux")]
            if let Ok(init) = library
                .get::<unsafe extern "system" fn(*mut std::ffi::c_void) -> bool>(b"ModuleEntry\0")
                && !init(std::ptr::null_mut())
            {
                return Err(format!("{name} refused to start"));
            }
        }
        // SAFETY: GetPluginFactory is the VST3 entry point; it returns an
        // owned reference (or null).
        let factory = unsafe {
            let get = library
                .get::<unsafe extern "system" fn() -> *mut IPluginFactory>(b"GetPluginFactory\0")
                .map_err(|_| format!("{name} is not a VST3 plugin"))?;
            ComPtr::from_raw(get())
        }
        .ok_or_else(|| format!("{name} has no plugins inside"))?;
        Ok(Module {
            factory: Some(Shared(factory)),
            library: Some(library),
            path: path.to_path_buf(),
        })
    }

    /// Wraps a factory that is already in this process (the test plugin).
    ///
    /// # Safety
    /// `factory` must be an owned reference to a live `IPluginFactory`.
    #[allow(unsafe_code)]
    pub unsafe fn from_factory(
        factory: *mut IPluginFactory,
        path: &Path,
    ) -> Result<Arc<Module>, String> {
        // SAFETY: the caller hands over one reference.
        let factory = unsafe { ComPtr::from_raw(factory) }.ok_or("no factory")?;
        let module = Arc::new(Module {
            factory: Some(Shared(factory)),
            library: None,
            path: path.to_path_buf(),
        });
        cache()
            .lock()
            .map_err(|e| e.to_string())?
            .insert(path.to_path_buf(), Arc::downgrade(&module));
        Ok(module)
    }

    pub(crate) fn factory(&self) -> Result<&ComPtr<IPluginFactory>, String> {
        self.factory
            .as_ref()
            .map(|f| &f.0)
            .ok_or_else(|| "module unloaded".into())
    }

    /// The instruments and effects in this module.
    #[allow(unsafe_code)]
    pub fn plugins(&self) -> Result<Vec<PluginInfo>, String> {
        let factory = self.factory()?;
        // SAFETY: plain calls on a live factory with valid out-structs.
        unsafe {
            let mut finfo: PFactoryInfo = std::mem::zeroed();
            let factory_vendor = if factory.getFactoryInfo(&mut finfo) == kResultOk {
                from_c(&finfo.vendor)
            } else {
                String::new()
            };
            let f2 = factory.cast::<IPluginFactory2>();
            let mut out = Vec::new();
            for i in 0..factory.countClasses() {
                let (cid, category, name, sub, vendor, version) = if let Some(f2) = &f2 {
                    let mut c: PClassInfo2 = std::mem::zeroed();
                    if f2.getClassInfo2(i, &mut c) != kResultOk {
                        continue;
                    }
                    (
                        c.cid,
                        from_c(&c.category),
                        from_c(&c.name),
                        from_c(&c.subCategories),
                        from_c(&c.vendor),
                        from_c(&c.version),
                    )
                } else {
                    let mut c: PClassInfo = std::mem::zeroed();
                    if factory.getClassInfo(i, &mut c) != kResultOk {
                        continue;
                    }
                    (
                        c.cid,
                        from_c(&c.category),
                        from_c(&c.name),
                        String::new(),
                        String::new(),
                        String::new(),
                    )
                };
                if category != "Audio Module Class" {
                    continue;
                }
                out.push(PluginInfo {
                    uid: crate::uid_string(&cid),
                    kind: PluginKind::from_subcategories(&sub),
                    name,
                    vendor: if vendor.is_empty() {
                        factory_vendor.clone()
                    } else {
                        vendor
                    },
                    version,
                    categories: sub,
                    path: self.path.display().to_string(),
                });
            }
            Ok(out)
        }
    }
}
