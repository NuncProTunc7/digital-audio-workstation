//! Connecting Claude Desktop and Claude Code to the bridge (`npt-mcp`).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The server name Claude shows (tools appear as `nunc-pro-tune` tools).
pub const SERVER_NAME: &str = "nunc-pro-tune";
const CONFIG_FILE: &str = "claude_desktop_config.json";

/// Where Claude Desktop keeps its settings. The standard install uses
/// `%APPDATA%\Claude`; the Microsoft Store install keeps it inside its
/// package folder. Prefers a folder that already has a config file.
pub fn desktop_config_path() -> PathBuf {
    let standard = dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Claude");
    let mut candidates = vec![standard.clone()];
    if let Some(packages) = dirs::data_local_dir().map(|d| d.join("Packages"))
        && let Ok(entries) = std::fs::read_dir(packages)
    {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with("Claude_") {
                candidates.push(e.path().join("LocalCache").join("Roaming").join("Claude"));
            }
        }
    }
    candidates
        .iter()
        .find(|d| d.join(CONFIG_FILE).exists())
        .or_else(|| candidates.iter().find(|d| d.is_dir()))
        .unwrap_or(&standard)
        .join(CONFIG_FILE)
}

/// Adds (or updates) our server in a Claude Desktop config, keeping every
/// other setting. `existing` may be empty.
pub fn merge_desktop_config(existing: &str, bridge: &Path) -> Result<String, String> {
    let mut config: Value = if existing.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(existing).map_err(|e| {
            format!("Claude Desktop's settings file isn't valid JSON ({e}); fix or remove it first")
        })?
    };
    let root = config
        .as_object_mut()
        .ok_or("Claude Desktop's settings file isn't a JSON object")?;
    let servers = root
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("\"mcpServers\" in Claude Desktop's settings isn't an object")?;
    servers.insert(
        SERVER_NAME.into(),
        json!({ "command": bridge.display().to_string(), "args": [] }),
    );
    serde_json::to_string_pretty(&config)
        .map(|s| s + "\n")
        .map_err(|e| e.to_string())
}

/// True when the config already points at this bridge.
pub fn desktop_configured(config_path: &Path, bridge: &Path) -> bool {
    std::fs::read_to_string(config_path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| {
            v.pointer(&format!("/mcpServers/{SERVER_NAME}/command"))
                .and_then(Value::as_str)
                .map(|c| Path::new(c) == bridge)
        })
        .unwrap_or(false)
}

/// Writes our server into Claude Desktop's config, backing up the original
/// once as `claude_desktop_config.json.before-nunc-pro-tune`.
pub fn install_desktop(config_path: &Path, bridge: &Path) -> Result<(), String> {
    let existing = std::fs::read_to_string(config_path).unwrap_or_default();
    let merged = merge_desktop_config(&existing, bridge)?;
    if let Some(dir) = config_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let backup = config_path.with_extension("json.before-nunc-pro-tune");
    if !existing.is_empty() && !backup.exists() {
        std::fs::write(&backup, &existing).map_err(|e| e.to_string())?;
    }
    std::fs::write(config_path, merged).map_err(|e| e.to_string())
}

/// The one-line Claude Code setup command (available in every folder).
pub fn claude_code_command(bridge: &Path) -> String {
    format!(
        "claude mcp add --scope user {SERVER_NAME} -- \"{}\"",
        bridge.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_keeps_other_settings_and_servers() {
        let existing = r#"{ "theme": "dark", "mcpServers": { "other": { "command": "x" } } }"#;
        let merged = merge_desktop_config(existing, Path::new("/apps/npt-mcp")).expect("ok");
        let v: Value = serde_json::from_str(&merged).expect("json");
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["mcpServers"]["nunc-pro-tune"]["command"], "/apps/npt-mcp");
    }

    #[test]
    fn merge_creates_a_config_from_nothing_and_rejects_garbage() {
        let merged = merge_desktop_config("", Path::new("/npt")).expect("ok");
        assert!(merged.contains("nunc-pro-tune"));
        assert!(merge_desktop_config("{oops", Path::new("/npt")).is_err());
        assert!(merge_desktop_config("[]", Path::new("/npt")).is_err());
    }

    #[test]
    fn install_backs_up_and_is_detected() {
        let dir = std::env::temp_dir().join(format!("npt-claude-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let config = dir.join(CONFIG_FILE);
        std::fs::write(&config, r#"{"keep": 1}"#).expect("write");
        let bridge = Path::new("/opt/npt/npt-mcp");
        assert!(!desktop_configured(&config, bridge));
        install_desktop(&config, bridge).expect("install");
        assert!(desktop_configured(&config, bridge));
        let backup =
            std::fs::read_to_string(dir.join("claude_desktop_config.json.before-nunc-pro-tune"))
                .expect("backup");
        assert_eq!(backup, r#"{"keep": 1}"#);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn claude_code_command_quotes_the_path() {
        let cmd = claude_code_command(Path::new(r"C:\Program Files\Nunc Pro Tune\npt-mcp.exe"));
        assert!(cmd.starts_with("claude mcp add --scope user nunc-pro-tune -- \""));
        assert!(cmd.ends_with("npt-mcp.exe\""));
    }
}
