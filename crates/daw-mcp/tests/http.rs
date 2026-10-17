//! The bridge in web mode (`npt-mcp --http`), the way a local AI app that
//! only takes a web address reaches it: plain HTTP on this computer, with
//! the secret key.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command as Process, Stdio};
use std::time::Duration;

const KEY: &str = "test-key-1234567890";

struct Bridge {
    child: Child,
    port: u16,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start() -> Bridge {
    let mut child = Process::new(env!("CARGO_BIN_EXE_npt-mcp"))
        .args(["--http", "--port", "0"])
        .env("NPT_MCP_KEY", KEY)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn npt-mcp");
    let mut line = String::new();
    BufReader::new(child.stdout.take().expect("stdout"))
        .read_line(&mut line)
        .expect("address line");
    // "npt-mcp: listening on http://127.0.0.1:PORT/mcp"
    let port = line
        .trim()
        .rsplit(':')
        .next()
        .and_then(|p| p.trim_end_matches("/mcp").parse().ok())
        .unwrap_or_else(|| panic!("no port in {line:?}"));
    Bridge { child, port }
}

/// Sends one POST to /mcp and returns (status, body).
fn post(port: u16, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(30)))
        .expect("timeout");
    let mut req = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\n\
         Accept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes()).expect("send");
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let status = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (status, out)
}

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"open-webui","version":"1"}}}"#;

#[test]
fn a_local_app_connects_with_the_key() {
    let b = start();
    let bearer = format!("Bearer {KEY}");
    let (status, body) = post(b.port, "/mcp", &[("Authorization", &bearer)], INITIALIZE);
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("nunc-pro-tune"), "{body}");
    // Apps that only take an address can put the key in it.
    let (status, body) = post(b.port, &format!("/mcp?key={KEY}"), &[], INITIALIZE);
    assert_eq!(status, 200, "{body}");
}

#[test]
fn requests_without_the_key_are_refused() {
    let b = start();
    let (status, _) = post(b.port, "/mcp", &[], INITIALIZE);
    assert_eq!(status, 401);
    let (status, _) = post(
        b.port,
        "/mcp",
        &[("Authorization", "Bearer wrong")],
        INITIALIZE,
    );
    assert_eq!(status, 401);
}

#[test]
fn web_pages_are_refused_even_with_the_key() {
    let b = start();
    let bearer = format!("Bearer {KEY}");
    let (status, _) = post(
        b.port,
        "/mcp",
        &[
            ("Authorization", &bearer),
            ("Origin", "https://example.com"),
        ],
        INITIALIZE,
    );
    assert_eq!(status, 403);
    // A local web app (Open WebUI in a browser tab) is fine.
    let (status, body) = post(
        b.port,
        "/mcp",
        &[
            ("Authorization", &bearer),
            ("Origin", "http://localhost:3000"),
        ],
        INITIALIZE,
    );
    assert_eq!(status, 200, "{body}");
}

#[test]
fn it_will_not_start_without_a_key() {
    let out = Process::new(env!("CARGO_BIN_EXE_npt-mcp"))
        .args(["--http", "--port", "0"])
        .env_remove("NPT_MCP_KEY")
        .output()
        .expect("run");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("NPT_MCP_KEY"));
}

#[test]
fn a_tool_call_works_over_the_web_address() {
    let b = start();
    let bearer = format!("Bearer {KEY}");
    let (status, body) = post(b.port, "/mcp", &[("Authorization", &bearer)], INITIALIZE);
    assert_eq!(status, 200, "{body}");
    let session = body
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("mcp-session-id")
                .then(|| v.trim().to_owned())
        })
        .unwrap_or_default();
    let mut headers = vec![
        ("Authorization", bearer.as_str()),
        ("MCP-Protocol-Version", "2025-06-18"),
    ];
    if !session.is_empty() {
        headers.push(("Mcp-Session-Id", session.as_str()));
    }
    post(
        b.port,
        "/mcp",
        &headers,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    );
    // The guide is built into the bridge, so this works without the app.
    let (status, body) = post(
        b.port,
        "/mcp",
        &headers,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"read_guide","arguments":{"page":"sound-effects"}}}"#,
    );
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("Sound effects"), "{body}");
}
