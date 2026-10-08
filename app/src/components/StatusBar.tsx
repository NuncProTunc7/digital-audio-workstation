import type { AppInfo, AudioStatus, ClaudeStatus, TransportStatus } from "../types";

interface StatusBarProps {
  audio: AudioStatus | null;
  transport: TransportStatus | null;
  info: AppInfo | null;
  preview: boolean;
  error: string | null;
  filePath: string | null;
  onDevice: (name: string | null) => void;
  onBufferSize: (frames: number | null) => void;
  onCopyReport: () => void;
  onRefreshMidi: () => void;
  claude: ClaudeStatus | null;
  onToggleClaude: () => void;
}

/** Claude counts as "working" for this long after its last request. */
const CLAUDE_ACTIVE_SECS = 10;

const DEFAULT_DEVICE = "__default__";
const DEFAULT_BUFFER = "default";

function Meter({ level }: { level: number }) {
  // Map -60..0 dBFS to 0..100%.
  const db = level > 0 ? 20 * Math.log10(level) : -100;
  const pct = Math.max(0, Math.min(100, ((db + 60) / 60) * 100));
  const hot = db > -3;
  return (
    <div className="meter" aria-hidden>
      <div className={hot ? "meter-fill hot" : "meter-fill"} style={{ width: `${pct}%` }} />
    </div>
  );
}

export default function StatusBar({
  audio,
  transport,
  info,
  preview,
  error,
  filePath,
  onDevice,
  onBufferSize,
  onCopyReport,
  onRefreshMidi,
  claude,
  onToggleClaude,
}: StatusBarProps) {
  const struggling = transport?.struggling ?? false;
  const claudeActive = claude?.last_activity_secs != null && claude.last_activity_secs < CLAUDE_ACTIVE_SECS;
  const claudeDot = !claude?.listening ? "off" : claudeActive ? "active" : "ok";
  const latencyMs =
    transport?.buffer_frames && audio?.sample_rate_hz
      ? (transport.buffer_frames / audio.sample_rate_hz) * 1000
      : null;
  const cpu = transport ? Math.round(transport.cpu_load * 100) : 0;
  const midi = audio?.midi_inputs ?? [];
  const rate = audio?.sample_rate_hz ?? null;
  const msFor = (frames: number) => (rate ? ` (${((frames / rate) * 1000).toFixed(1)} ms)` : "");
  const options = audio?.buffer_options ?? [];
  // The next size up, offered when the computer can't keep up.
  const current = audio?.buffer_active ?? transport?.buffer_frames ?? 0;
  const bigger = options.find((f) => f > current) ?? null;

  return (
    <footer className="status-bar">
      <label className="status-item" title="Audio output device">
        🔈
        <select
          aria-label="Audio output device"
          value={audio?.active_output ?? DEFAULT_DEVICE}
          onChange={(e) => {
            onDevice(e.target.value === DEFAULT_DEVICE ? null : e.target.value);
            e.currentTarget.blur();
          }}
        >
          <option value={DEFAULT_DEVICE}>System default{audio?.default_output ? ` (${audio.default_output})` : ""}</option>
          {(audio?.output_devices ?? []).map((d) => (
            <option key={d} value={d}>
              {d}
            </option>
          ))}
        </select>
      </label>
      {audio?.sample_rate_hz && (
        <span className="status-item muted" title="Sample rate and sound card buffer (output latency)">
          {(audio.sample_rate_hz / 1000).toFixed(1)} kHz{latencyMs ? ` · ${latencyMs.toFixed(1)} ms buffer` : ""}
        </span>
      )}
      {audio?.sample_rate_hz && options.length > 0 && (
        <label
          className="status-item"
          title="Sound card buffer. Smaller answers faster when you play; bigger stops crackles on a busy computer."
        >
          <span className="muted">Buffer</span>
          <select
            aria-label="Sound card buffer"
            value={audio.buffer_setting ?? DEFAULT_BUFFER}
            onChange={(e) => {
              onBufferSize(e.target.value === DEFAULT_BUFFER ? null : Number(e.target.value));
              e.currentTarget.blur();
            }}
          >
            <option value={DEFAULT_BUFFER}>Default</option>
            {options.map((f) => (
              <option key={f} value={f}>
                {f}
                {msFor(f)}
              </option>
            ))}
          </select>
        </label>
      )}
      <span className={cpu > 70 ? "status-item error" : "status-item muted"} title="Audio CPU load">
        CPU {cpu}%
      </span>
      {struggling && bigger !== null && (
        <button
          className="small status-hint"
          onClick={() => onBufferSize(bigger)}
          title="The computer is struggling to keep up, which you hear as crackles. A bigger buffer gives it more time."
        >
          Crackling? Use buffer {bigger}
        </button>
      )}
      <span className="status-item meters" title="Output level">
        <Meter level={transport?.peak_left ?? 0} />
        <Meter level={transport?.peak_right ?? 0} />
      </span>
      <span className="status-item muted" title={midi.join(", ")}>
        🎹 {midi.length > 0 ? midi.join(", ") : "No MIDI keyboard"}
        <button className="small" onClick={onRefreshMidi} title="Look for MIDI keyboards again">
          ↻
        </button>
      </span>
      {preview && <span className="status-item preview">Browser preview: no audio engine</span>}
      {(error ?? audio?.error) && (
        <span className="status-item error" role="alert">
          {error ?? audio?.error}
        </span>
      )}
      <span className="spacer" />
      <button
        className="small"
        onClick={onCopyReport}
        title="Copy a report about your sound card, CPU and recent errors, to paste to Claude when something goes wrong"
        aria-label="Copy diagnostic report"
      >
        Report
      </button>
      <button
        className="small claude-button"
        onClick={onToggleClaude}
        title={claude?.listening ? "Claude can connect. Click for setup and activity." : (claude?.error ?? "Claude")}
      >
        <span className={`claude-dot ${claudeDot}`} aria-hidden />
        Claude
      </button>
      <span className="status-item muted" title={filePath ?? "Not saved yet"}>
        {filePath ? filePath.split(/[\\/]/).pop() : "Not saved"}
      </span>
      {info && (
        <span className="status-item muted">
          v{info.version} · {info.license}
        </span>
      )}
    </footer>
  );
}
