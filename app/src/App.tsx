import { useCallback, useEffect, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import type { Backend } from "./backend";
import InstrumentPanel from "./components/InstrumentPanel";
import Piano from "./components/Piano";
import StatusBar from "./components/StatusBar";
import TrackList from "./components/TrackList";
import { formatPosition } from "./format";
import {
  DEFAULT_BASE_NOTE,
  DEFAULT_VELOCITY,
  DRUM_BASE_NOTE,
  noteForCode,
  noteName,
  shiftOctave,
  stepVelocity,
} from "./keymap";
import type { AppInfo, AudioStatus, Catalog, Command, ProjectView, TransportStatus } from "./types";

const TIME_SIGNATURES = ["2/4", "3/4", "4/4", "5/4", "6/8", "7/8", "9/8", "12/8"];
const STATUS_POLL_MS = 60;

interface AppProps {
  backend: Backend;
}

function isTextEntry(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  return el?.tagName === "INPUT" && (el as HTMLInputElement).type !== "range"
    ? true
    : el?.tagName === "SELECT" || el?.tagName === "TEXTAREA";
}

export default function App({ backend }: AppProps) {
  const [view, setView] = useState<ProjectView | null>(null);
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [audio, setAudio] = useState<AudioStatus | null>(null);
  const [transport, setTransport] = useState<TransportStatus | null>(null);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState(1);
  const [baseNote, setBaseNote] = useState(DEFAULT_BASE_NOTE);
  const [velocity, setVelocity] = useState(DEFAULT_VELOCITY);
  const [activeNotes, setActiveNotes] = useState<ReadonlySet<number>>(new Set());
  // Computer keys currently held, and the note each one started.
  const heldKeys = useRef(new Map<string, number>());

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
    Promise.all([backend.getProject(), backend.catalog(), backend.audioStatus(), backend.appInfo()])
      .then(([v, c, a, i]) => {
        setView(v);
        setCatalog(c);
        setAudio(a);
        setInfo(i);
      })
      .catch((e: unknown) => setError(String(e)));
  }, [backend]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      backend
        .transportStatus()
        .then(setTransport)
        .catch(() => {});
    }, STATUS_POLL_MS);
    return () => window.clearInterval(timer);
  }, [backend]);

  const markNote = useCallback((note: number, on: boolean) => {
    setActiveNotes((prev) => {
      const next = new Set(prev);
      if (on) next.add(note);
      else next.delete(note);
      return next;
    });
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void backend.onMidiNote(markNote).then((u) => {
      unlisten = u;
    });
    return () => unlisten?.();
  }, [backend, markNote]);

  const selectedTrack = view?.project.tracks.find((t) => t.id === selectedId) ?? view?.project.tracks[0];
  const isDrums = selectedTrack?.instrument.kind === "drums";
  const typingBase = isDrums ? DRUM_BASE_NOTE : baseNote;

  const noteOn = useCallback(
    (note: number) => {
      if (!selectedTrack) return;
      void backend.noteOn(selectedTrack.id, note, velocity / 127);
      markNote(note, true);
    },
    [backend, selectedTrack, velocity, markNote],
  );

  const noteOff = useCallback(
    (note: number) => {
      if (!selectedTrack) return;
      void backend.noteOff(selectedTrack.id, note);
      markNote(note, false);
    },
    [backend, selectedTrack, markNote],
  );

  const releaseAll = useCallback(() => {
    heldKeys.current.clear();
    setActiveNotes(new Set());
    void backend.allNotesOff();
  }, [backend]);

  const selectTrack = (id: number) => {
    releaseAll();
    setSelectedId(id);
    void backend.selectTrack(id);
  };

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

  const togglePlay = useCallback(() => {
    void (transport?.playing ? backend.stop() : backend.play());
  }, [backend, transport?.playing]);

  // Keyboard: shortcuts and musical typing.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (isTextEntry(e.target)) return;
      if (e.ctrlKey || e.metaKey) {
        const key = e.key.toLowerCase();
        if (key === "z" && !e.shiftKey) {
          e.preventDefault();
          void undo();
        } else if (key === "y" || (key === "z" && e.shiftKey)) {
          e.preventDefault();
          void redo();
        }
        return;
      }
      if (e.altKey) return;
      if (e.repeat) {
        if (heldKeys.current.has(e.code)) e.preventDefault();
        return;
      }
      switch (e.code) {
        case "Space":
          e.preventDefault();
          togglePlay();
          return;
        case "KeyZ":
          setBaseNote((b) => shiftOctave(b, -1));
          return;
        case "KeyX":
          setBaseNote((b) => shiftOctave(b, 1));
          return;
        case "KeyC":
          setVelocity((v) => stepVelocity(v, -1));
          return;
        case "KeyV":
          setVelocity((v) => stepVelocity(v, 1));
          return;
      }
      const note = noteForCode(e.code, typingBase);
      if (note !== null && !heldKeys.current.has(e.code)) {
        e.preventDefault();
        heldKeys.current.set(e.code, note);
        noteOn(note);
      }
    };
    const onKeyUp = (e: KeyboardEvent) => {
      const note = heldKeys.current.get(e.code);
      if (note !== undefined) {
        heldKeys.current.delete(e.code);
        noteOff(note);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", releaseAll);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", releaseAll);
    };
  }, [undo, redo, togglePlay, typingBase, noteOn, noteOff, releaseAll]);

  if (!view || !catalog || !selectedTrack) {
    return <div className="loading">{error ?? "Starting Nunc Pro Tune…"}</div>;
  }

  const { project } = view;
  const signature = `${project.time_signature.numerator}/${project.time_signature.denominator}`;
  const pianoLow = Math.max(24, Math.min(60, baseNote - 12));

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
          <button
            onClick={() => void backend.play()}
            className={transport?.playing ? "playing" : ""}
            title="Play (Space)"
            aria-label="Play"
          >
            ▶
          </button>
          <button onClick={() => void backend.stop()} title="Stop (Space). Press twice to return to the start." aria-label="Stop">
            ■
          </button>
          <button disabled className="record" title="Recording arrives in Phase 2" aria-label="Record">
            ●
          </button>
        </div>

        <span className="position" aria-label="Position" title="Bar.Beat">
          {formatPosition(transport?.position_beats ?? 0, project.time_signature.numerator)}
        </span>

        <button
          className={transport?.metronome_on ? "toggle on" : "toggle"}
          aria-pressed={transport?.metronome_on ?? true}
          onClick={() => void backend.setMetronome(!(transport?.metronome_on ?? true))}
          title="Metronome click while playing"
        >
          Click
        </button>

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
              e.currentTarget.blur();
            }}
          >
            {(TIME_SIGNATURES.includes(signature) ? TIME_SIGNATURES : [signature, ...TIME_SIGNATURES]).map((ts) => (
              <option key={ts}>{ts}</option>
            ))}
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
        <TrackList tracks={project.tracks} selectedId={selectedTrack.id} onSelect={selectTrack} />
        <InstrumentPanel
          track={selectedTrack}
          catalog={catalog}
          activeNotes={activeNotes}
          onParam={(param, value) =>
            void execute({ command: "set_instrument_param", track_id: selectedTrack.id, param, value })
          }
          onEndGesture={() => void backend.endGesture()}
          onPreset={(preset) => void execute({ command: "load_preset", track_id: selectedTrack.id, preset })}
          onPadHit={noteOn}
          onPadRelease={noteOff}
        />
      </main>

      <section className="keyboard-dock" aria-label="Keyboard">
        <div className="keyboard-help">
          {isDrums ? (
            <span>
              Drums: <kbd>A</kbd> kick · <kbd>S</kbd> snare · <kbd>T</kbd> closed hat · <kbd>U</kbd> open hat ·{" "}
              <kbd>O</kbd> crash — or click the pads.
            </span>
          ) : (
            <span>
              Play with <kbd>A</kbd>–<kbd>'</kbd> (white keys) and <kbd>W</kbd> <kbd>E</kbd> <kbd>T</kbd>{" "}
              <kbd>Y</kbd> <kbd>U</kbd> <kbd>O</kbd> <kbd>P</kbd> (black keys) · <kbd>Z</kbd>/<kbd>X</kbd> octave (
              {noteName(baseNote)}) · <kbd>C</kbd>/<kbd>V</kbd> velocity ({velocity}) · <kbd>Space</kbd> play/stop
            </span>
          )}
        </div>
        {!isDrums && (
          <Piano
            lowNote={pianoLow}
            highNote={pianoLow + 47}
            activeNotes={activeNotes}
            baseNote={typingBase}
            onNoteOn={noteOn}
            onNoteOff={noteOff}
          />
        )}
      </section>

      <StatusBar
        audio={audio}
        transport={transport}
        info={info}
        preview={backend.preview}
        error={error}
        onDevice={(name) => void run(() => backend.setOutputDevice(name)).then((a) => a && setAudio(a))}
        onRefreshMidi={() => void run(() => backend.refreshMidi()).then((a) => a && setAudio(a))}
      />
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
