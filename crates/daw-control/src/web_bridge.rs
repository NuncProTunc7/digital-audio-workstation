//! Runs the bridge in web mode (`npt-mcp --http`) for AI apps that take a
//! web address: starts it with a fresh secret key, learns its port, and
//! stops it when dropped.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde::Serialize;

/// How long to wait for the bridge to say where it's listening.
const START_TIMEOUT: Duration = Duration::from_secs(10);

/// Where an AI app should connect.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WebAddress {
    /// `http://127.0.0.1:<port>/mcp`
    pub url: String,
    /// The secret key (send as `Authorization: Bearer <key>`).
    pub key: String,
    /// The address with the key in it, for apps that only take an address.
    pub url_with_key: String,
    /// Offers the short tool list for small models.
    pub core: bool,
}

/// The running bridge. Dropping it stops it.
pub struct WebBridge {
    child: Child,
    address: WebAddress,
}

impl WebBridge {
    /// Starts `bridge --http` (with `--core` for the short tool list) on a
    /// free port with a new key.
    pub fn start(bridge: &Path, core: bool) -> Result<WebBridge, String> {
        let key = new_key()?;
        let mut command = Command::new(bridge);
        command.args(["--http", "--port", "0"]);
        if core {
            command.arg(crate::ai_setup::CORE_FLAG);
        }
        command
            .env("NPT_MCP_KEY", &key)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // No console window flashing up.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("couldn't start the AI bridge ({}): {e}", bridge.display()))?;
        let stdout = child.stdout.take().ok_or("the AI bridge has no output")?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut lines = BufReader::new(stdout).lines();
            if let Some(Ok(first)) = lines.next() {
                let _ = tx.send(first);
            }
            // Keep reading so the bridge never blocks on a full pipe.
            for _ in lines {}
        });
        let line = match rx.recv_timeout(START_TIMEOUT) {
            Ok(line) => line,
            Err(_) => {
                let _ = child.kill();
                let mut why = String::new();
                if let Some(mut err) = child.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut err, &mut why);
                }
                return Err(format!(
                    "the AI bridge didn't start its web address{}",
                    if why.trim().is_empty() {
                        String::new()
                    } else {
                        format!(": {}", why.trim())
                    }
                ));
            }
        };
        let url = line
            .split_whitespace()
            .find(|w| w.starts_with("http://"))
            .ok_or_else(|| format!("unexpected answer from the AI bridge: {line}"))?
            .to_owned();
        Ok(WebBridge {
            child,
            address: WebAddress {
                url_with_key: format!("{url}?key={key}"),
                url,
                key,
                core,
            },
        })
    }

    pub fn address(&self) -> &WebAddress {
        &self.address
    }

    /// Whether the bridge is still running.
    pub fn running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for WebBridge {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 32 random hex characters.
fn new_key() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| format!("couldn't make a secret key: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_long_and_different() {
        let a = new_key().expect("key");
        let b = new_key().expect("key");
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }

    #[test]
    fn a_missing_bridge_is_a_plain_error() {
        let err = WebBridge::start(Path::new("definitely-not-here/npt-mcp"), false)
            .err()
            .expect("error");
        assert!(err.contains("couldn't start the AI bridge"), "{err}");
    }
}
