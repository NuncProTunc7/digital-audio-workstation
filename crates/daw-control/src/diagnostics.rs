//! The diagnostic report: what the owner pastes (or Claude fetches) when
//! something goes wrong, so a problem can be understood without a debugger.
//!
//! The app records what happens (devices opening, recordings, errors) in a
//! small in-memory log, and, once [`start_log_file`] has run, in a log file
//! kept for the last few runs, so a freeze or crash can still be read about
//! after a restart. Never log from the audio thread: logging locks,
//! allocates and writes. File paths are cut down to their last part so the
//! report can be shared without revealing the user's folders.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use daw_model::Project;

/// Log lines kept for the report.
pub const LOG_LINES: usize = 50;
/// Errors kept for the report.
pub const ERROR_LINES: usize = 10;

struct Log {
    started: Instant,
    lines: VecDeque<String>,
    errors: VecDeque<String>,
}

fn log_state() -> &'static Mutex<Log> {
    static LOG: OnceLock<Mutex<Log>> = OnceLock::new();
    LOG.get_or_init(|| {
        Mutex::new(Log {
            started: Instant::now(),
            lines: VecDeque::with_capacity(LOG_LINES),
            errors: VecDeque::with_capacity(ERROR_LINES),
        })
    })
}

fn push(message: &str, error: bool) {
    let message = scrub_paths(message);
    let Ok(mut log) = log_state().lock() else {
        return;
    };
    let line = format!(
        "[{:>7.1}s] {}{message}",
        log.started.elapsed().as_secs_f64(),
        if error { "ERROR " } else { "" }
    );
    eprintln!("{line}");
    if let Ok(mut file) = log_file().lock()
        && let Some(f) = file.as_mut()
    {
        // Flushed line by line: the app may be killed while frozen.
        let _ = writeln!(f, "{line}");
        let _ = f.flush();
    }
    if log.lines.len() == LOG_LINES {
        log.lines.pop_front();
    }
    log.lines.push_back(line.clone());
    if error {
        if log.errors.len() == ERROR_LINES {
            log.errors.pop_front();
        }
        log.errors.push_back(line);
    }
}

/// Records something that happened, for the report. Not for the audio thread.
pub fn log(message: &str) {
    push(message, false);
}

/// Records an error, for the report's "Recent errors". Not for the audio thread.
pub fn log_error(message: &str) {
    push(message, true);
}

/// The latest log lines, oldest first.
pub fn recent_lines() -> Vec<String> {
    log_state()
        .lock()
        .map(|l| l.lines.iter().cloned().collect())
        .unwrap_or_default()
}

/// The latest errors, oldest first.
pub fn recent_errors() -> Vec<String> {
    log_state()
        .lock()
        .map(|l| l.errors.iter().cloned().collect())
        .unwrap_or_default()
}

/// Log files kept: this run's (`log.txt`) and the runs before it
/// (`log-1.txt` is the previous one).
pub const LOG_FILES: usize = 5;
/// Lines of the previous run's log kept for the report.
pub const PREVIOUS_LINES: usize = 25;
/// Present while the app runs; left behind if it froze, crashed or was
/// ended from Task Manager.
const RUNNING_MARKER: &str = "running";

fn log_file() -> &'static Mutex<Option<std::fs::File>> {
    static FILE: OnceLock<Mutex<Option<std::fs::File>>> = OnceLock::new();
    FILE.get_or_init(|| Mutex::new(None))
}

fn log_folder() -> &'static Mutex<Option<PathBuf>> {
    static DIR: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    DIR.get_or_init(|| Mutex::new(None))
}

/// How the previous run went, learned when this run's log file starts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PreviousRun {
    /// It didn't close normally (it froze or crashed, or was ended from
    /// Task Manager).
    pub ended_unexpectedly: bool,
    /// The end of its log.
    pub last_lines: Vec<String>,
}

fn previous_run() -> &'static Mutex<PreviousRun> {
    static PREV: OnceLock<Mutex<PreviousRun>> = OnceLock::new();
    PREV.get_or_init(|| Mutex::new(PreviousRun::default()))
}

/// `%APPDATA%\io.github.nuncprotunc7.nuncprotune\logs` on Windows.
pub fn log_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(crate::discovery::APP_ID)
        .join("logs")
}

