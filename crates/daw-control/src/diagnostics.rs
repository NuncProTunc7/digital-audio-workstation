//! The diagnostic report: what the owner pastes (or Claude fetches) when
//! something goes wrong, so a problem can be understood without a debugger.
//!
//! The app records what happens (devices opening, recordings, errors) in a
//! small in-memory log. Never log from the audio thread: logging locks and
//! allocates. File paths are cut down to their last part so the report can
//! be shared without revealing the user's folders.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

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
