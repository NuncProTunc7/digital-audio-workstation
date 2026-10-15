import { useState } from "react";
import type { ClaudeStatus } from "../types";

interface ClaudePanelProps {
  status: ClaudeStatus | null;
  onInstallDesktop: () => void;
  onClose: () => void;
  onCopyReport: () => void;
}

/** Things to try first; each exercises a different group of tools. */
const EXAMPLE_PROMPTS = [
  "Make a 16-bar chiptune battle loop at 150 BPM with lead, bass and drums.",
  "Listen to my mix and tell me what to fix.",
  "Make the bass punchier and add some reverb to the keys.",
  "Turn the second clip into a calmer version for the village theme.",
];

/** "Claude: set tempo" → "Set tempo"; the panel is already titled Claude. */
function label(description: string): string {
  const text = description.replace(/^Claude:? /, "");
  return text.charAt(0).toUpperCase() + text.slice(1);
}

function ago(atMs: number): string {
  const secs = Math.max(0, Math.round((Date.now() - atMs) / 1000));
  if (secs < 60) return `${secs}s ago`;
  if (secs < 3600) return `${Math.round(secs / 60)} min ago`;
  return `${Math.round(secs / 3600)} h ago`;
}

/** How to connect Claude, and what Claude has done this session. */
export default function ClaudePanel({ status, onInstallDesktop, onClose, onCopyReport }: ClaudePanelProps) {
  const [copied, setCopied] = useState(false);

  const copy = (text: string) => {
    void navigator.clipboard
      ?.writeText(text)
      .then(() => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1500);
      })
      .catch(() => {});
  };

  return (
    <div className="claude-panel" role="dialog" aria-label="Claude">
      <div className="claude-panel-header">
        <strong>Claude</strong>
        <span className={status?.listening ? "claude-state ok" : "claude-state off"}>
          {status?.listening ? "Ready: Claude can connect" : (status?.error ?? "Not available")}
        </span>
        <span className="spacer" />
        <button className="small" onClick={onClose} aria-label="Close Claude panel">
          ✕
        </button>
      </div>

      <section>
        <h3>Claude Desktop</h3>
        {status?.desktop_configured ? (
          <p>
            ✓ Set up. Restart Claude Desktop if you just did this, then look for <em>nunc-pro-tune</em> in its tools menu.
          </p>
        ) : (
          <>
            <p>One click adds Nunc Pro Tune to Claude Desktop's settings (your old settings are backed up).</p>
            <button onClick={onInstallDesktop} disabled={!status?.bridge_found}>
              Set up Claude Desktop
            </button>
            {status && !status.bridge_found && (
              <p className="muted">The Claude bridge program wasn't found next to the app ({status.bridge_path}).</p>
            )}
          </>
        )}
      </section>

      <section>
        <h3>Claude Code</h3>
        <p>Run this once in a terminal:</p>
        <div className="claude-command">
          <code>{status?.claude_code_command ?? ""}</code>
          <button className="small" onClick={() => copy(status?.claude_code_command ?? "")}>
            {copied ? "Copied" : "Copy"}
          </button>
        </div>
      </section>

      <section>
        <h3>Try asking</h3>
        <ul className="claude-examples">
          {EXAMPLE_PROMPTS.map((p) => (
            <li key={p}>“{p}”</li>
          ))}
        </ul>
        <p className="muted">Keep this app open while Claude works. Everything Claude does can be undone with Ctrl+Z.</p>
      </section>

      <section>
        <h3>Something wrong?</h3>
        <p>
          Copy a report about your sound card, CPU load and recent errors, and paste it to Claude with what you heard.
          Claude can also fetch it itself (<em>diagnostic_report</em>).
        </p>
        <button onClick={onCopyReport}>Copy diagnostic report</button>
      </section>

      <section>
        <h3>What Claude did</h3>
        {status && status.activity.length > 0 ? (
          <ul className="claude-activity" aria-label="Claude activity">
            {status.activity.map((a) => (
              <li key={`${a.at_ms}-${a.description}`}>
                <span>{label(a.description)}</span>
                <span className="muted">{ago(a.at_ms)}</span>
              </li>
            ))}
          </ul>
        ) : (
          <p className="muted">Nothing yet this session.</p>
        )}
      </section>
    </div>
  );
}
