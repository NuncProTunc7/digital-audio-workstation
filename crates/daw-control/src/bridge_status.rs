use std::io::ErrorKind;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

use crate::{ControlClient, Request, client::ClientError};

/// Version of the local app-to-bridge protocol.
pub const PROTOCOL_VERSION: u32 = 1;
const PROBE_TIMEOUT: Duration = Duration::from_secs(1);
const CAPABILITIES: &[&str] = &[
    "bridge_status",
    "audio_export",
    "audio_preview",
    "active_project_id",
    "save_project_copy",
    "typed_batch",
];

/// Safe to display in the AI panel or send to an MCP caller.
#[derive(Debug, Clone, Serialize)]
pub struct BridgeStatus {
    pub status: &'static str,
    pub failed_step: Option<&'static str>,
    pub message: &'static str,
    pub app_version: Option<String>,
    pub protocol_version: Option<u32>,
    pub expected_protocol_version: u32,
    pub capabilities: Vec<String>,
}

fn failure(status: &'static str, step: &'static str, message: &'static str) -> BridgeStatus {
    BridgeStatus {
        status,
        failed_step: Some(step),
        message,
        app_version: None,
        protocol_version: None,
        expected_protocol_version: PROTOCOL_VERSION,
        capabilities: Vec::new(),
    }
}

/// Diagnose a server that could not start, including control-file permissions.
pub fn startup_failure(kind: ErrorKind) -> BridgeStatus {
    if kind == ErrorKind::PermissionDenied {
        failure(
            "access_denied",
            "start_server",
            "Access denied while starting the AI connection. Check access to the control file folder and open Nunc Pro Tune again.",
        )
    } else {
        failure(
            "startup_failed",
            "start_server",
            "Nunc Pro Tune could not start its AI connection. Open the app again and check the diagnostic report.",
        )
    }
}

pub(crate) fn handshake() -> Value {
    json!({
        "app": "Nunc Pro Tune",
        "app_version": env!("CARGO_PKG_VERSION"),
        "protocol_version": PROTOCOL_VERSION,
        "capabilities": CAPABILITIES,
    })
}

/// Inspect discovery and make an authenticated, bounded handshake without changing the control file.
pub fn probe(path: &Path) -> BridgeStatus {
    let client = match ControlClient::from_file(path) {
        Ok(client) => client,
        Err(ClientError::NotRunning(_)) => {
            return failure(
                "control_file_missing",
                "read_control_file",
                "The AI control file is missing. Open Nunc Pro Tune or check its AI panel if it is already open.",
            );
        }
        Err(ClientError::FileAccessDenied(_)) => {
            return failure(
                "access_denied",
                "read_control_file",
                "Access denied while reading the AI control file. Check Nunc Pro Tune’s AI panel and launch both apps under the same Windows account.",
            );
        }
        Err(_) => {
            return failure(
                "control_file_unreadable",
                "read_control_file",
                "The AI control file could not be read. Check Nunc Pro Tune’s AI panel and open the app again.",
            );
        }
    };
    let reply = match client.call_with_timeout(&Request::BridgeHandshake, PROBE_TIMEOUT) {
        Ok(Ok(reply)) => reply,
        Ok(Err(_)) | Err(ClientError::Protocol(_)) => Value::Null,
        Err(ClientError::ConnectAccessDenied) => {
            return failure(
                "access_denied",
                "connect",
                "Windows denied the local AI connection. Check Nunc Pro Tune's AI panel and launch both apps under the same Windows account.",
            );
        }
        Err(ClientError::AccessDenied) => {
            return failure(
                "access_denied",
                "handshake",
                "Nunc Pro Tune refused authentication. Check its AI panel and restart the AI bridge.",
            );
        }
        Err(ClientError::NoListener(_)) => {
            return failure(
                "stale_control_file",
                "connect",
                "No server is listening at the address in the AI control file. Open or restart Nunc Pro Tune, then retry.",
            );
        }
        Err(ClientError::TimedOut(step)) => {
            return failure(
                "connection_timed_out",
                step,
                "The AI connection timed out. Check the AI panel and try again after the app finishes any export in progress.",
            );
        }
        Err(_) => {
            return failure(
                "connection_failed",
                "handshake",
                "The AI connection dropped. Check Nunc Pro Tune’s AI panel and try again.",
            );
        }
    };
    let protocol_version = reply["protocol_version"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok());
    let app_version = reply["app_version"].as_str().map(str::to_owned);
    let capabilities = serde_json::from_value::<Vec<String>>(reply["capabilities"].clone()).ok();
    if let (Some(app_version), Some(capabilities)) = (&app_version, capabilities)
        && reply["app"] == "Nunc Pro Tune"
        && protocol_version == Some(PROTOCOL_VERSION)
    {
        return BridgeStatus {
            status: "connected",
            failed_step: None,
            message: "The AI bridge can connect to Nunc Pro Tune.",
            app_version: Some(app_version.clone()),
            protocol_version,
            expected_protocol_version: PROTOCOL_VERSION,
            capabilities,
        };
    }
    let mut result = failure(
        "protocol_version_mismatch",
        "handshake",
        "The app and AI bridge could not agree on a protocol. Install the matching bridge for this version of Nunc Pro Tune and restart your AI app.",
    );
    // Only use the numeric version from an incompatible peer.
    result.protocol_version = protocol_version;
    result
}