fn log_path(dir: &Path, age: usize) -> PathBuf {
    if age == 0 {
        dir.join("log.txt")
    } else {
        dir.join(format!("log-{age}.txt"))
    }
}

/// Starts this run's log file in `dir` (older runs move up one, the oldest
/// is dropped), and says how the previous run ended. `header` is the first
/// line, e.g. the app's version. Call once, early in start-up.
pub fn start_log_file(dir: &Path, header: &str) -> PreviousRun {
    let _ = std::fs::create_dir_all(dir);
    let ended_unexpectedly = dir.join(RUNNING_MARKER).exists();
    let last_lines: Vec<String> = std::fs::read_to_string(log_path(dir, 0))
        .map(|t| {
            let lines: Vec<&str> = t.lines().collect();
            let from = lines.len().saturating_sub(PREVIOUS_LINES);
            lines[from..].iter().map(|l| (*l).to_owned()).collect()
        })
        .unwrap_or_default();
    for age in (0..LOG_FILES - 1).rev() {
        let _ = std::fs::rename(log_path(dir, age), log_path(dir, age + 1));
    }
    let _ = std::fs::write(dir.join(RUNNING_MARKER), header);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path(dir, 0))
        .ok();
    if let Ok(mut f) = log_file().lock() {
        *f = file;
    }
    if let Ok(mut d) = log_folder().lock() {
        *d = Some(dir.to_owned());
    }
    let previous = PreviousRun {
        ended_unexpectedly,
        last_lines,
    };
    if let Ok(mut p) = previous_run().lock() {
        *p = previous.clone();
    }
    log(&format!("{header} · started {}", utc_now()));
    if ended_unexpectedly {
        log_error(
            "the last run didn't close normally (it froze or crashed, or was ended from Task Manager); the end of its log is in the report",
        );
    }
    previous
}

/// Marks a normal close, so the next start doesn't report a freeze or crash.
pub fn end_log_file() {
    log("Closed normally");
    let dir = log_folder().lock().ok().and_then(|d| d.clone());
    if let Some(dir) = dir {
        let _ = std::fs::remove_file(dir.join(RUNNING_MARKER));
    }
}

