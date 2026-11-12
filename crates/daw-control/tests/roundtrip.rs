//! Server and client talking over a real local socket.

use std::sync::Arc;

use daw_control::{ControlClient, ControlFile, ControlServer, Request, Response};

fn temp_file(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("npt-rt-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    dir.join("control.json")
}

#[test]
fn client_reaches_server_with_token() {
    let path = temp_file("ok");
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let server = ControlServer::start(&path, move |req, ai| {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        match req {
            Request::Ping => Response::ok(serde_json::json!({"pong": true, "ai": ai})),
            _ => Response::error("unsupported in this test"),
        }
    })
    .expect("start");

    let client = ControlClient::from_file(&path).expect("discover");
    let v = client.call(&Request::Ping).expect("transport").expect("ok");
    assert_eq!(v["pong"], true);
    assert_eq!(v["ai"], "AI", "an unnamed AI");
    // The bridge names the AI it speaks for.
    let named = ControlClient::from_file(&path)
        .expect("discover")
        .speaking_for(Some("Codex"));
    let v = named.call(&Request::Ping).expect("transport").expect("ok");
    assert_eq!(v["ai"], "Codex");
    let err = client
        .call(&Request::Undo)
        .expect("transport")
        .expect_err("app error");
    assert!(err.contains("unsupported"));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 3);

    // Dropping the server removes the discovery file.
    drop(server);
    assert!(!path.exists());
    assert!(ControlClient::from_file(&path).is_err());
}

#[test]
fn wrong_token_is_rejected_before_the_handler_runs() {
    let path = temp_file("bad");
    let server = ControlServer::start(&path, |_, _| panic!("handler must not run")).expect("start");
    let client = ControlClient::new(server.port(), "not-the-token".into());
    let err = client.call(&Request::Ping).expect_err("rejected");
    assert!(matches!(err, daw_control::ClientError::AccessDenied));
}

#[test]
fn bridge_status_distinguishes_connected_denied_stale_and_missing() {
    use daw_control::bridge_status::probe;

    let dir = tempfile::tempdir().expect("test directory");
    let path = dir.path().join("control.json");
    assert_eq!(probe(&path).status, "control_file_missing");
    let server = ControlServer::start(&path, |_, _| panic!("status should bypass app requests"))
        .expect("start");
    let report = probe(&path);
    assert_eq!(report.status, "connected");
    assert!(report.capabilities.iter().any(|c| c == "audio_export"));
    let encoded = serde_json::to_string(&report).expect("serialize");
    assert!(!encoded.contains(&server.file().token));

    let discovery = server.file().clone();
    let mut wrong = discovery.clone();
    wrong.token = "wrong".into();
    wrong.write(&path).expect("replace");
    let report = probe(&path);
    assert_eq!(report.status, "access_denied");
    assert_eq!(report.failed_step, Some("handshake"));
    discovery.write(&path).expect("restore");
    drop(server);
    assert_eq!(probe(&path).status, "control_file_missing");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let closed = listener.local_addr().expect("address").port();
    drop(listener);
    ControlFile {
        port: closed,
        ..discovery
    }
    .write(&path)
    .expect("write stale");
    let stale = probe(&path);
    assert_eq!(stale.status, "stale_control_file", "{stale:?}");
    assert!(path.exists(), "status is read-only");
}

#[test]
fn bridge_status_reports_incompatible_and_timed_out_handshakes() {
    use daw_control::bridge_status::probe;

    let dir = tempfile::tempdir().expect("test directory");
    let path = dir.path().join("control.json");
    for (response, expected) in [
        (
            Some(r#"{"ok":true,"result":{"app":"Nunc Pro Tune","protocol_version":99}}"#),
            "protocol_version_mismatch",
        ),
        (None, "connection_timed_out"),
    ] {
        let listener = tiny_http::Server::http("127.0.0.1:0").expect("listen");
        ControlFile {
            port: listener.server_addr().to_ip().expect("address").port(),
            token: "secret".into(),
            pid: std::process::id(),
            version: "test".into(),
        }
        .write(&path)
        .expect("write");
        let worker = std::thread::spawn(move || {
            let mut request = listener.recv().expect("accept");
            let mut body = Vec::new();
            request.as_reader().read_to_end(&mut body).expect("read");
            if let Some(response) = response {
                request
                    .respond(tiny_http::Response::from_string(response))
                    .expect("reply");
            } else {
                std::thread::sleep(std::time::Duration::from_millis(1250));
            }
        });
        let report = probe(&path);
        assert_eq!(report.status, expected);
        if response.is_some() {
            assert_eq!(report.protocol_version, Some(99));
        }
        worker.join().expect("thread");
    }
}

#[cfg(windows)]
#[test]
fn bridge_status_distinguishes_access_denied_reading_control_file() {
    let dir = tempfile::tempdir().expect("test directory");
    // Windows returns access denied when the file path can only be opened as a directory.
    let report = daw_control::bridge_status::probe(dir.path());
    assert_eq!(report.status, "access_denied");
    assert_eq!(report.failed_step, Some("read_control_file"));
}
