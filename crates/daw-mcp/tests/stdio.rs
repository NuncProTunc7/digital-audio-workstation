//! Runs the real `npt-mcp` binary the way an AI app does (JSON-RPC over
//! stdin/stdout) against a control server backed by an in-memory project.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command as Process, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use daw_control::{ControlServer, Host};
use daw_engine::{AudioPool, Engine};
use daw_model::Session;
use serde_json::{Value, json};

#[derive(Default)]
struct MemoryHost {
    session: Mutex<Session>,
    path: Mutex<Option<PathBuf>>,
    /// What the app's activity list would show.
    changes: Mutex<Vec<String>>,
    /// What the "window" has selected.
    selection: Mutex<daw_control::Selection>,
}

impl Host for MemoryHost {
    fn session(&self) -> Result<MutexGuard<'_, Session>, String> {
        self.session.lock().map_err(|_| "poisoned".to_owned())
    }
    fn engine(&self) -> Option<Arc<Engine>> {
        None
    }
    fn project_changed(&self, description: &str) {
        if let Ok(mut c) = self.changes.lock() {
            c.push(description.to_owned());
        }
    }
    fn project_path(&self) -> Option<PathBuf> {
        self.path.lock().ok().and_then(|p| p.clone())
    }
    fn set_project_path(&self, path: Option<PathBuf>) {
        if let Ok(mut p) = self.path.lock() {
            *p = path;
        }
    }
    fn audio(&self) -> Arc<AudioPool> {
        AudioPool::in_temp_dir()
    }
    fn selection(&self) -> daw_control::Selection {
        self.selection.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
}

impl Mcp {
    fn start(control_file: &PathBuf) -> Self {
        Self::start_with(control_file, &[])
    }

