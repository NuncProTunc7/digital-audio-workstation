use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

use thiserror::Error;

use crate::discovery::{ControlFile, control_file_path};
use crate::protocol::{Request, Response};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// Analysis and export can take a while on long songs.
const READ_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("Nunc Pro Tune isn't running. Open the app, then try again. (No control file at {0}.)")]
    NotRunning(String),
    #[error("Nunc Pro Tune isn't responding. If it was closed, open it again. ({0})")]
    Unreachable(String),
    /// Nothing listens on the port the discovery file names: the file is
    /// left over from an earlier run.
    #[error("Nunc Pro Tune isn't running (or was closed). Open the app, then try again. ({0})")]
    NoListener(String),
    #[error("unexpected reply from Nunc Pro Tune: {0}")]
    Protocol(String),
}

/// Talks to a running app.
#[derive(Debug, Clone)]
pub struct ControlClient {
    port: u16,
    token: String,
    /// The AI this client speaks for, told to the app with each request.
    ai: Option<String>,
}

impl ControlClient {
    /// Finds the running app through the default discovery file.
    pub fn discover() -> Result<Self, ClientError> {
        Self::from_file(&control_file_path())
    }

    pub fn from_file(path: &Path) -> Result<Self, ClientError> {
        let file = ControlFile::read(path)
            .map_err(|_| ClientError::NotRunning(path.display().to_string()))?;
        Ok(Self::new(file.port, file.token))
    }

    /// Finds the running app and sends it a request (see [`Self::call_via`]).
    pub fn call_discovered(
        request: &Request,
        ai: Option<&str>,
    ) -> Result<Result<serde_json::Value, String>, ClientError> {
        Self::call_via(&control_file_path(), request, ai)
    }

    /// Sends a request to the app named in the discovery file at `path`.
    ///
    /// When nothing listens on that file's port, the file is stale. It is
    /// removed and read again once: an app sandbox (Claude Desktop's
    /// packaged storage on Windows) can keep its own old copy that hides
    /// the app's fresh one, and removing the copy uncovers it.
    pub fn call_via(
        path: &Path,
        request: &Request,
        ai: Option<&str>,
    ) -> Result<Result<serde_json::Value, String>, ClientError> {
        let seen = std::fs::read(path).ok();
        let why = match Self::from_file(path)?.speaking_for(ai).call(request) {
            Err(ClientError::NoListener(why)) => why,
            other => return other,
        };
        // Only if it still holds what we read: the app may have just
        // started and written a fresh one.
        if std::fs::read(path).ok() == seen {
            let _ = std::fs::remove_file(path);
        }
        match std::fs::read(path).ok() {
            Some(now) if Some(&now) != seen.as_ref() => {
                Self::from_file(path)?.speaking_for(ai).call(request)
            }
            _ => Err(ClientError::NoListener(why)),
        }
    }

    pub fn new(port: u16, token: String) -> Self {
        Self {
            port,
            token,
            ai: None,
        }
    }

    /// Names the AI behind this client's requests (shown in the app).
    pub fn speaking_for(mut self, ai: Option<&str>) -> Self {
        self.ai = ai.map(crate::ai::clean);
        self
    }

    /// Sends a request and returns its result, or the app's error message.
    pub fn call(
        &self,
        request: &Request,
    ) -> Result<Result<serde_json::Value, String>, ClientError> {
        let body =
            serde_json::to_string(request).map_err(|e| ClientError::Protocol(e.to_string()))?;
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        let mut stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
            .map_err(|e| ClientError::NoListener(e.to_string()))?;
        stream
            .set_read_timeout(Some(READ_TIMEOUT))
            .map_err(|e| ClientError::Unreachable(e.to_string()))?;
        let ai = self
            .ai
            .as_deref()
            .map(|a| format!("{}: {a}\r\n", crate::ai::AI_HEADER))
            .unwrap_or_default();
        let head = format!(
            "POST /rpc HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\n{ai}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.token,
            body.len()
        );
        stream
            .write_all(head.as_bytes())
            .and_then(|()| stream.write_all(body.as_bytes()))
            .map_err(|e| ClientError::Unreachable(e.to_string()))?;
        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .map_err(|e| ClientError::Unreachable(e.to_string()))?;
        let text = String::from_utf8_lossy(&raw);
        let (head, body) = text
            .split_once("\r\n\r\n")
            .ok_or_else(|| ClientError::Protocol("no body".into()))?;
        let chunked = head.lines().any(|l| {
            l.to_ascii_lowercase().starts_with("transfer-encoding:")
                && l.to_ascii_lowercase().contains("chunked")
        });
        let json = if chunked {
            dechunk(body)?
        } else {
            body.to_owned()
        };
        let response: Response =
            serde_json::from_str(&json).map_err(|e| ClientError::Protocol(e.to_string()))?;
        Ok(response.into_result())
    }
}

/// Reassembles an HTTP chunked body.
fn dechunk(mut body: &str) -> Result<String, ClientError> {
    let mut out = String::new();
    loop {
        let (size_line, rest) = body
            .split_once("\r\n")
            .ok_or_else(|| ClientError::Protocol("bad chunk".into()))?;
        let size = usize::from_str_radix(size_line.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| ClientError::Protocol("bad chunk size".into()))?;
        if size == 0 {
            return Ok(out);
        }
        let chunk = rest
            .get(..size)
            .ok_or_else(|| ClientError::Protocol("short chunk".into()))?;
        out.push_str(chunk);
        body = rest.get(size + 2..).unwrap_or("");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A port nothing listens on.
    fn closed_port() -> u16 {
        let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        l.local_addr().expect("addr").port()
    }

    #[test]
    fn a_stale_control_file_is_cleared() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("control.json");
        ControlFile {
            port: closed_port(),
            token: "old".into(),
            pid: 1,
            version: "0.0.0".into(),
        }
        .write(&path)
        .expect("write");
        let err = ControlClient::call_via(&path, &Request::Ping, None).expect_err("stale");
        assert!(matches!(err, ClientError::NoListener(_)), "{err}");
        assert!(err.to_string().contains("isn't running"));
        assert!(!path.exists(), "the stale file is gone");
        // With no file at all, the usual "not running" message.
        let err = ControlClient::call_via(&path, &Request::Ping, None).expect_err("none");
        assert!(matches!(err, ClientError::NotRunning(_)));
    }

    #[test]
    fn dechunks_bodies() {
        let body = "5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        assert_eq!(dechunk(body).expect("ok"), "hello world");
        assert!(dechunk("zz\r\n").is_err());
    }
}
