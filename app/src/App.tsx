import { useCallback, useEffect, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import type { Backend } from "./backend";
import type { AppInfo, AudioStatus, Command, ProjectView } from "./types";

const TIME_SIGNATURES = ["2/4", "3/4", "4/4", "5/4", "6/8", "7/8", "9/8", "12/8"];
const RULER_BARS = 32;

interface AppProps {
  backend: Backend;
}

export default function App({ backend }: AppProps) {
  const [view, setView] = useState<ProjectView | null>(null);
  const [audio, setAudio] = useState<AudioStatus | null>(null);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Runs a backend call and surfaces any failure in the status bar.
  const run = useCallback(async <T,>(call: () => Promise<T>): Promise<T | undefined> => {
    try {
      const result = await call();
      setError(null);
      return result;
    } catch (e) {
      setError(String(e));
      return undefined;
    }
  }, []);

  useEffect(() => {
    Promise.all([backend.getProject(), backend.audioStatus(), backend.appInfo()])
      .then(([v, a, i]) => {
        setView(v);
        setAudio(a);
        setInfo(i);
      })
      .catch((e: unknown) => setError(String(e)));
  }, [backend]);

  const execute = useCallback(
    async (command: Command) => {
      const v = await run(() => backend.execute(command));
      if (v) setView(v);
      else {
        // Re-sync so inputs drop the rejected value.
        const current = await run(() => backend.getProject());
        if (current) setView(current);
      }
    },
    [backend, run],
  );

  const undo = useCallback(async () => {
    const v = await run(() => backend.undo());
    if (v) setView(v);
  }, [backend, run]);

  const redo = useCallback(async () => {
    const v = await run(() => backend.redo());
    if (v) setView(v);
  }, [backend, run]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      const target = e.target as HTMLElement | null;
      if (target?.tagName === "INPUT") return; // let inputs keep their own undo
      const key = e.key.toLowerCase();
      if (key === "z" && !e.shiftKey) {
        e.preventDefault();
        void undo();
      } else if (key === "y" || (key === "z" && e.shiftKey)) {
        e.preventDefault();
        void redo();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [undo, redo]);

  const toggleTone = async () => {
    const a = await run(() => backend.setTestTone(!audio?.test_tone_on));
    if (a) setAudio(a);
  };

  if (!view) {
    return <div className="loading">{error ?? "Starting Nunc Pro Tune…"}</div>;
  }

  const { project } = view;
  const signature = `${project.time_signature.numerator}/${project.time_signature.denominator}`;

  return (
    <div className="app">
      <header className="transport">
        <div className="brand">
          <img src="/favicon.png" alt="" width={20} height={20} />
          <span>Nunc Pro Tune</span>
        </div>

        <CommitInput
          key={`name-${project.name}`}
          className="project-name"
          label="Project name"
          initial={project.name}
          onCommit={(name) => execute({ command: "rename_project", name })}
        />

        <div className="transport-buttons" role="group" aria-label="Transport">
          <button disabled title="Playback arrives in Phase 1">▶</button>
          <button disabled title="Playback arrives in Phase 1">■</button>
          <button disabled className="record" title="Recording arrives in Phase 4">●</button>
        </div>

        <label className="field">
          <span>Tempo</span>
          <CommitInput
            key={`tempo-${project.tempo_bpm}`}
            className="tempo"
            label="Tempo in BPM"
            inputMode="decimal"
            initial={String(project.tempo_bpm)}
            onCommit={(text) => {
              const bpm = Number(text);
              if (Number.isFinite(bpm)) void execute({ command: "set_tempo", bpm });
              else setError(`"${text}" is not a number`);
            }}
          />
          <span className="unit">BPM</span>
        </label>

        <label className="field">
          <span>Time</span>
          <select
            aria-label="Time signature"
            value={signature}
            onChange={(e) => {
              const [numerator, denominator] = e.target.value.split("/").map(Number);
              void execute({ command: "set_time_signature", numerator, denominator });
            }}
          >
            {(TIME_SIGNATURES.includes(signature) ? TIME_SIGNATURES : [signature, ...TIME_SIGNATURES]).map(
              (ts) => (
                <option key={ts}>{ts}</option>
              ),
            )}
          </select>
        </label>

        <div className="history" role="group" aria-label="History">
          <button onClick={() => void undo()} disabled={!view.can_undo} title="Undo (Ctrl+Z)">
            ↶ Undo
          </button>
          <button onClick={() => void redo()} disabled={!view.can_redo} title="Redo (Ctrl+Y)">
            ↷ Redo
          </button>
        </div>
      </header>

      <main className="workspace">
        <aside className="track-list">
          <div className="panel-title">Tracks</div>
          <p className="empty">No tracks yet.</p>
        </aside>
        <section className="timeline">
          <div className="ruler" style={{ gridTemplateColumns: `repeat(${RULER_BARS}, 96px)` }}>
            {Array.from({ length: RULER_BARS }, (_, i) => (
              <div key={i} className="bar-number">
                {i + 1}
              </div>
            ))}
          </div>
          <div className="timeline-empty">
            <h1>Phase 0: the shell works</h1>
            <p>
              Tracks, instruments, and the on-screen keyboard arrive in Phase 1 and 2.
              <br />
              Press <strong>Test sound</strong> below to check your speakers.
            </p>
          </div>
        </section>
      </main>

      <footer className="status-bar">
        <button
          className={audio?.test_tone_on ? "tone on" : "tone"}
          onClick={() => void toggleTone()}
          aria-pressed={audio?.test_tone_on ?? false}
        >
          {audio?.test_tone_on ? "Stop test sound" : "Test sound"}
        </button>
        <span className="status-item" title="Audio output device">
          🔈 {audio?.active_output ?? audio?.default_output ?? "No output device found"}
        </span>
        {audio?.sample_rate_hz && (
          <span className="status-item">{(audio.sample_rate_hz / 1000).toFixed(1)} kHz</span>
        )}
        {backend.preview && <span className="status-item preview">Browser preview: no audio engine</span>}
        {error && (
          <span className="status-item error" role="alert">
            {error}
          </span>
        )}
        <span className="spacer" />
        {info && (
          <span className="status-item muted">
            v{info.version} · {info.license}
          </span>
        )}
      </footer>
    </div>
  );
}

interface CommitInputProps {
  label: string;
  initial: string;
  className?: string;
  inputMode?: "decimal" | "text";
  onCommit: (value: string) => void;
}

/** A text field that only sends a Command on Enter or when focus leaves. */
function CommitInput({ label, initial, className, inputMode, onCommit }: CommitInputProps) {
  const [text, setText] = useState(initial);
  const commit = () => {
    if (text.trim() !== initial) onCommit(text.trim());
  };
  const onKeyDown = (e: ReactKeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") e.currentTarget.blur();
    if (e.key === "Escape") {
      setText(initial);
      e.currentTarget.blur();
    }
  };
  return (
    <input
      aria-label={label}
      className={className}
      inputMode={inputMode}
      value={text}
      onChange={(e) => setText(e.target.value)}
      onBlur={commit}
      onKeyDown={onKeyDown}
    />
  );
}