/// "2026-10-09 16:05:39 UTC" (to line the log up with Windows' own reports).
fn utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Days since 1970-01-01 to a calendar date (H. Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Notices the app's window thread not answering. The app calls
/// [`Watchdog::beat`] from the window thread about once a second and
/// [`Watchdog::check`] from another thread; a stall is logged once when it
/// passes [`Watchdog::THRESHOLD`], and again when the window recovers.
#[derive(Debug)]
pub struct Watchdog {
    last_beat: Mutex<Instant>,
    /// When the current stall was reported.
    reported: Mutex<Option<Instant>>,
}

impl Default for Watchdog {
    fn default() -> Self {
        Self {
            last_beat: Mutex::new(Instant::now()),
            reported: Mutex::new(None),
        }
    }
}

impl Watchdog {
    /// How long the window may be busy before it counts as frozen.
    pub const THRESHOLD: Duration = Duration::from_secs(5);

    /// The window thread is answering.
    pub fn beat(&self) {
        self.beat_at(Instant::now());
    }

    fn beat_at(&self, now: Instant) {
        if let Ok(mut b) = self.last_beat.lock() {
            *b = now;
        }
    }

    /// Logs a stall or a recovery, if one happened. `doing` names what the
    /// window thread was busy with (a command, a plugin starting), if known.
    /// Returns the logged message.
    pub fn check(&self, doing: Option<&str>) -> Option<String> {
        self.check_at(Instant::now(), doing)
    }

    fn check_at(&self, now: Instant, doing: Option<&str>) -> Option<String> {
        let last = *self.last_beat.lock().ok()?;
        let mut reported = self.reported.lock().ok()?;
        let stalled = now.saturating_duration_since(last);
        if stalled >= Self::THRESHOLD {
            if reported.is_some() {
                return None;
            }
            *reported = Some(last);
            let message = format!(
                "the window stopped responding ({} s so far){}",
                stalled.as_secs(),
                doing.map_or(String::new(), |d| format!(" during {d}"))
            );
            log_error(&message);
            return Some(message);
        }
        let since = reported.take()?;
        let message = format!(
            "the window is responding again after {} s",
            last.saturating_duration_since(since).as_secs()
        );
        log(&message);
        Some(message)
    }
}

/// Replaces file paths in `text` with their last part
/// (`C:\Users\Sam\Music\Song.nptune` becomes `Song.nptune`).
pub fn scrub_paths(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split_inclusive(char::is_whitespace) {
        let body = word.trim_end();
        let space = &word[body.len()..];
        // Keep quotes and trailing punctuation around a path.
        let start = body
            .find(|c: char| !matches!(c, '"' | '\'' | '(' | '['))
            .unwrap_or(body.len());
        let end = body
            .rfind(|c: char| !matches!(c, '"' | '\'' | ')' | ']' | ',' | ';' | ':' | '.'))
            .map_or(start, |i| {
                i + body[i..].chars().next().map_or(1, char::len_utf8)
            });
        let core = &body[start..end.max(start)];
        if looks_like_path(core) {
            out.push_str(&body[..start]);
            out.push_str(core.rsplit(['/', '\\']).next().unwrap_or(""));
            out.push_str(&body[end.max(start)..]);
        } else {
            out.push_str(body);
        }
        out.push_str(space);
    }
    out
}

fn looks_like_path(word: &str) -> bool {
    let drive = word.len() > 2 && word.as_bytes()[1] == b':' && word[2..].starts_with(['\\', '/']);
    let unc = word.starts_with("\\\\");
    let unix = word.starts_with('/') && word[1..].contains('/');
    let home = word.starts_with("~/") || word.starts_with("~\\");
    drive || unc || unix || home || (word.contains('\\') && word.len() > 1)
}

/// What the app knows about its devices and load, for [`report`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DiagnosticInfo {
    pub app_version: String,
    /// e.g. "Windows 10.0.26100".
    pub os: String,
    pub output_device: Option<String>,
    pub sample_rate_hz: Option<u32>,
    /// The buffer size asked for (None = the device's default).
    pub buffer_setting: Option<u32>,
    /// Frames per sound card callback, as measured.
    pub buffer_frames: u32,
    pub audio_error: Option<String>,
    pub input_device: Option<String>,
    /// The microphone is open right now.
    pub input_open: bool,
    pub input_error: Option<String>,
    pub recording_offset_ms: f64,
    pub cpu_load: f32,
    pub cpu_peak: f32,
    pub overloads: u32,
    pub midi_inputs: Vec<String>,
    pub count_in_bars: u32,
    /// Claude's connection, e.g. "listening, last request 12 s ago".
    pub claude: String,
}

