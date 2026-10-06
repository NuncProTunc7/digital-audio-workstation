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
    #[error("unexpected reply from Nunc Pro Tune: {0}")]
    Protocol(String),
}

/// Talks to a running app.
#[derive(Debug, Clone)]
pub struct ControlClient {
    port: u16,
    token: String,
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

    pub fn new(port: u16, token: String) -> Self {
        Self { port, token }
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
            .map_err(|e| ClientError::Unreachable(e.to_string()))?;
        stream
            .set_read_timeout(Some(READ_TIMEOUT))
            .map_err(|e| ClientError::Unreachable(e.to_string()))?;
        let head = format!(
            "POST /rpc HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
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

    #[test]
    fn dechunks_bodies() {
        let body = "5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        assert_eq!(dechunk(body).expect("ok"), "hello world");
        assert!(dechunk("zz\r\n").is_err());
    }
}
