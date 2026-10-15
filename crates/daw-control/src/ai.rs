//! Which AI is talking to the app. Any MCP-capable assistant can drive Nunc
//! Pro Tune through `npt-mcp`; the bridge passes on the name the assistant
//! gave when it connected, so the app can say "Codex: set tempo" or
//! "Before Claude's changes" rather than guessing.

/// HTTP header the bridge uses to name the AI behind a request.
pub const AI_HEADER: &str = "X-NPT-AI";

/// The name used when the AI didn't say who it is.
pub const UNKNOWN_AI: &str = "AI";

/// Longest name kept, so a long client name can't flood the activity list.
const MAX_NAME_CHARS: usize = 32;

/// Product names for MCP clients that report a technical id (lower-case
/// fragment of the client name, friendly name). First match wins.
const KNOWN: &[(&str, &str)] = &[
    ("claude", "Claude"),
    ("codex", "Codex"),
    ("chatgpt", "ChatGPT"),
    ("openai", "ChatGPT"),
    ("goose", "Goose"),
    ("lm studio", "LM Studio"),
    ("lmstudio", "LM Studio"),
    ("open webui", "Open WebUI"),
    ("open-webui", "Open WebUI"),
    ("openwebui", "Open WebUI"),
    ("cursor", "Cursor"),
    ("cline", "Cline"),
    ("continue", "Continue"),
    ("gemini", "Gemini"),
    ("copilot", "Copilot"),
    ("visual studio code", "Copilot"),
    ("vscode", "Copilot"),
    ("ollama", "Ollama"),
];

/// A short, friendly name for an MCP client from its `clientInfo` name and
/// optional title: known apps get their product name, others their own
/// (title first), cleaned up; [`UNKNOWN_AI`] when there is nothing usable.
pub fn friendly_name(client_name: &str, title: Option<&str>) -> String {
    let lower = format!("{} {}", client_name, title.unwrap_or("")).to_lowercase();
    if let Some((_, name)) = KNOWN.iter().find(|(id, _)| lower.contains(id)) {
        return (*name).to_owned();
    }
    let own = title
        .filter(|t| !t.trim().is_empty())
        .unwrap_or(client_name);
    clean(own)
}

/// A name safe to show and to send in a header: printable characters only,
/// trimmed and shortened; [`UNKNOWN_AI`] if nothing is left.
pub fn clean(name: &str) -> String {
    let kept: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_CHARS)
        .collect();
    let kept = kept.trim();
    if kept.is_empty() {
        UNKNOWN_AI.to_owned()
    } else {
        kept.to_owned()
    }
}

/// Name of the version saved before an AI starts changing the song:
/// "Before Claude's changes", or "Before AI changes" for an unnamed one.
pub fn before_changes(ai: &str) -> String {
    if ai == UNKNOWN_AI {
        "Before AI changes".to_owned()
    } else {
        format!("Before {ai}'s changes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_clients_get_their_product_name() {
        assert_eq!(friendly_name("claude-ai", None), "Claude");
        assert_eq!(friendly_name("claude-code", Some("Claude Code")), "Claude");
        assert_eq!(friendly_name("codex-mcp-client", None), "Codex");
        assert_eq!(friendly_name("goose", None), "Goose");
        assert_eq!(friendly_name("mcp-client", Some("LM Studio")), "LM Studio");
    }

    #[test]
    fn other_clients_keep_their_own_name_cleaned_up() {
        assert_eq!(friendly_name("my-agent", Some("  Music Bot ")), "Music Bot");
        assert_eq!(friendly_name("my-agent", None), "my-agent");
        assert_eq!(friendly_name("", None), UNKNOWN_AI);
        assert_eq!(clean("bad\r\nname"), "badname");
        assert_eq!(clean(&"x".repeat(100)).len(), 32);
    }

    #[test]
    fn versions_are_named_after_the_ai() {
        assert_eq!(before_changes("Codex"), "Before Codex's changes");
        assert_eq!(before_changes(UNKNOWN_AI), "Before AI changes");
    }
}