/// The report as plain text.
pub fn report(info: &DiagnosticInfo, project: &Project) -> String {
    let mut r = String::new();
    let _ = writeln!(r, "Nunc Pro Tune diagnostic report");
    let _ = writeln!(r, "App: {} · {}", info.app_version, info.os);

    let rate = info.sample_rate_hz.unwrap_or(0);
    let latency = if rate > 0 && info.buffer_frames > 0 {
        format!(
            "{} frames = {:.1} ms",
            info.buffer_frames,
            f64::from(info.buffer_frames) / f64::from(rate) * 1000.0
        )
    } else {
        "unknown".into()
    };
    let setting = info
        .buffer_setting
        .map_or("Default".to_owned(), |f| f.to_string());
    match &info.output_device {
        Some(d) => {
            let _ = writeln!(
                r,
                "Output: {d} · {rate} Hz · buffer {latency} (setting: {setting})"
            );
        }
        None => {
            let _ = writeln!(r, "Output: none");
        }
    }
    if let Some(e) = &info.audio_error {
        let _ = writeln!(r, "Output error: {}", scrub_paths(e));
    }
    let _ = writeln!(
        r,
        "Input: {} ({}) · recording delay {:.0} ms",
        info.input_device.as_deref().unwrap_or("none"),
        if info.input_open { "open" } else { "closed" },
        info.recording_offset_ms
    );
    if let Some(e) = &info.input_error {
        let _ = writeln!(r, "Input error: {}", scrub_paths(e));
    }
    let _ = writeln!(
        r,
        "CPU: now {:.0}% · peak {:.0}% · overloads (crackles) {}",
        info.cpu_load * 100.0,
        info.cpu_peak * 100.0,
        info.overloads
    );
    let _ = writeln!(
        r,
        "MIDI: {}",
        if info.midi_inputs.is_empty() {
            "none".to_owned()
        } else {
            info.midi_inputs.join(", ")
        }
    );
    let _ = writeln!(r, "Count-in: {} bar(s)", info.count_in_bars);
    let _ = writeln!(r, "Claude: {}", info.claude);

    let clips: usize = project.tracks.iter().map(|t| t.clips.len()).sum();
    let _ = writeln!(
        r,
        "Song: \"{}\" · {} BPM · {}/{} · {} tracks · {} clips",
        project.name,
        project.tempo_bpm,
        project.time_signature.numerator,
        project.time_signature.denominator,
        project.tracks.len(),
        clips
    );
    for t in &project.tracks {
        if let Some(pack) = &t.instrument.sample_pack {
            let _ = writeln!(
                r,
                "Sample pack on \"{}\": {}",
                t.name,
                pack_summary(Path::new(pack))
            );
        }
    }

    let errors = recent_errors();
    let _ = writeln!(r, "\nRecent errors ({}):", errors.len());
    for e in &errors {
        let _ = writeln!(r, "  {e}");
    }
    let lines = recent_lines();
    let _ = writeln!(r, "\nLog (last {}):", lines.len());
    for l in &lines {
        let _ = writeln!(r, "  {l}");
    }
    let previous = previous_run().lock().map(|p| p.clone()).unwrap_or_default();
    if previous.ended_unexpectedly {
        let _ = writeln!(
            r,
            "\nThe last run didn't close normally. The end of its log ({}):",
            previous.last_lines.len()
        );
        for l in &previous.last_lines {
            let _ = writeln!(r, "  {}", scrub_paths(l));
        }
    }
    r
}

