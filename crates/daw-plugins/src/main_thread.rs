//! Running plugin work on the app's main (window) thread.
//!
//! Many plugins (everything built with JUCE, e.g. Spitfire LABS) expect to
//! be created, saved, and shown on the thread that runs the app's windows.
//! The app installs a runner at startup; without one (tests, the command
//! line) work runs on the calling thread.

use std::sync::{Mutex, OnceLock, mpsc};
use std::thread::ThreadId;

type Job = Box<dyn FnOnce() + Send>;
type Runner = Box<dyn Fn(Job) + Send + Sync>;

static RUNNER: OnceLock<(ThreadId, Runner)> = OnceLock::new();

/// Installs the app's main-thread runner. Call once, from the main thread;
/// `post` must run each job on that thread soon. It may run a job at once
/// when called on the main thread (Tauri's `run_on_main_thread` does).
pub fn install(post: impl Fn(Box<dyn FnOnce() + Send>) + Send + Sync + 'static) {
    let _ = RUNNER.set((std::thread::current().id(), Box::new(post)));
}

/// Runs `f` on the main thread and waits for its result. Fails only when the
/// app is closing and drops the job.
pub fn run<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> Result<R, String> {
    match RUNNER.get() {
        Some((main, post)) if *main != std::thread::current().id() => {
            let (tx, rx) = mpsc::sync_channel(1);
            post(Box::new(move || {
                let _ = tx.send(f());
            }));
            rx.recv().map_err(|_| "the app is closing".to_string())
        }
        _ => Ok(f()),
    }
}

/// Whether the app installed a main-thread runner.
pub fn has_runner() -> bool {
    RUNNER.get().is_some()
}

/// Queues `f` for the main thread even when called from it, so the caller
/// finishes first (and holds no locks when `f` runs). Without a runner, `f`
/// runs now.
pub fn post_later(f: impl FnOnce() + Send + 'static) {
    match RUNNER.get() {
        // The runner may run a job posted from the main thread at once,
        // while the caller still holds its locks; posting from another
        // thread always queues it.
        Some((main, _)) if *main == std::thread::current().id() => relay(Box::new(f)),
        Some((_, post)) => post(Box::new(f)),
        None => f(),
    }
}

/// Hands `job` to a helper thread that posts it to the main thread.
fn relay(job: Job) {
    static RELAY: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
    let sender = RELAY.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        let _ = std::thread::Builder::new()
            .name("npt-main-relay".into())
            .spawn(move || {
                for job in rx {
                    if let Some((_, post)) = RUNNER.get() {
                        post(job);
                    }
                }
            });
        Mutex::new(tx)
    });
    if let Ok(tx) = sender.lock() {
        let _ = tx.send(job);
    }
}

/// Runs `f` on the main thread without waiting (for releasing plugins).
pub fn post(f: impl FnOnce() + Send + 'static) {
    match RUNNER.get() {
        Some((main, post)) if *main != std::thread::current().id() => post(Box::new(f)),
        _ => f(),
    }
}
