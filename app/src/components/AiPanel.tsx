import { useState } from "react";
import type { AiStatus } from "../types";

interface AiPanelProps {
  status: AiStatus | null;
  onInstallClaudeDesktop: () => void;
  onInstallCodex: () => void;
  /** Turns the web address on or off (`core` = short tool list). */
  onWeb: (on: boolean, core: boolean) => void;
  onClose: () => void;
  onCopyReport: () => void;
}

/** The AI apps the panel explains, in menu order. */
const APPS = [
  { id: "claude-desktop", name: "Claude Desktop" },
  { id: "claude-code", name: "Claude Code" },
  { id: "codex", name: "Codex (OpenAI)" },
  { id: "other", name: "Another AI app (LM Studio, Cursor, Cline, goose…)" },
  { id: "local", name: "A small model on this computer" },
  { id: "web", name: "An app that takes a web address (Open WebUI…)" },
] as const;
type AppId = (typeof APPS)[number]["id"];

const APP_KEY = "npt.aiApp";

function loadApp(): AppId {
  try {
    const saved = window.localStorage.getItem(APP_KEY);
    return APPS.find((a) => a.id === saved)?.id ?? "claude-desktop";
  } catch {
    return "claude-desktop";
  }
}

function saveApp(id: AppId) {
  try {
    window.localStorage.setItem(APP_KEY, id);
  } catch {
    // Remembering the choice is a convenience only.
  }
}

/** Things to try first; each exercises a different group of tools. */
const EXAMPLE_PROMPTS = [
  "Make a 16-bar chiptune battle loop at 150 BPM with lead, bass and drums.",
  "Listen to my mix and tell me what to fix.",
  "Make the bass punchier and add some reverb to the keys.",
  "Turn the second clip into a calmer version for the village theme.",
];

/** "Codex: set tempo" → who "Codex", what "Set tempo"; "Claude saved the project" stays whole. */
function splitActivity(description: string): { who: string | null; what: string } {
  const m = /^([^:]{1,32}): (.*)$/.exec(description);
  const what = m ? m[2] : description;
  return { who: m ? m[1] : null, what: what.charAt(0).toUpperCase() + what.slice(1) };
}

function ago(atMs: number): string {
  const secs = Math.max(0, Math.round((Date.now() - atMs) / 1000));
  if (secs < 60) return `${secs}s ago`;
  if (secs < 3600) return `${Math.round(secs / 60)} min ago`;
  return `${Math.round(secs / 3600)} h ago`;
}

/** A command or settings text with a Copy button. */
function Copyable({ label, text, block = false }: { label: string; text: string; block?: boolean }) {
  const [copied, setCopied] = useState(false);
  const copy = () => {
    void navigator.clipboard
      ?.writeText(text)
      .then(() => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1500);
      })
      .catch(() => {});
  };
  return (
    <div className={block ? "ai-command block" : "ai-command"}>
      {block ? <pre aria-label={label}>{text}</pre> : <code aria-label={label}>{text}</code>}
      <button className="small" onClick={copy} aria-label={`Copy ${label}`}>
        {copied ? "Copied" : "Copy"}
      </button>
    </div>
  );
}

/**
 * How to connect an AI (Claude, Codex, or any app that speaks MCP), and
 * what the AI has done this session.
 */