fn pack_summary(path: &Path) -> String {
    use daw_sampler::PackStatus as S;
    let file = path
        .file_name()
        .map_or_else(|| "?".into(), |n| n.to_string_lossy().into_owned());
    match daw_sampler::pack_status(path) {
        S::NotLoaded => format!("{file}, not loaded"),
        S::Loading => format!("{file}, loading"),
        S::Ready {
            zones,
            megabytes,
            layers_kept,
            layers_total,
            ..
        } => format!(
            "{file}, ready: {zones} zones, {megabytes:.0} MB, {layers_kept}/{layers_total} velocity layers"
        ),
        S::Failed(e) => format!("{file}, failed: {}", scrub_paths(&e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The log is shared by the whole process; tests that read it take turns.
    static LOG_TEST: Mutex<()> = Mutex::new(());

    #[test]
    fn paths_are_cut_to_their_last_part() {
        assert_eq!(
            scrub_paths(r#"could not open "C:\Users\Sam\Music\Song.nptune": denied"#),
            r#"could not open "Song.nptune": denied"#
        );
        assert_eq!(
            scrub_paths("saved /home/sam/songs/Theme.nptune."),
            "saved Theme.nptune."
        );
        assert_eq!(
            scrub_paths(r"pack \\server\share\piano.sfz loaded"),
            "pack piano.sfz loaded"
        );
        // Ordinary text, device names and ratios are left alone.
        let plain = "Output: Speakers (Realtek(R) Audio) at 48000 Hz, 4/4 time";
        assert_eq!(scrub_paths(plain), plain);
    }

    #[test]
    fn report_lists_devices_load_song_and_log() {
        let _turn = LOG_TEST.lock();
        log("Audio output: Speakers, 48000 Hz");
        log_error(r"could not open C:\Users\Sam\secret\a.wav");
        let info = DiagnosticInfo {
            app_version: "0.1.0".into(),
            os: "Windows 10.0.26100".into(),
            output_device: Some("Speakers".into()),
            sample_rate_hz: Some(48_000),
            buffer_setting: Some(256),
            buffer_frames: 256,
            input_device: Some("Headset (WH-1000XM4)".into()),
            recording_offset_ms: 160.0,
            cpu_load: 0.12,
            cpu_peak: 0.85,
            overloads: 3,
            count_in_bars: 1,
            claude: "listening".into(),
            ..DiagnosticInfo::default()
        };
        let text = report(&info, &Project::default());
        for want in [
            "App: 0.1.0 · Windows 10.0.26100",
            "Output: Speakers · 48000 Hz · buffer 256 frames = 5.3 ms (setting: 256)",
            "recording delay 160 ms",
            "peak 85% · overloads (crackles) 3",
            "MIDI: none",
            "tracks",
            "Audio output: Speakers, 48000 Hz",
            "could not open a.wav",
        ] {
            assert!(text.contains(want), "missing {want:?} in:\n{text}");
        }
        assert!(!text.contains("secret"), "a path leaked:\n{text}");
        assert!(text.contains("Recent errors (") && text.contains("ERROR could not open a.wav"));
    }

    #[test]
    fn the_log_file_outlives_the_run_and_notices_a_freeze() {
        let _turn = LOG_TEST.lock();
        let dir = tempfile::tempdir().expect("tmp");
        // First run: closes normally.
        let first = start_log_file(dir.path(), "Nunc Pro Tune test");
        assert!(!first.ended_unexpectedly);
        log("Opened the song");
        end_log_file();
        // Second run: "freezes" (never closes).
        assert!(!start_log_file(dir.path(), "Nunc Pro Tune test").ended_unexpectedly);
        log_error("the window stopped responding (5 s so far) during load_plugin");
        // Third run: learns about it, and the frozen run's log is kept.
        let third = start_log_file(dir.path(), "Nunc Pro Tune test");
        assert!(third.ended_unexpectedly);
        assert!(
            third
                .last_lines
                .iter()
                .any(|l| l.contains("stopped responding")),
            "{third:?}"
        );
        let older = std::fs::read_to_string(dir.path().join("log-2.txt")).expect("first run");
        assert!(older.contains("Opened the song") && older.contains("Closed normally"));
        let text = report(&DiagnosticInfo::default(), &Project::default());
        assert!(text.contains("didn't close normally"), "{text}");
        // Old runs beyond the limit are dropped.
        for _ in 0..LOG_FILES + 2 {
            start_log_file(dir.path(), "again");
        }
        assert!(!dir.path().join(format!("log-{LOG_FILES}.txt")).exists());
        assert!(
            dir.path()
                .join(format!("log-{}.txt", LOG_FILES - 1))
                .exists()
        );
        // Stop writing to the temporary folder.
        end_log_file();
        if let Ok(mut f) = log_file().lock() {
            *f = None;
        }
        if let Ok(mut p) = previous_run().lock() {
            *p = PreviousRun::default();
        }
    }

    #[test]
    fn the_watchdog_reports_a_frozen_window_once_and_its_recovery() {
        let _turn = LOG_TEST.lock();
        let dog = Watchdog::default();
        let t0 = Instant::now();
        dog.beat_at(t0);
        assert_eq!(dog.check_at(t0 + Duration::from_secs(1), None), None);
        let frozen = dog
            .check_at(t0 + Duration::from_secs(6), Some("load_plugin"))
            .expect("reported");
        assert_eq!(
            frozen,
            "the window stopped responding (6 s so far) during load_plugin"
        );
        assert!(recent_errors().iter().any(|e| e.ends_with(&frozen)));
        // Only once per freeze.
        assert_eq!(dog.check_at(t0 + Duration::from_secs(9), None), None);
        dog.beat_at(t0 + Duration::from_secs(12));
        assert_eq!(
            dog.check_at(t0 + Duration::from_secs(12), None).as_deref(),
            Some("the window is responding again after 12 s")
        );
        assert_eq!(dog.check_at(t0 + Duration::from_secs(13), None), None);
    }

    #[test]
    fn dates_are_written_in_utc() {
        let now = utc_now();
        assert!(now.ends_with(" UTC") && now.len() == 23, "{now}");
    }

    #[test]
    fn the_log_keeps_only_the_latest_lines() {
        let _turn = LOG_TEST.lock();
        for n in 0..LOG_LINES + 20 {
            log(&format!("line {n}"));
        }
        let lines = recent_lines();
        assert_eq!(lines.len(), LOG_LINES);
        assert!(lines[LOG_LINES - 1].ends_with(&format!("line {}", LOG_LINES + 19)));
        assert!(!lines.iter().any(|l| l.ends_with(" line 0")));
    }
}
