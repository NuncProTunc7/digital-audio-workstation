//! Runs the real `npt-mcp` binary the way Claude does (JSON-RPC over
//! stdin/stdout) against a control server backed by an in-memory project.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command as Process, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use daw_control::{ControlServer, Host};
use daw_engine::Engine;
use daw_model::Session;
use serde_json::{Value, json};

#[derive(Default)]
struct MemoryHost {
    session: Mutex<Session>,
    path: Mutex<Option<PathBuf>>,
}

impl Host for MemoryHost {
    fn session(&self) -> Result<MutexGuard<'_, Session>, String> {
        self.session.lock().map_err(|_| "poisoned".to_owned())
    }
    fn engine(&self) -> Option<Arc<Engine>> {
        None
    }
    fn project_changed(&self, _description: &str) {}
    fn project_path(&self) -> Option<PathBuf> {
        self.path.lock().ok().and_then(|p| p.clone())
    }
    fn set_project_path(&self, path: Option<PathBuf>) {
        if let Ok(mut p) = self.path.lock() {
            *p = path;
        }
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
        let mut child = Process::new(env!("CARGO_BIN_EXE_npt-mcp"))
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
                "clientInfo": { "name": "test", "version": "1.0" }
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
    let server = ControlServer::start(&control_file, move |req| {
        daw_control::handle(&*server_host, req)
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
