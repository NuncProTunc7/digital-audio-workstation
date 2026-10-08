//! A plugin's own window (its "editor"), shown in a native window the app
//! opens. Windows only for now; elsewhere opening one explains that.
//!
//! Every window lives on the main thread; the functions here hop there.

use crate::{Instance, main_thread};

/// Opens (or brings forward) the window for `instance`. `key` names the
/// window (the app uses the track id); `owner` is the app window's handle
/// (HWND) so the plugin window stays in front of it.
pub fn open(
    instance: &Instance,
    key: u64,
    title: &str,
    owner: Option<isize>,
) -> Result<(), String> {
    let instance = instance.clone();
    let title = title.to_owned();
    main_thread::run(move || {
        crate::guard::watch(instance.info(), || imp::open(&instance, key, &title, owner))
    })?
}

/// Closes the window named `key`, if open.
pub fn close(key: u64) {
    main_thread::post(move || imp::close(key));
}

/// Which windows are open, with the plugin each shows.
pub fn open_windows() -> Vec<(u64, Instance)> {
    main_thread::run(imp::open_windows).unwrap_or_default()
}

#[cfg(not(windows))]
mod imp {
    use crate::Instance;

    pub fn open(_: &Instance, _: u64, _: &str, _: Option<isize>) -> Result<(), String> {
        Err("plugin windows work on Windows only for now".into())
    }
    pub fn close(_: u64) {}
    pub fn open_windows() -> Vec<(u64, Instance)> {
        Vec::new()
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicIsize, Ordering};

    use vst3::Steinberg::Vst::IEditControllerTrait;
    use vst3::Steinberg::*;
    use vst3::{Class, ComPtr, ComWrapper};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    use crate::Instance;

    struct Window {
        hwnd: HWND,
        view: ComPtr<IPlugView>,
        resizable: bool,
        _frame: ComWrapper<Frame>,
        instance: Instance,
    }

    thread_local! {
        // Only touched on the main thread.
        static WINDOWS: RefCell<HashMap<u64, Window>> = RefCell::new(HashMap::new());
    }

    const CLASS_NAME: &str = "NuncProTunePluginWindow";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn style(resizable: bool) -> WINDOW_STYLE {
        let base = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
        if resizable {
            base | WS_THICKFRAME | WS_MAXIMIZEBOX
        } else {
            base
        }
    }

    /// Outer window size for a client area of `w` × `h`.
    fn outer_size(w: i32, h: i32, resizable: bool) -> (i32, i32) {
        let mut r = RECT {
            left: 0,
            top: 0,
            right: w.max(1),
            bottom: h.max(1),
        };
        // SAFETY: valid RECT pointer.
        unsafe { AdjustWindowRectEx(&mut r, style(resizable), 0, 0) };
        (r.right - r.left, r.bottom - r.top)
    }

    /// Lets the plugin ask for a new size.
    struct Frame {
        hwnd: AtomicIsize,
        resizable: bool,
    }

    impl Class for Frame {
        type Interfaces = (IPlugFrame,);
    }

