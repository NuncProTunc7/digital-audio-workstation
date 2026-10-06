import type { AppInfo, AudioStatus, TransportStatus } from "../types";

interface StatusBarProps {
  audio: AudioStatus | null;
  transport: TransportStatus | null;
  info: AppInfo | null;
  preview: boolean;
  error: string | null;
  onDevice: (name: string | null) => void;
  onRefreshMidi: () => void;
}

const DEFAULT_DEVICE = "__default__";

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

export default function StatusBar({ audio, transport, info, preview, error, onDevice, onRefreshMidi }: StatusBarProps) {
  const latencyMs =
    transport?.buffer_frames && audio?.sample_rate_hz
      ? (transport.buffer_frames / audio.sample_rate_hz) * 1000
      : null;
  const cpu = transport ? Math.round(transport.cpu_load * 100) : 0;
  const midi = audio?.midi_inputs ?? [];

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
      <span className={cpu > 70 ? "status-item error" : "status-item muted"} title="Audio CPU load">
        CPU {cpu}%
      </span>
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
      {info && (
        <span className="status-item muted">
          v{info.version} · {info.license}
        </span>
      )}
    </footer>
  );
}