export default function AiPanel({ status, onInstallClaudeDesktop, onInstallCodex, onWeb, onClose, onCopyReport }: AiPanelProps) {
  const [app, setApp] = useState<AppId>(loadApp);
  const web = status?.web ?? null;
  const [webCore, setWebCore] = useState(false);
  const missing = status && !status.bridge_found && (
    <p className="muted">The AI bridge program wasn't found next to the app ({status.bridge_path}).</p>
  );

  return (
    <div className="ai-panel" role="dialog" aria-label="Your AI">
      <div className="ai-panel-header">
        <strong>Your AI</strong>
        <span className={status?.listening ? "ai-state ok" : "ai-state off"}>
          {status?.listening
            ? status.last_ai
              ? `Connected: ${status.last_ai}`
              : "Ready: an AI can connect"
            : (status?.error ?? "Not available")}
        </span>
        <span className="spacer" />
        <button className="small" onClick={onClose} aria-label="Close AI panel">
          ✕
        </button>
      </div>

      <p className="muted">
        Any AI app that supports MCP (the open standard for AI tools) can work on your songs: Claude, Codex, or a model
        running on your own computer.
      </p>

      <label className="field ai-app">
        <span>Which AI app?</span>
        <select
          aria-label="Which AI app"
          value={app}
          onChange={(e) => {
            const id = e.target.value as AppId;
            setApp(id);
            saveApp(id);
          }}
        >
          {APPS.map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </select>
      </label>

      {app === "claude-desktop" && (
        <section>
          {status?.desktop_configured ? (
            <p>
              ✓ Set up. Restart Claude Desktop if you just did this, then look for <em>nunc-pro-tune</em> in its tools menu.
            </p>
          ) : (
            <>
              <p>One click adds Nunc Pro Tune to Claude Desktop's settings (your old settings are backed up).</p>
              <button onClick={onInstallClaudeDesktop} disabled={!status?.bridge_found}>
                Set up Claude Desktop
              </button>
              {missing}
            </>
          )}
        </section>
      )}

      {app === "claude-code" && (
        <section>
          <p>Run this once in a terminal:</p>
          <Copyable label="Claude Code command" text={status?.claude_code_command ?? ""} />
        </section>
      )}

      {app === "codex" && (
        <section>
          {status?.codex_configured ? (
            <p>✓ Set up. Start a new Codex session; it lists nunc-pro-tune under its MCP tools (/mcp).</p>
          ) : (
            <>
              <p>One click adds Nunc Pro Tune to Codex's settings (your old settings are backed up):</p>
              <button onClick={onInstallCodex} disabled={!status?.bridge_found}>
                Set up Codex
              </button>
              {missing}
            </>
          )}
          <p>Or run this once in a terminal:</p>
          <Copyable label="Codex command" text={status?.codex_command ?? ""} />
        </section>
      )}

      {app === "other" && (
        <section>
          <p>
            In the app's MCP or tools settings, add a server that runs this program (no arguments):{" "}
          </p>
          <Copyable label="Bridge program" text={status?.bridge_path ?? ""} />
          <p>
            Apps that take a settings file (LM Studio's <em>mcp.json</em>, Cursor, Cline, Windsurf) want this; paste it
            in, keeping any servers already there:
          </p>
          <Copyable label="MCP settings" text={status?.generic_config ?? ""} block />
        </section>
      )}

      {app === "web" && (
        <section>
          <p>
            Some AI apps on your computer (Open WebUI, some local-model chat apps) connect with a web address instead of
            a program. Turn this on, then paste the address and key into the app's MCP or tools settings.
          </p>
          <label className="ai-check">
            <input
              type="checkbox"
              aria-label="Let AI apps connect with a web address"
              checked={web !== null}
              disabled={!status?.bridge_found}
              onChange={(e) => onWeb(e.target.checked, web?.core ?? webCore)}
            />
            Let AI apps connect with a web address
          </label>
          <label className="ai-check">
            <input
              type="checkbox"
              aria-label="Short tool list for small models"
              checked={web?.core ?? webCore}
              disabled={web !== null}
              onChange={(e) => setWebCore(e.target.checked)}
            />
            Short tool list for small models
          </label>
          {web && (
            <>
              <p>Address (type: Streamable HTTP):</p>
              <Copyable label="Web address" text={web.url} />
              <p>Secret key (as a Bearer token, or "API key"):</p>
              <Copyable label="Secret key" text={web.key} />
              <p>Or, for apps that only take an address, this one has the key in it:</p>
              <Copyable label="Address with key" text={web.url_with_key} />
              <p className="muted">
                Only apps on this computer can use it, and only with the key. Turning it off and on again makes a new
                key.
              </p>
            </>
          )}
          {missing}
        </section>
      )}

      {app === "local" && (
        <section>
          <p>
            Small models on your own computer (through LM Studio, goose, or Open WebUI with Ollama) do best with a short
            list of tools. This setting offers the essentials (write notes, pick sounds, mix, play, save) and sends
            text instead of pictures:
          </p>
          <Copyable label="MCP settings for small models" text={status?.generic_config_core ?? ""} block />
          <p className="muted">
            Pick a model that supports tool use (also called function calling). Bigger models write better music; if
            yours gets confused, ask for one small change at a time.
          </p>
        </section>
      )}

      <section>
        <h3>Try asking</h3>
        <ul className="ai-examples">
          {EXAMPLE_PROMPTS.map((p) => (
            <li key={p}>“{p}”</li>
          ))}
        </ul>
        <p className="muted">Keep this app open while your AI works. Everything it does can be undone with Ctrl+Z.</p>
      </section>

      <section>
        <h3>Something wrong?</h3>
        <p>
          Copy a report about your sound card, CPU load and recent errors, and paste it to your AI with what you heard.
          It can also fetch it itself (<em>diagnostic_report</em>).
        </p>
        <button onClick={onCopyReport}>Copy diagnostic report</button>
      </section>

      <section>
        <h3>What your AI did</h3>
        {status && status.activity.length > 0 ? (
          <ul className="ai-activity" aria-label="AI activity">
            {status.activity.map((a) => {
              const { who, what } = splitActivity(a.description);
              return (
                <li key={`${a.at_ms}-${a.description}`}>
                  <span>
                    {who && <span className="ai-who">{who}</span>}
                    {what}
                  </span>
                  <span className="muted">{ago(a.at_ms)}</span>
                </li>
              );
            })}
          </ul>
        ) : (
          <p className="muted">Nothing yet this session.</p>
        )}
      </section>
    </div>
  );
}