    impl IPlugFrameTrait for Frame {
        unsafe fn resizeView(&self, view: *mut IPlugView, new_size: *mut ViewRect) -> tresult {
            if new_size.is_null() || view.is_null() {
                return kInvalidArgument;
            }
            // SAFETY: non-null rect from the plugin.
            let r = unsafe { *new_size };
            let hwnd = self.hwnd.load(Ordering::Relaxed) as HWND;
            let (w, h) = outer_size(r.right - r.left, r.bottom - r.top, self.resizable);
            // SAFETY: our window; the view is the plugin's live view.
            unsafe {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    0,
                    0,
                    w,
                    h,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                let mut rr = r;
                ((*(*view).vtbl).onSize)(view, &mut rr);
            }
            kResultOk
        }
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // SAFETY: standard window procedure; user data holds our key.
        unsafe {
            let key = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as u64;
            match msg {
                WM_CLOSE => {
                    close(key);
                    0
                }
                WM_SIZE => {
                    let _ = WINDOWS.try_with(|w| {
                        if let Ok(w) = w.try_borrow()
                            && let Some(win) = w.get(&key)
                            && win.resizable
                        {
                            let mut c = RECT {
                                left: 0,
                                top: 0,
                                right: 0,
                                bottom: 0,
                            };
                            GetClientRect(hwnd, &mut c);
                            let mut r = ViewRect {
                                left: 0,
                                top: 0,
                                right: c.right,
                                bottom: c.bottom,
                            };
                            win.view.onSize(&mut r);
                        }
                    });
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
    }

    fn register_class() {
        thread_local!(static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
        if DONE.with(|d| d.replace(true)) {
            return;
        }
        let name = wide(CLASS_NAME);
        // SAFETY: a plain class registration with valid strings; the name
        // is copied by Windows.
        unsafe {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(window_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: GetModuleHandleW(std::ptr::null()),
                hIcon: std::ptr::null_mut(),
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: name.as_ptr(),
                hIconSm: std::ptr::null_mut(),
            };
            RegisterClassExW(&wc);
        }
    }

    pub fn open(
        instance: &Instance,
        key: u64,
        title: &str,
        owner: Option<isize>,
    ) -> Result<(), String> {
        let existing = WINDOWS.with(|w| {
            w.borrow()
                .get(&key)
                .map(|win| (win.hwnd, win.instance.same_as(instance)))
        });
        match existing {
            // SAFETY: our live window.
            Some((hwnd, true)) => {
                unsafe {
                    ShowWindow(hwnd, SW_RESTORE);
                    SetForegroundWindow(hwnd);
                }
                return Ok(());
            }
            // The track now plays a different plugin.
            Some((_, false)) => close(key),
            None => {}
        }
        let controller = instance
            .controller()
            .ok_or("this plugin has no controls to show")?;
        register_class();
        // SAFETY: standard VST3 editor set-up on the main thread with live
        // objects; every pointer passed is valid for the call.
        unsafe {
            let view_ptr = controller.createView(c"editor".as_ptr());
            let view = ComPtr::<IPlugView>::from_raw(view_ptr)
                .ok_or("this plugin has no window of its own")?;
            if view.isPlatformTypeSupported(kPlatformTypeHWND) != kResultOk {
                return Err("this plugin's window doesn't work on Windows".into());
            }
            let mut size = ViewRect {
                left: 0,
                top: 0,
                right: 600,
                bottom: 400,
            };
            view.getSize(&mut size);
            let resizable = view.canResize() == kResultOk;
            let (w, h) = outer_size(size.right - size.left, size.bottom - size.top, resizable);
            let class = wide(CLASS_NAME);
            let title = wide(title);
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                style(resizable),
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                w,
                h,
                owner.map_or(std::ptr::null_mut(), |o| o as HWND),
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            );
            if hwnd.is_null() {
                return Err("couldn't open a window for the plugin".into());
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, key as isize);
            let frame = ComWrapper::new(Frame {
                hwnd: AtomicIsize::new(hwnd as isize),
                resizable,
            });
            if let Some(f) = frame.as_com_ref::<IPlugFrame>() {
                view.setFrame(f.as_ptr());
            }
            if view.attached(hwnd, kPlatformTypeHWND) != kResultOk {
                view.setFrame(std::ptr::null_mut());
                DestroyWindow(hwnd);
                return Err("the plugin couldn't show its window".into());
            }
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
            WINDOWS.with(|w| {
                w.borrow_mut().insert(
                    key,
                    Window {
                        hwnd,
                        view,
                        resizable,
                        _frame: frame,
                        instance: instance.clone(),
                    },
                )
            });
        }
        Ok(())
    }

    pub fn close(key: u64) {
        let Some(win) = WINDOWS.with(|w| w.borrow_mut().remove(&key)) else {
            return;
        };
        // SAFETY: detach the plugin's view before destroying its parent.
        unsafe {
            win.view.removed();
            win.view.setFrame(std::ptr::null_mut());
            DestroyWindow(win.hwnd);
        }
    }

    pub fn open_windows() -> Vec<(u64, Instance)> {
        WINDOWS.with(|w| {
            w.borrow()
                .iter()
                .map(|(k, win)| (*k, win.instance.clone()))
                .collect()
        })
    }
}
