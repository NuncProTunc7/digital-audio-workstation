//! Connecting AI assistants to the bridge (`npt-mcp`): one-click setup for
//! Claude Desktop and Codex, the commands for Claude Code and Codex, and a
//! standard `mcpServers` snippet for any other MCP app (LM Studio, Cursor,
//! Cline, goose...).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The server name AI apps show (tools appear as `nunc-pro-tune` tools).
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

/// Bridge flag that offers only the core tools, for small local models
/// that lose their way in a long tool list.
pub const CORE_FLAG: &str = "--core";

/// The one-line Codex CLI setup command.
pub fn codex_command(bridge: &Path) -> String {
    format!("codex mcp add {SERVER_NAME} -- \"{}\"", bridge.display())
}

/// Where Codex keeps its settings: `$CODEX_HOME/config.toml`, by default
/// `~/.codex/config.toml`.
pub fn codex_config_path() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".codex")))
        .unwrap_or_else(std::env::temp_dir)
        .join("config.toml")
}

const CODEX_TABLE: &str = "[mcp_servers.nunc-pro-tune]";

/// Our table for Codex's `config.toml`. A TOML literal string ('...')
/// keeps Windows backslashes as they are (it can't hold a quote, and paths
/// don't have one).
fn codex_table(bridge: &Path) -> String {
    format!(
        "{CODEX_TABLE}\ncommand = '{}'\n",
        bridge.display().to_string().replace('\'', "")
    )
}

/// `existing` (a Codex config) with our table added or replaced, every
/// other line kept.
pub fn merge_codex_config(existing: &str, bridge: &Path) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in existing.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            inside = t == CODEX_TABLE || t.starts_with("[mcp_servers.nunc-pro-tune.");
        }
        if !inside {
            kept.push(line);
        }
    }
    let mut out = kept.join("\n").trim_end().to_owned();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(&codex_table(bridge));
    out
}

/// True when Codex's config already points at this bridge.
pub fn codex_configured(config_path: &Path, bridge: &Path) -> bool {
    std::fs::read_to_string(config_path).is_ok_and(|s| s.contains(&codex_table(bridge)))
}

/// Writes our server into Codex's config, backing up the original once as
/// `config.toml.before-nunc-pro-tune`.
pub fn install_codex(config_path: &Path, bridge: &Path) -> Result<(), String> {
    let existing = std::fs::read_to_string(config_path).unwrap_or_default();
    if let Some(dir) = config_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let backup = config_path.with_extension("toml.before-nunc-pro-tune");
    if !existing.is_empty() && !backup.exists() {
        std::fs::write(&backup, &existing).map_err(|e| e.to_string())?;
    }
    std::fs::write(config_path, merge_codex_config(&existing, bridge)).map_err(|e| e.to_string())
}

/// The standard `mcpServers` JSON most MCP apps accept (LM Studio's
/// mcp.json, Cursor, Cline, Windsurf...). `core` adds [`CORE_FLAG`].
pub fn generic_config(bridge: &Path, core: bool) -> String {
    let args: Vec<&str> = if core { vec![CORE_FLAG] } else { Vec::new() };
    let v = json!({
        "mcpServers": {
            SERVER_NAME: { "command": bridge.display().to_string(), "args": args }
        }
    });
    serde_json::to_string_pretty(&v).unwrap_or_default()
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
    fn codex_config_gets_our_table_once_and_keeps_the_rest() {
        let bridge = Path::new(r"C:\Program Files\Nunc Pro Tune\npt-mcp.exe");
        let existing = "model = \"gpt-5\"\n\n[mcp_servers.other]\ncommand = \"x\"\n";
        let merged = merge_codex_config(existing, bridge);
        assert!(merged.starts_with("model = \"gpt-5\""));
        assert!(merged.contains("[mcp_servers.other]\ncommand = \"x\""));
        assert!(merged.contains(
            r"[mcp_servers.nunc-pro-tune]
command = 'C:\Program Files\Nunc Pro Tune\npt-mcp.exe'"
        ));
        // Setting up again (say, after the app moved) replaces our table.
        let moved = merge_codex_config(&merged, Path::new("/new/npt-mcp"));
        assert_eq!(moved.matches(CODEX_TABLE).count(), 1);
        assert!(moved.contains("command = '/new/npt-mcp'"));
        assert!(moved.contains("[mcp_servers.other]"));
        assert_eq!(
            merge_codex_config("", bridge).matches(CODEX_TABLE).count(),
            1
        );
    }

    #[test]
    fn codex_install_is_detected() {
        let dir = tempfile::tempdir().expect("dir");
        let config = dir.path().join("config.toml");
        let bridge = Path::new("/opt/npt/npt-mcp");
        assert!(!codex_configured(&config, bridge));
        install_codex(&config, bridge).expect("install");
        assert!(codex_configured(&config, bridge));
    }

    #[test]
    fn other_apps_get_standard_mcp_servers_json() {
        let v: Value =
            serde_json::from_str(&generic_config(Path::new("/npt"), true)).expect("json");
        assert_eq!(v["mcpServers"]["nunc-pro-tune"]["command"], "/npt");
        assert_eq!(v["mcpServers"]["nunc-pro-tune"]["args"][0], CORE_FLAG);
        assert!(codex_command(Path::new("/npt")).starts_with("codex mcp add nunc-pro-tune -- "));
    }

    #[test]
    fn claude_code_command_quotes_the_path() {
        let cmd = claude_code_command(Path::new(r"C:\Program Files\Nunc Pro Tune\npt-mcp.exe"));
        assert!(cmd.starts_with("claude mcp add --scope user nunc-pro-tune -- \""));
        assert!(cmd.ends_with("npt-mcp.exe\""));
    }
}