    fn start_with(control_file: &PathBuf, args: &[&str]) -> Self {
        let mut child = Process::new(env!("CARGO_BIN_EXE_npt-mcp"))
            .args(args)
            .env("NPT_CONTROL_FILE", control_file)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn npt-mcp");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut mcp = Mcp {
            child,
            stdin,
            lines,
            next_id: 1,
        };
        let init = mcp.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "codex-mcp-client", "version": "1.0" }
            }),
        );
        assert_eq!(
            init["result"]["serverInfo"]["name"], "nunc-pro-tune",
            "{init}"
        );
        assert!(
            init["result"]["instructions"]
                .as_str()
                .is_some_and(|s| s.contains("beats"))
        );
        mcp.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        mcp
    }

    fn send(&mut self, message: &Value) {
        writeln!(self.stdin, "{message}").expect("write");
        self.stdin.flush().expect("flush");
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(60))
                .expect("reply from npt-mcp");
            let v: Value = serde_json::from_str(&line).expect("json line");
            if v["id"] == json!(id) {
                return v;
            }
        }
    }

    fn call(&mut self, tool: &str, args: Value) -> Value {
        let reply = self.request("tools/call", json!({ "name": tool, "arguments": args }));
        reply["result"].clone()
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn text_of(result: &Value) -> String {
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn claude_can_list_tools_edit_the_song_and_analyze_it() {
    let dir = std::env::temp_dir().join(format!("npt-mcp-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let control_file = dir.join("control.json");
    let host = Arc::new(MemoryHost::default());
    let server_host = Arc::clone(&host);
    let server = ControlServer::start(&control_file, move |req, ai| {
        daw_control::handle_from(&*server_host, req, ai)
    })
    .expect("server");

    let mut mcp = Mcp::start(&control_file);

    let list = mcp.request("tools/list", json!({}));
    let names: Vec<String> = list["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_owned))
        .collect();
    for expected in [
        "set_tempo",
        "create_clip",
        "batch",
        "get_song",
        "analyze_mix",
    ] {
        assert!(names.iter().any(|n| n == expected), "missing {expected}");
    }

    let r = mcp.call("set_tempo", json!({ "bpm": 133 }));
    assert_eq!(r["isError"], json!(false), "{r}");
    assert_eq!(
        host.session.lock().expect("lock").project().tempo_bpm,
        133.0
    );
    // The app is told which AI made the change.
    assert_eq!(
        host.changes
            .lock()
            .expect("lock")
            .last()
            .map(String::as_str),
        Some("Codex: set tempo")
    );

    let r = mcp.call(
        "batch",
        json!({ "commands": [
            { "command": "add_track", "name": "Lead", "instrument": "synth", "preset": "Chip Square" },
            { "command": "create_clip", "track_id": 4, "start_beats": 0, "length_beats": 4,
              "notes": [ { "pitch": 72, "start_beats": 0, "length_beats": 1 },
                         { "pitch": 76, "start_beats": 1, "length_beats": 1 } ] }
        ] }),
    );
    assert_eq!(r["isError"], json!(false), "{r}");
    let song: Value =
        serde_json::from_str(&text_of(&mcp.call("get_song", json!({})))).expect("song json");
    assert_eq!(song["tracks"][3]["name"], "Lead");
    assert_eq!(song["tracks"][3]["clips"][0]["note_count"], 2);

    let r = mcp.call(
        "analyze_mix",
        json!({ "per_track": false, "spectrogram": true }),
    );
    assert_eq!(r["isError"], json!(false), "{r}");
    assert!(text_of(&r).contains("integrated_lufs"));
    assert!(text_of(&r).contains("Spectrogram in words"));
    assert_eq!(r["content"][1]["type"], "image");
    assert_eq!(r["content"][1]["mimeType"], "image/png");

    // Mistakes come back as tool errors Claude can read and fix.
    let r = mcp.call("delete_clip", json!({ "clip_id": 9999 }));
    assert_eq!(r["isError"], json!(true));
    assert!(text_of(&r).contains("no clip with id 9999"));

    // With the app closed, Claude is told to open it.
    drop(server);
    let r = mcp.call("get_song", json!({}));
    assert_eq!(r["isError"], json!(true));
    assert!(text_of(&r).contains("isn't running"), "{}", text_of(&r));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn core_mode_describes_the_spectrogram_in_words_without_an_image() {
    let dir = std::env::temp_dir().join(format!("npt-mcp-core-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let control_file = dir.join("control.json");
    let host = Arc::new(MemoryHost::default());
    let server_host = Arc::clone(&host);
    let _server = ControlServer::start(&control_file, move |req, ai| {
        daw_control::handle_from(&*server_host, req, ai)
    })
    .expect("server");

    let mut mcp = Mcp::start_with(&control_file, &[daw_control::ai_setup::CORE_FLAG]);
    let r = mcp.call(
        "create_clip",
        json!({ "track_id": 1, "start_beats": 0, "length_beats": 8,
                "notes": [ { "pitch": 60, "start_beats": 0, "length_beats": 4 },
                           { "pitch": 84, "start_beats": 4, "length_beats": 4 } ] }),
    );
    assert_eq!(r["isError"], json!(false), "{r}");

    let r = mcp.call(
        "analyze_mix",
        json!({ "per_track": false, "spectrogram": true }),
    );
    assert_eq!(r["isError"], json!(false), "{r}");
    // Small models get one text answer: no picture, the sound in words.
    assert_eq!(r["content"].as_array().map(Vec::len), Some(1), "{r}");
    let v: Value = serde_json::from_str(&text_of(&r)).expect("json");
    assert!(v.get("spectrogram_png_base64").is_none());
    let words: Vec<&str> = v["spectrogram_summary"]
        .as_array()
        .expect("summary")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(words.iter().any(|l| l.starts_with("Bar 1:")), "{words:?}");
    assert!(words.iter().any(|l| l.starts_with("Bar 2:")), "{words:?}");
    std::fs::remove_dir_all(&dir).ok();
}

/// An AI making game sound effects end to end, as a small local model on
/// the short tool list would: make, read, tweak one setting, export.
#[test]
fn an_ai_can_make_shape_and_export_sound_effects() {
    let dir = std::env::temp_dir().join(format!("npt-mcp-sfx-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let control_file = dir.join("control.json");
    let godot = dir.join("game");
    std::fs::create_dir_all(&godot).expect("godot");
    std::fs::write(godot.join("project.godot"), "config_version=5\n").expect("godot");
    let host = Arc::new(MemoryHost::default());
    let server_host = Arc::clone(&host);
    let _server = ControlServer::start(&control_file, move |req, ai| {
        daw_control::handle_from(&*server_host, req, ai)
    })
    .expect("server");
    let mut mcp = Mcp::start_with(&control_file, &["--core"]);

    let r = mcp.call("add_sound", json!({ "category": "coin", "seed": 7 }));
    assert_eq!(r["isError"], json!(false), "{r}");
    let id = host.session.lock().expect("lock").project().sounds[0].id;

    let r = mcp.call("get_sound", json!({ "sound_id": id }));
    assert_eq!(r["isError"], json!(false), "{r}");
    let sound: Value = serde_json::from_str(&text_of(&r)).expect("json");
    assert_eq!(sound["name"], "coin");
    let before = sound["recipe"].clone();
    assert!(sound["start_hz"].as_f64().is_some_and(|hz| hz > 20.0));

    // "Lower and softer": only the settings named change.
    let r = mcp.call(
        "set_sound",
        json!({ "sound_id": id, "recipe": { "base_freq": 0.25, "punch": 0.1 } }),
    );
    assert_eq!(r["isError"], json!(false), "{r}");
    let after: Value =
        serde_json::from_str(&text_of(&mcp.call("get_sound", json!({ "sound_id": id }))))
            .expect("json");
    assert_eq!(after["recipe"]["base_freq"], json!(0.25));
    assert_eq!(after["recipe"]["decay"], before["decay"], "kept");
    assert_eq!(after["recipe"]["arp_mod"], before["arp_mod"], "kept");

    let r = mcp.call(
        "export_sounds",
        json!({ "project_dir": godot.to_string_lossy() }),
    );
    assert_eq!(r["isError"], json!(false), "{r}");
    assert!(text_of(&r).contains("res://sfx/coin.wav"), "{r}");
    assert!(godot.join("sfx").join("coin.wav").is_file());
    assert_eq!(
        host.changes
            .lock()
            .expect("lock")
            .first()
            .map(String::as_str),
        Some("Codex: add sound")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_ai_can_see_what_the_user_selected() {
    let dir = std::env::temp_dir().join(format!("npt-mcp-sel-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let control_file = dir.join("control.json");
    let host = Arc::new(MemoryHost::default());
    let server_host = Arc::clone(&host);
    let _server = ControlServer::start(&control_file, move |req, ai| {
        daw_control::handle_from(&*server_host, req, ai)
    })
    .expect("server");
    let mut mcp = Mcp::start_with(&control_file, &["--core"]);

    let r = mcp.call("get_selection", json!({}));
    assert_eq!(r["isError"], json!(false), "{r}");
    assert!(text_of(&r).contains("Nothing is selected"), "{r}");

    let r = mcp.call(
        "create_clip",
        json!({
            "track_id": 1,
            "start_beats": 0.0,
            "length_beats": 4.0,
            "name": "Hook",
            "notes": [
                { "pitch": 64, "start_beats": 0.0, "length_beats": 1.0, "velocity": 100 },
                { "pitch": 69, "start_beats": 1.0, "length_beats": 1.0, "velocity": 100 }
            ]
        }),
    );
    assert_eq!(r["isError"], json!(false), "{r}");
    let (clip_id, note_ids) = {
        let session = host.session.lock().expect("lock");
        let clip = &session.project().tracks[0].clips[0];
        (clip.id, clip.notes.iter().map(|n| n.id).collect::<Vec<_>>())
    };
    // The user clicks the clip and selects both notes in the piano roll.
    *host.selection.lock().expect("lock") = daw_control::Selection {
        view: Some("pianoroll".into()),
        track_id: Some(1),
        clip_id: Some(clip_id),
        note_ids,
        sound_id: None,
    };
    let r = mcp.call("get_selection", json!({}));
    assert_eq!(r["isError"], json!(false), "{r}");
    let sel: Value = serde_json::from_str(&text_of(&r)).expect("json");
    assert_eq!(sel["clip"]["id"], json!(clip_id));
    assert_eq!(sel["clip"]["name"], "Hook");
    assert_eq!(sel["notes"]["count"], 2);
    assert_eq!(sel["notes"]["lowest"], "E4");
    assert_eq!(sel["notes"]["highest"], "A4");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_ai_can_repeat_a_section_in_one_call_and_undo_it() {
    let dir = std::env::temp_dir().join(format!("npt-mcp-arrange-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let control_file = dir.join("control.json");
    let host = Arc::new(MemoryHost::default());
    let server_host = Arc::clone(&host);
    let _server = ControlServer::start(&control_file, move |req, ai| {
        daw_control::handle_from(&*server_host, req, ai)
    })
    .expect("server");
    let mut mcp = Mcp::start(&control_file);

    // Verse (bar 1) and chorus (bar 2) on Keys, an outro clip on Bass at bar 3.
    let r = mcp.call(
        "batch",
        json!({ "commands": [
            { "command": "create_clip", "track_id": 1, "start_beats": 0, "length_beats": 8,
              "notes": [ { "pitch": 60, "start_beats": 0, "length_beats": 1 },
                         { "pitch": 67, "start_beats": 4, "length_beats": 1 } ] },
            { "command": "create_clip", "track_id": 2, "start_beats": 8, "length_beats": 4 },
            { "command": "add_marker", "name": "Chorus", "start_beats": 4 }
        ] }),
    );
    assert_eq!(r["isError"], json!(false), "{r}");

    let r = mcp.call("repeat_bars", json!({ "from_bar": 2, "bars": 1 }));
    assert_eq!(r["isError"], json!(false), "{r}");
    {
        let session = host.session.lock().expect("lock");
        let p = session.project();
        let keys: Vec<(f64, usize)> = p.tracks[0]
            .clips
            .iter()
            .map(|c| (c.start_beats, c.notes.len()))
            .collect();
        assert_eq!(keys, vec![(0.0, 2), (8.0, 1)], "the chorus plays twice");
        assert_eq!(p.tracks[1].clips[0].start_beats, 12.0, "the outro moved");
        let names: Vec<&str> = p.markers.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["Chorus", "Chorus 2"]);
    }
    assert_eq!(
        host.changes
            .lock()
            .expect("lock")
            .last()
            .map(String::as_str),
        Some("Codex: repeat bars")
    );

    // One undo puts it all back.
    let r = mcp.call("undo", json!({}));
    assert_eq!(r["isError"], json!(false), "{r}");
    {
        let session = host.session.lock().expect("lock");
        let p = session.project();
        assert_eq!(p.tracks[0].clips.len(), 1);
        assert_eq!(p.tracks[1].clips[0].start_beats, 8.0);
        assert_eq!(p.markers.len(), 1);
    }

    // Bad bars come back as an error the AI can read.
    let r = mcp.call("delete_bars", json!({ "from_bar": 0, "bars": 1 }));
    assert_eq!(r["isError"], json!(true));
    assert!(text_of(&r).contains("start at 1"), "{}", text_of(&r));
    std::fs::remove_dir_all(&dir).ok();
}
