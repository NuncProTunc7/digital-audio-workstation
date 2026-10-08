//! Running plugin work on the app's main (window) thread.
//!
//! Many plugins (everything built with JUCE, e.g. Spitfire LABS) expect to
//! be created, saved, and shown on the thread that runs the app's windows.
//! The app installs a runner at startup; without one (tests, the command
//! line) work runs on the calling thread.

use std::sync::{OnceLock, mpsc};
use std::thread::ThreadId;

type Job = Box<dyn FnOnce() + Send>;
type Runner = Box<dyn Fn(Job) + Send + Sync>;

static RUNNER: OnceLock<(ThreadId, Runner)> = OnceLock::new();

/// Installs the app's main-thread runner. Call once, from the main thread;
/// `post` must run each job on that thread soon.
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

/// Runs `f` on the main thread without waiting (for releasing plugins).
pub fn post(f: impl FnOnce() + Send + 'static) {
    match RUNNER.get() {
        Some((main, post)) if *main != std::thread::current().id() => post(Box::new(f)),
        _ => f(),
    }
}
