use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::discovery::ControlFile;
use crate::protocol::{Request, Response};

/// Requests larger than this are rejected (no legitimate request comes close).
const MAX_BODY_BYTES: u64 = 16 * 1024 * 1024;

/// HTTP endpoint on 127.0.0.1 that accepts [`Request`]s carrying the secret
/// token. Dropping it stops the server and removes the discovery file.
pub struct ControlServer {
    server: Arc<tiny_http::Server>,
    thread: Option<JoinHandle<()>>,
    file_path: PathBuf,
    file: ControlFile,
}

impl ControlServer {
    /// Starts on a free port and writes the discovery file at `file_path`.
    pub fn start<F>(file_path: &Path, handler: F) -> std::io::Result<ControlServer>
    where
        F: Fn(Request) -> Response + Send + Sync + 'static,
    {
        let server = tiny_http::Server::http("127.0.0.1:0").map_err(std::io::Error::other)?;
        let port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .ok_or_else(|| std::io::Error::other("server has no port"))?;
        let file = ControlFile {
            port,
            token: new_token()?,
            pid: std::process::id(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        };
        file.write(file_path)?;

        let server = Arc::new(server);
        let token = file.token.clone();
        let thread_server = Arc::clone(&server);
        let thread = std::thread::Builder::new()
            .name("npt-control".into())
            .spawn(move || {
                for request in thread_server.incoming_requests() {
                    serve(request, &token, &handler);
                }
            })?;
        Ok(ControlServer {
            server,
            thread: Some(thread),
            file_path: file_path.to_owned(),
            file,
        })
    }

    pub fn port(&self) -> u16 {
        self.file.port
    }

    pub fn file(&self) -> &ControlFile {
        &self.file
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        // Only remove the file if it is still ours (another instance may
        // have replaced it).
        if ControlFile::read(&self.file_path).is_ok_and(|f| f.token == self.file.token) {
            let _ = std::fs::remove_file(&self.file_path);
        }
    }
}

fn new_token() -> std::io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn respond(request: tiny_http::Request, status: u16, body: &Response) {
    let json = serde_json::to_string(body).unwrap_or_else(|_| r#"{"ok":false}"#.into());
    let header = tiny_http::Header::from_bytes("Content-Type", "application/json")
        .unwrap_or_else(|_| unreachable!("static header is valid"));
    let _ = request.respond(
        tiny_http::Response::from_string(json)
            .with_status_code(status)
            .with_header(header)
            // Always send a Content-Length body, never chunked.
            .with_chunked_threshold(usize::MAX),
    );
}

fn serve<F: Fn(Request) -> Response>(mut request: tiny_http::Request, token: &str, handler: &F) {
    if request.method() != &tiny_http::Method::Post || request.url() != "/rpc" {
        return respond(request, 404, &Response::error("not found"));
    }
    let expected = format!("Bearer {token}");
    let authorized = request
        .headers()
        .iter()
        .any(|h| h.field.equiv("Authorization") && h.value.as_str() == expected);
    if !authorized {
        return respond(request, 401, &Response::error("missing or wrong token"));
    }
    let mut body = String::new();
    if request
        .as_reader()
        .take(MAX_BODY_BYTES)
        .read_to_string(&mut body)
        .is_err()
    {
        return respond(request, 400, &Response::error("could not read request"));
    }
    match serde_json::from_str::<Request>(&body) {
        Ok(parsed) => {
            let reply = handler(parsed);
            respond(request, 200, &reply);
        }
        Err(e) => respond(request, 400, &Response::error(format!("bad request: {e}"))),
    }
}
