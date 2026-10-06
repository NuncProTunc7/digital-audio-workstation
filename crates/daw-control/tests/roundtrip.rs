//! Server and client talking over a real local socket.

use std::sync::Arc;

use daw_control::{ControlClient, ControlServer, Request, Response};

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
    let server = ControlServer::start(&path, move |req| {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        match req {
            Request::Ping => Response::ok(serde_json::json!({"pong": true})),
            _ => Response::error("unsupported in this test"),
        }
    })
    .expect("start");

    let client = ControlClient::from_file(&path).expect("discover");
    let v = client.call(&Request::Ping).expect("transport").expect("ok");
    assert_eq!(v["pong"], true);
    let err = client
        .call(&Request::Undo)
        .expect("transport")
        .expect_err("app error");
    assert!(err.contains("unsupported"));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);

    // Dropping the server removes the discovery file.
    drop(server);
    assert!(!path.exists());
    assert!(ControlClient::from_file(&path).is_err());
}

#[test]
fn wrong_token_is_rejected_before_the_handler_runs() {
    let path = temp_file("bad");
    let server = ControlServer::start(&path, |_| panic!("handler must not run")).expect("start");
    let client = ControlClient::new(server.port(), "not-the-token".into());
    let err = client
        .call(&Request::Ping)
        .expect("transport")
        .expect_err("rejected");
    assert!(err.contains("token"), "{err}");
}
