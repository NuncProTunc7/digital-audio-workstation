import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import type { Backend } from "./backend";
import { AUDIO_EXTENSIONS, MIDI_EXTENSIONS, MUSICXML_EXTENSIONS } from "./backend";
import type { ExportKind } from "./backend";
import AudioPanel from "./components/AudioPanel";
import ClaudePanel from "./components/ClaudePanel";
import GodotExportDialog from "./components/GodotExportDialog";
import InstrumentPanel from "./components/InstrumentPanel";
import Mixer from "./components/Mixer";
import Piano from "./components/Piano";
import PianoRoll from "./components/PianoRoll";
import StepSequencer from "./components/StepSequencer";
import Versions from "./components/Versions";
import SheetMusic from "./components/SheetMusic";
import StatusBar from "./components/StatusBar";
import Timeline from "./components/Timeline";
import { formatPosition, snapDown } from "./format";
import {
  DEFAULT_BASE_NOTE,
  DEFAULT_VELOCITY,
  DRUM_BASE_NOTE,
  noteForCode,
  noteName,
  shiftOctave,
  stepVelocity,
} from "./keymap";
import type {
  AppInfo,
  AudioStatus,
  Catalog,
  ClaudeStatus,
  Command,
  Comparison,
  InputStatus,
  Peaks,
  Project,
  ProjectView,
  Track,
  TransportStatus,
} from "./types";

const TIME_SIGNATURES = ["2/4", "3/4", "4/4", "5/4", "6/8", "7/8", "9/8", "12/8"];
const STATUS_POLL_MS = 60;
const CLAUDE_POLL_MS = 2000;
const INPUT_POLL_MS = 70;
const TOAST_MS = 4000;
type Tab = "instrument" | "pianoroll" | "sheet" | "mixer";
const SHEET_REFRESH_MS = 300;

interface AppProps {
  backend: Backend;
}

function isTextEntry(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  if (el.tagName === "INPUT") return (el as HTMLInputElement).type !== "range";
  return el.tagName === "SELECT" || el.tagName === "TEXTAREA";
}

export default function App({ backend }: AppProps) {
  const [view, setView] = useState<ProjectView | null>(null);
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [audio, setAudio] = useState<AudioStatus | null>(null);
  const [transport, setTransport] = useState<TransportStatus | null>(null);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selectedTrackId, setSelectedTrackId] = useState(1);
  const [selectedClipId, setSelectedClipId] = useState<number | null>(null);
  const [selectedNotes, setSelectedNotes] = useState<ReadonlySet<number>>(new Set());
  const [tab, setTab] = useState<Tab>("instrument");
  const [pixelsPerBeat, setPixelsPerBeat] = useState(24);
  const [dockHeight, setDockHeight] = useState(330);
  const [baseNote, setBaseNote] = useState(DEFAULT_BASE_NOTE);
  const [velocity, setVelocity] = useState(DEFAULT_VELOCITY);
  const [activeNotes, setActiveNotes] = useState<ReadonlySet<number>>(new Set());
  const [claude, setClaude] = useState<ClaudeStatus | null>(null);
  const [claudeOpen, setClaudeOpen] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  // Loudness of the song and the version being compared A/B.
  const [comparison, setComparison] = useState<Comparison | null>(null);
  // Drum clips open in the step grid unless the user picked the piano roll.
  const [drumView, setDrumView] = useState<"steps" | "roll">("steps");
  const [peaks, setPeaks] = useState<Record<string, Peaks>>({});
  const peaksRequested = useRef(new Set<string>());
  const [input, setInput] = useState<InputStatus | null>(null);
  const [sheetAll, setSheetAll] = useState(false);
  const [sheetXml, setSheetXml] = useState<string | null>(null);
  const [sheetZoom, setSheetZoom] = useState(0.8);
  // Computer keys currently held, and the note each one started.
  const heldKeys = useRef(new Map<string, number>());
  // Latest view for callbacks that must not re-subscribe on every edit.
  const viewRef = useRef<ProjectView | null>(null);
  useEffect(() => {
    viewRef.current = view;
  }, [view]);

  // Runs a backend call and surfaces any failure in the status bar.
  const run = useCallback(async <T,>(call: () => Promise<T>): Promise<T | undefined> => {
    try {
      const result = await call();
      setError(null);
      return result;
    } catch (e) {
      setError(String(e));
      // Kept for the diagnostic report.
      backend.logError(String(e)).catch(() => {});
      return undefined;
    }
  }, [backend]);

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

  // Offer back work left unsaved when the app last closed unexpectedly
  // (once, even though development mode runs effects twice).
  const recoveryAsked = useRef(false);
  useEffect(() => {
    if (recoveryAsked.current) return;
    recoveryAsked.current = true;
    void (async () => {
      const found = await backend.recoveryCheck().catch(() => null);
      if (!found) return;
      const when = new Date(found.saved_at_ms).toLocaleString();
      const recover = await backend.confirm(
        `Nunc Pro Tune didn't close properly last time. Recover your unsaved changes to "${found.name}" from ${when}?`,
      );
      const v = await run(() => backend.recoveryResolve(recover));
      if (v) setView(v);
    })();
  }, [backend, run]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      backend
        .transportStatus()
        .then(setTransport)
        .catch(() => {});
    }, STATUS_POLL_MS);
    return () => window.clearInterval(timer);
  }, [backend]);

  // Window title shows the song name and an unsaved-changes dot.
  useEffect(() => {
    if (!view) return;
    void backend.setTitle(`${view.dirty ? "• " : ""}${view.project.name} — Nunc Pro Tune`);
  }, [backend, view?.dirty, view?.project.name, view]);

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

  const refreshClaude = useCallback(() => {
    backend
      .claudeStatus()
      .then(setClaude)
      .catch(() => {});
  }, [backend]);

  // Poll only while the panel is open; changes also trigger a refresh below.
  useEffect(() => {
    refreshClaude();
    if (!claudeOpen) return;
    const timer = window.setInterval(refreshClaude, CLAUDE_POLL_MS);
    return () => window.clearInterval(timer);
  }, [claudeOpen, refreshClaude]);

  // Claude edits the project from outside the UI: re-fetch and say what changed.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let toastTimer: number | undefined;
    void backend
      .onProjectChanged((description) => {
        backend
          .getProject()
          .then(setView)
          .catch(() => {});
        refreshClaude();
        setToast(description);
        window.clearTimeout(toastTimer);
        toastTimer = window.setTimeout(() => setToast(null), TOAST_MS);
      })
      .then((u) => {
        unlisten = u;
      });
    return () => {
      unlisten?.();
      window.clearTimeout(toastTimer);
    };
  }, [backend, refreshClaude]);

  const project = view?.project;
  const selectedTrack: Track | undefined =
    project?.tracks.find((t) => t.id === selectedTrackId) ?? project?.tracks[0];
  const selectedClip = project?.tracks.flatMap((t) => t.clips).find((c) => c.id === selectedClipId);
  const clipTrack = selectedClip ? project?.tracks.find((t) => t.clips.some((c) => c.id === selectedClip.id)) : undefined;
  const isDrums = selectedTrack?.instrument.kind === "drums";
  const isAudioTrack = selectedTrack?.instrument.kind === "audio";
  const missingAudio = useMemo(() => new Set(view?.missing_audio ?? []), [view?.missing_audio]);

  // The sheet music view follows the song while it is open.
  const sheetTrackId = sheetAll ? null : (selectedTrack?.id ?? null);
  useEffect(() => {
    if (tab !== "sheet" || !project) return;
    const timer = window.setTimeout(() => {
      backend
        .sheetMusic(sheetTrackId === null ? null : [sheetTrackId])
        .then(setSheetXml)
        .catch((e) => setError(String(e)));
    }, SHEET_REFRESH_MS);
    return () => window.clearTimeout(timer);
  }, [backend, tab, project, sheetTrackId]);
  const typingBase = isDrums ? DRUM_BASE_NOTE : baseNote;
  const recording = transport?.recording ?? false;

  const applyView = useCallback((v: ProjectView | undefined) => {
    if (v) setView(v);
    return v?.project;
  }, []);

  const execute = useCallback(
    async (command: Command): Promise<Project | undefined> => {
      const v = await run(() => backend.execute(command));
      if (v) return applyView(v);
      // Re-sync so inputs drop the rejected value.
      applyView(await run(() => backend.getProject()));
      return undefined;
    },
    [backend, run, applyView],
  );

  const endGesture = useCallback(() => void backend.endGesture(), [backend]);

  // Waveforms for every audio file in the song, fetched once each.
  useEffect(() => {
    for (const t of project?.tracks ?? []) {
      for (const c of t.clips) {
        const file = c.audio?.file;
        if (!file || peaksRequested.current.has(file) || missingAudio.has(file)) continue;
        peaksRequested.current.add(file);
        backend
          .audioPeaks(file)
          .then((p) => setPeaks((prev) => ({ ...prev, [file]: p })))
          .catch(() => peaksRequested.current.delete(file));
      }
    }
  }, [backend, project, missingAudio]);

  // The microphone is open (and metered) while an audio track is selected.
  useEffect(() => {
    if (!isAudioTrack) return;
    let alive = true;
    void backend.monitorInput(true).then((s) => alive && setInput(s));
    const timer = window.setInterval(() => {
      backend
        .inputStatus()
        .then((s) => alive && setInput(s))
        .catch(() => {});
    }, INPUT_POLL_MS);
    return () => {
      alive = false;
      window.clearInterval(timer);
      void backend.monitorInput(false);
    };
  }, [backend, isAudioTrack]);

  /** Imports files one by one, then selects the last new clip. */
  const importFiles = useCallback(
    async (paths: string[], trackId: number | null, beats: number) => {
      const ext = (p: string) => p.split(".").pop()?.toLowerCase() ?? "";
      const usable = paths.filter((p) =>
        [...AUDIO_EXTENSIONS, ...MIDI_EXTENSIONS, ...MUSICXML_EXTENSIONS].includes(ext(p)),
      );
      if (usable.length < paths.length) {
        setError("Only audio (WAV, MP3, M4A, FLAC, OGG), MIDI, and sheet music (MusicXML) files can be imported");
      }
      let latest: Project | undefined;
      for (const path of usable) {
        const v = await run(() =>
          MIDI_EXTENSIONS.includes(ext(path))
            ? backend.importMidi(path)
            : MUSICXML_EXTENSIONS.includes(ext(path))
              ? backend.importMusicXml(path)
              : backend.importAudio(path, trackId, beats),
        );
        if (!v) break;
        latest = applyView(v);
      }
      if (!latest) return;
      const newest = latest.tracks
        .flatMap((t) => t.clips.map((c) => ({ track: t.id, clip: c.id })))
        .reduce((a, b) => (b.clip > a.clip ? b : a), { track: 0, clip: 0 });
      if (newest.clip) {
        setSelectedClipId(newest.clip);
        setSelectedTrackId(newest.track);
        setTab("instrument");
      }
    },
    [backend, run, applyView],
  );

  const [exportOpen, setExportOpen] = useState(false);
  const [godotOpen, setGodotOpen] = useState(false);
  const exportAs = useCallback(
    async (kind: ExportKind) => {
      setExportOpen(false);
      const name = view?.project.name ?? "Untitled";
      const path = await run(() => backend.pickExportPath(kind, name));
      if (!path) return;
      setToast(`Exporting ${path.split(/[\\/]/).pop()}…`);
      const done = await run(() =>
        kind === "wav"
          ? backend.exportWav(path)
          : kind === "mid"
            ? backend.exportMidi(path)
            : backend.exportMusicXml(path, null),
      );
      setToast(done === undefined ? null : `Exported ${path.split(/[\\/]/).pop()}`);
      window.setTimeout(() => setToast(null), TOAST_MS);
    },
    [backend, run, view?.project.name],
  );

  const pickAndImport = useCallback(
    async (trackId: number | null, beats: number) => {
      const paths = await run(() => backend.pickAudioFiles());
      if (paths && paths.length > 0) await importFiles(paths, trackId, beats);
    },
    [backend, run, importFiles],
  );

  // Files dragged from Explorer onto the timeline land where they're dropped.
  // Latest playhead, for handlers that shouldn't re-subscribe on every poll.
  const positionRef = useRef(0);
  const dropTarget = useRef<(x: number, y: number) => { trackId: number | null; beats: number }>(() => ({
    trackId: null,
    beats: 0,
  }));
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void backend
      .onFileDrop((paths, x, y) => {
        const { trackId, beats } = dropTarget.current(x, y);
        void importFiles(paths, trackId, beats);
      })
      .then((u) => {
        unlisten = u;
      });
    return () => unlisten?.();
  }, [backend, importFiles]);
  useLayoutEffect(() => {
    positionRef.current = transport?.position_beats ?? 0;
    dropTarget.current = (x, y) => {
      const bpb = project?.time_signature.numerator ?? 4;
      const lane = document.elementFromPoint?.(x, y)?.closest<HTMLElement>(".lane[data-track]");
      const lanes = document.querySelector<HTMLElement>(".lanes");
      if (!lane || !lanes) {
        return { trackId: null, beats: snapDown(Math.max(0, transport?.position_beats ?? 0), bpb) };
      }
      const id = Number(lane.dataset.track);
      const track = project?.tracks.find((t) => t.id === id);
      const beats = snapDown(Math.max(0, (x - lanes.getBoundingClientRect().left) / pixelsPerBeat), 1);
      return { trackId: track?.instrument.kind === "audio" ? id : null, beats };
    };
  });

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

  const audition = useCallback(
    (note: number) => {
      const track = clipTrack ?? selectedTrack;
      if (!track) return;
      void backend.noteOn(track.id, note, 0.8);
      window.setTimeout(() => void backend.noteOff(track.id, note), 250);
    },
    [backend, clipTrack, selectedTrack],
  );

  const releaseAll = useCallback(() => {
    heldKeys.current.clear();
    setActiveNotes(new Set());
    void backend.allNotesOff();
  }, [backend]);

  const selectTrack = useCallback(
    (id: number) => {
      if (id !== selectedTrackId) releaseAll();
      setSelectedTrackId(id);
      void backend.selectTrack(id);
    },
    [backend, releaseAll, selectedTrackId],
  );

  const selectClip = useCallback(
    (id: number | null, trackId: number) => {
      setSelectedClipId(id);
      setSelectedNotes(new Set());
      selectTrack(trackId);
    },
    [selectTrack],
  );

  const openClip = useCallback((id: number) => {
    setSelectedClipId(id);
    setSelectedNotes(new Set());
    setTab("pianoroll");
  }, []);

  const undo = useCallback(async () => applyView(await run(() => backend.undo())), [backend, run, applyView]);
  const redo = useCallback(async () => applyView(await run(() => backend.redo())), [backend, run, applyView]);

  // ---- Files ----
  const confirmDiscard = useCallback(async () => {
    if (!viewRef.current?.dirty) return true;
    return backend.confirm("You have unsaved changes. Discard them?");
  }, [backend]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void backend.onCloseRequested(confirmDiscard).then((u) => {
      unlisten = u;
    });
    return () => unlisten?.();
  }, [backend, confirmDiscard]);

  const newFile = useCallback(async () => {
    if (!(await confirmDiscard())) return;
    applyView(await run(() => backend.newProject()));
    setSelectedClipId(null);
    setSelectedTrackId(1);
  }, [backend, run, applyView, confirmDiscard]);

  const openFile = useCallback(async () => {
    if (!(await confirmDiscard())) return;
    const path = await run(() => backend.pickOpenPath());
    if (!path) return;
    const p = applyView(await run(() => backend.openProject(path)));
    setSelectedClipId(null);
    if (p?.tracks[0]) setSelectedTrackId(p.tracks[0].id);
  }, [backend, run, applyView, confirmDiscard]);

  const saveFile = useCallback(
    async (saveAs: boolean) => {
      const current = viewRef.current;
      if (!current) return;
      let path: string | null = null;
      if (saveAs || !current.file_path) {
        path = (await run(() => backend.pickSavePath(current.project.name))) ?? null;
        if (!path) return;
      }
      applyView(await run(() => backend.saveProject(path)));
    },
    [backend, run, applyView],
  );

  // ---- Transport ----
  const toggleRecord = useCallback(async () => {
    if (recording) {
      applyView(await run(() => backend.recordStop()));
    } else if (selectedTrack) {
      await run(() => backend.recordStart(selectedTrack.id));
    }
  }, [backend, run, applyView, recording, selectedTrack]);

  const stop = useCallback(async () => {
    if (recording) await toggleRecord();
    else await backend.stop();
  }, [backend, recording, toggleRecord]);

  const togglePlay = useCallback(() => {
    void (transport?.playing ? stop() : backend.play());
  }, [backend, transport?.playing, stop]);

  const removeTrack = useCallback(
    async (track: Track) => {
      if (track.clips.length > 0 && !(await backend.confirm(`Delete "${track.name}" and its clips?`))) return;
      await execute({ command: "remove_track", track_id: track.id });
      endGesture();
    },
    [backend, execute, endGesture],
  );

  const deleteSelection = useCallback(async () => {
    if (tab === "pianoroll" && selectedClip && selectedNotes.size > 0) {
      await execute({ command: "remove_notes", clip_id: selectedClip.id, note_ids: [...selectedNotes] });
      setSelectedNotes(new Set());
    } else if (selectedClip) {
      await execute({ command: "delete_clip", clip_id: selectedClip.id });
      setSelectedClipId(null);
    }
    endGesture();
  }, [tab, selectedClip, selectedNotes, execute, endGesture]);

  // Keyboard: shortcuts and musical typing. A layout effect, so the listener
  // is swapped during the commit and a key press can never reach a handler
  // from the previous render.
  useLayoutEffect(() => {
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
        } else if (key === "s") {
          e.preventDefault();
          void saveFile(e.shiftKey);
        } else if (key === "o") {
          e.preventDefault();
          void openFile();
        } else if (key === "n") {
          e.preventDefault();
          void newFile();
        } else if (key === "d" && selectedClip) {
          e.preventDefault();
          void execute({ command: "duplicate_clip", clip_id: selectedClip.id, start_beats: null }).then(endGesture);
        } else if (key === "e" && selectedClip) {
          // Split the selected clip at the playhead.
          e.preventDefault();
          const at = positionRef.current;
          if (at > selectedClip.start_beats && at < selectedClip.start_beats + selectedClip.length_beats) {
            void execute({ command: "split_clip", clip_id: selectedClip.id, at_beats: at }).then(endGesture);
          }
        } else if (key === "a" && tab === "pianoroll" && selectedClip) {
          e.preventDefault();
          setSelectedNotes(new Set(selectedClip.notes.map((n) => n.id)));
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
        case "KeyR":
          void toggleRecord();
          return;
        case "Home":
          void backend.locate(0);
          return;
        case "Delete":
        case "Backspace":
          e.preventDefault();
          void deleteSelection();
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
  }, [
    backend,
    undo,
    redo,
    saveFile,
    openFile,
    newFile,
    togglePlay,
    toggleRecord,
    deleteSelection,
    execute,
    endGesture,
    selectedClip,
    tab,
    typingBase,
    noteOn,
    noteOff,
    releaseAll,
  ]);

  if (!view || !catalog || !project || !selectedTrack) {
    return <div className="loading">{error ?? "Starting Nunc Pro Tune…"}</div>;
  }

  const signature = `${project.time_signature.numerator}/${project.time_signature.denominator}`;
  const beatsPerBar = project.time_signature.numerator;
  const pianoLow = Math.max(24, Math.min(60, baseNote - 12));
  const copyReport = async () => {
    const report = await run(() => backend.diagnosticReport());
    if (report === undefined) return;
    const copied = await navigator.clipboard?.writeText(report).then(
      () => true,
      () => false,
    );
    setToast(
      copied
        ? "Diagnostic report copied. Paste it into your message to Claude."
        : "Couldn't reach the clipboard; ask Claude to run diagnostic_report instead.",
    );
    window.setTimeout(() => setToast(null), TOAST_MS * 2);
  };

  const position = transport?.position_beats ?? 0;
  const countInBeats = transport?.count_in_beats ?? 0;

  return (
    <div className="app" style={{ gridTemplateRows: `auto 1fr 6px ${dockHeight}px auto` }}>
      <header className="transport">
        <div className="brand">
          <img src="/favicon.png" alt="" width={20} height={20} />
          <span>Nunc Pro Tune</span>
        </div>

        <div className="file-buttons" role="group" aria-label="File">
          <button className="small" onClick={() => void newFile()} title="New project (Ctrl+N)">
            New
          </button>
          <button className="small" onClick={() => void openFile()} title="Open project (Ctrl+O)">
            Open
          </button>
          <button className="small" onClick={() => void saveFile(false)} title="Save (Ctrl+S). Ctrl+Shift+S: Save as">
            Save{view.dirty ? " •" : ""}
          </button>
          <button
            className="small"
            onClick={() =>
              void run(() => backend.pickImportFiles()).then((paths) => {
                if (paths && paths.length > 0) void importFiles(paths, null, snapDown(position, beatsPerBar));
              })
            }
            title="Bring in audio (WAV, MP3, phone recordings) or MIDI files"
          >
            Import…
          </button>
          <div className="menu-anchor">
            <button
              className="small"
              aria-haspopup="menu"
              aria-expanded={exportOpen}
              onClick={() => setExportOpen((o) => !o)}
              title="Save the song as audio or for other apps"
            >
              Export ▾
            </button>
            {exportOpen && (
              <div className="menu" role="menu">
                <button role="menuitem" onClick={() => void exportAs("wav")}>
                  Song as WAV audio…
                </button>
                <button role="menuitem" onClick={() => void exportAs("mid")}>
                  MIDI file…
                </button>
                <button role="menuitem" onClick={() => void exportAs("musicxml")}>
                  Sheet music (MusicXML)…
                </button>
                <button
                  role="menuitem"
                  onClick={() => {
                    setExportOpen(false);
                    setGodotOpen(true);
                  }}
                >
                  To Godot (loops, stems)…
                </button>
              </div>
            )}
          </div>
        </div>

        <Versions
          project={project}
          comparing={transport?.comparing ?? null}
          comparison={comparison}
          onCommand={execute}
          onEndGesture={endGesture}
          onCompare={(id) => {
            setToast("Measuring both versions…");
            void run(() => backend.compareStart(id)).then((c) => {
              setToast(null);
              if (c) setComparison(c);
            });
          }}
          onListen={(side) => void run(() => backend.compareListen(side))}
          onStopComparing={() => {
            setComparison(null);
            void run(() => backend.compareStop());
          }}
        />

        <CommitInput
          key={`name-${project.name}`}
          className="project-name"
          label="Project name"
          initial={project.name}
          onCommit={(name) => void execute({ command: "rename_project", name })}
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
          <button onClick={() => void stop()} title="Stop (Space). Press twice to return to the start." aria-label="Stop">
            ■
          </button>
          <button
            onClick={() => void toggleRecord()}
            className={recording ? "record recording" : "record"}
            title={`Record onto "${selectedTrack.name}" (R). Play along; press again to stop and keep the take.`}
            aria-label="Record"
            aria-pressed={recording}
          >
            ●
          </button>
        </div>

        {countInBeats > 0 ? (
          <span className="position counting-in" aria-label="Count-in" title="Counting in: start on the beat after the last click">
            {Math.ceil(countInBeats - 1e-6)}
          </span>
        ) : (
          <span className="position" aria-label="Position" title="Bar.Beat">
            {formatPosition(position, beatsPerBar)}
          </span>
        )}

        <button
          className={project.loop_region.enabled ? "toggle on" : "toggle"}
          aria-pressed={project.loop_region.enabled}
          onClick={() =>
            void execute({ command: "set_loop", enabled: !project.loop_region.enabled, start_beats: null, end_beats: null })
          }
          title="Loop the highlighted region (drag along the top of the ruler to set it)"
        >
          Loop
        </button>
        <button
          className={transport?.metronome_on ? "toggle on" : "toggle"}
          aria-pressed={transport?.metronome_on ?? true}
          onClick={() => void backend.setMetronome(!(transport?.metronome_on ?? true))}
          title="Metronome click while playing"
        >
          Click
        </button>
        <label className="field" title="Clicks to play before recording starts, so you can come in on the first beat">
          <span>Count-in</span>
          <select
            aria-label="Count-in before recording"
            value={transport?.count_in_bars ?? 1}
            onChange={(e) => {
              void run(() => backend.setCountIn(Number(e.target.value)));
              e.currentTarget.blur();
            }}
          >
            <option value={0}>Off</option>
            <option value={1}>1 bar</option>
            <option value={2}>2 bars</option>
          </select>
        </label>

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
          <button onClick={() => void undo()} disabled={!view.can_undo} title="Undo (Ctrl+Z)" aria-label="Undo">
            ↶
          </button>
          <button onClick={() => void redo()} disabled={!view.can_redo} title="Redo (Ctrl+Y)" aria-label="Redo">
            ↷
          </button>
        </div>
      </header>

      <main className="arrange">
        <Timeline
          project={project}
          selectedTrackId={selectedTrack.id}
          selectedClipId={selectedClipId}
          playheadBeats={position}
          recording={recording}
          trackPeaks={transport?.track_peaks ?? []}
          pixelsPerBeat={pixelsPerBeat}
          onZoom={setPixelsPerBeat}
          onSelectTrack={selectTrack}
          onSelectClip={selectClip}
          onOpenClip={openClip}
          onCommand={execute}
          onEndGesture={endGesture}
          onLocate={(beats) => void backend.locate(beats)}
          onRemoveTrack={(t) => void removeTrack(t)}
          peaks={peaks}
          missingAudio={missingAudio}
          onImportAudio={(trackId, beats) => void pickAndImport(trackId, beats)}
          catalog={catalog}
        />
      </main>

      <div
        className="splitter"
        role="separator"
        aria-orientation="horizontal"
        title="Drag to resize"
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture?.(e.pointerId);
          const y0 = e.clientY;
          const h0 = dockHeight;
          const move = (ev: PointerEvent) =>
            setDockHeight(Math.max(180, Math.min(window.innerHeight * 0.75, h0 - (ev.clientY - y0))));
          const up = () => {
            window.removeEventListener("pointermove", move);
            window.removeEventListener("pointerup", up);
          };
          window.addEventListener("pointermove", move);
          window.addEventListener("pointerup", up);
        }}
      />

      <section className="dock">
        <nav className="tabs" role="tablist">
          {(
            [
              ["instrument", `${isAudioTrack ? "Audio" : "Instrument"} · ${selectedTrack.name}`],
              ["pianoroll", selectedClip && !selectedClip.audio ? `Piano roll · ${selectedClip.name}` : "Piano roll"],
              ["sheet", "Sheet music"],
              ["mixer", "Mixer"],
            ] as [Tab, string][]
          ).map(([id, label]) => (
            <button key={id} role="tab" aria-selected={tab === id} className={tab === id ? "tab active" : "tab"} onClick={() => setTab(id)}>
              {label}
            </button>
          ))}
        </nav>

        <div className="dock-body">
          {tab === "instrument" && isAudioTrack && (
            <AudioPanel
              track={selectedTrack}
              clip={clipTrack?.id === selectedTrack.id ? selectedClip : undefined}
              peaks={selectedClip?.audio ? peaks[selectedClip.audio.file] : undefined}
              missing={selectedClip?.audio ? missingAudio.has(selectedClip.audio.file) : false}
              tempoBpm={project.tempo_bpm}
              playheadBeats={position}
              recording={recording}
              input={input}
              onInputDevice={(name) => void run(() => backend.setInputDevice(name)).then((s) => s && setInput(s))}
              onRecordingOffset={(ms) =>
                void run(() => backend.setRecordingOffset(ms)).then(
                  (delay) => delay && setInput((i) => (i ? { ...i, delay } : i)),
                )
              }
              onCalibrate={async () => {
                const result = await run(() => backend.calibrateRecording());
                const s = await run(() => backend.inputStatus());
                if (s) setInput(s);
                return result;
              }}
              onCommand={execute}
              onEndGesture={endGesture}
              onImport={() => void pickAndImport(selectedTrack.id, snapDown(position, beatsPerBar))}
              onRecord={() => void toggleRecord()}
            />
          )}

          {tab === "instrument" && !isAudioTrack && (
            <div className="instrument-tab">
              <InstrumentPanel
                track={selectedTrack}
                catalog={catalog}
                activeNotes={activeNotes}
                onParam={(param, value) =>
                  void execute({ command: "set_instrument_param", track_id: selectedTrack.id, param, value })
                }
                onEndGesture={endGesture}
                onPreset={(preset) => void execute({ command: "load_preset", track_id: selectedTrack.id, preset })}
                onPadHit={noteOn}
                onPadRelease={noteOff}
                onChooseSamplePack={() =>
                  void run(() => backend.pickSamplePack()).then((path) => {
                    if (path) void execute({ command: "load_sample_pack", track_id: selectedTrack.id, path }).then(endGesture);
                  })
                }
                samplePackStatus={backend.samplePackStatus}
              />
              <div className="keyboard-dock">
                <div className="keyboard-help">
                  {isDrums ? (
                    <span>
                      Drums: <kbd>A</kbd> kick · <kbd>S</kbd> snare · <kbd>T</kbd> closed hat · <kbd>U</kbd> open hat ·{" "}
                      <kbd>O</kbd> crash — or click the pads. <kbd>R</kbd> record · <kbd>Space</kbd> play/stop
                    </span>
                  ) : (
                    <span>
                      Play <kbd>A</kbd>–<kbd>'</kbd> and <kbd>W</kbd> <kbd>E</kbd> <kbd>T</kbd> <kbd>Y</kbd> <kbd>U</kbd>{" "}
                      <kbd>O</kbd> <kbd>P</kbd> · <kbd>Z</kbd>/<kbd>X</kbd> octave ({noteName(baseNote)}) · <kbd>C</kbd>/
                      <kbd>V</kbd> velocity ({velocity}) · <kbd>R</kbd> record · <kbd>Space</kbd> play/stop
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
              </div>
            </div>
          )}

          {tab === "pianoroll" &&
            (selectedClip?.audio ? (
              <p className="empty-state">
                This is an audio clip. Change its volume and fades in the Audio tab, and drag its edges on the timeline to
                trim it.
              </p>
            ) : selectedClip && clipTrack && clipTrack.instrument.kind === "drums" && drumView === "steps" ? (
              <StepSequencer
                clip={selectedClip}
                drumPads={catalog.drum_pads}
                beatsPerBar={beatsPerBar}
                playheadBeats={position - selectedClip.start_beats}
                onCommand={execute}
                onEndGesture={endGesture}
                onAudition={audition}
                onShowPianoRoll={() => setDrumView("roll")}
              />
            ) : selectedClip && clipTrack ? (
              <PianoRoll
                onShowSteps={clipTrack.instrument.kind === "drums" ? () => setDrumView("steps") : undefined}
                clip={selectedClip}
                track={clipTrack}
                drumPads={catalog.drum_pads}
                playheadBeats={position - selectedClip.start_beats}
                beatsPerBar={beatsPerBar}
                selected={selectedNotes}
                onSelect={setSelectedNotes}
                onCommand={execute}
                onEndGesture={endGesture}
                onAudition={audition}
              />
            ) : (
              <p className="empty-state">
                Double-click a clip on the timeline to edit its notes — or double-click an empty spot in a track to make a
                new clip. You can also press <kbd>R</kbd> to record what you play.
              </p>
            ))}

          {tab === "sheet" && (
            <div className="sheet-tab">
              <div className="roll-toolbar">
                <div className="button-group" role="group" aria-label="Which tracks">
                  <button className={sheetAll ? "small" : "small toggle on"} onClick={() => setSheetAll(false)}>
                    {selectedTrack.name}
                  </button>
                  <button className={sheetAll ? "small toggle on" : "small"} onClick={() => setSheetAll(true)}>
                    All tracks
                  </button>
                </div>
                <span className="muted">Notes are shown on a sixteenth-note grid.</span>
                <span className="spacer" />
                <button className="small" onClick={() => void exportAs("musicxml")} title="Save for MuseScore and other notation apps">
                  Export MusicXML…
                </button>
                <button className="small" onClick={() => setSheetZoom((z) => Math.max(0.4, z / 1.2))} title="Zoom out">
                  −
                </button>
                <button className="small" onClick={() => setSheetZoom((z) => Math.min(2, z * 1.2))} title="Zoom in">
                  +
                </button>
              </div>
              {!sheetAll && isAudioTrack ? (
                <p className="empty-state">Audio tracks have no notes to show. Pick an instrument track or All tracks.</p>
              ) : (
                <SheetMusic xml={sheetXml} zoom={sheetZoom} />
              )}
            </div>
          )}

          {tab === "mixer" && (
            <Mixer
              project={project}
              catalog={catalog}
              trackPeaks={transport?.track_peaks ?? []}
              masterPeaks={[transport?.peak_left ?? 0, transport?.peak_right ?? 0]}
              selectedTrackId={selectedTrack.id}
              onSelectTrack={selectTrack}
              onCommand={execute}
              onEndGesture={endGesture}
            />
          )}
        </div>
      </section>

      <StatusBar
        audio={audio}
        transport={transport}
        info={info}
        preview={backend.preview}
        error={error}
        filePath={view.file_path}
        onDevice={(name) => void run(() => backend.setOutputDevice(name)).then((a) => a && setAudio(a))}
        onBufferSize={(frames) => void run(() => backend.setBufferSize(frames)).then((a) => a && setAudio(a))}
        onCopyReport={() => void copyReport()}
        onRefreshMidi={() => void run(() => backend.refreshMidi()).then((a) => a && setAudio(a))}
        claude={claude}
        onToggleClaude={() => setClaudeOpen((o) => !o)}
      />
      {godotOpen && (
        <GodotExportDialog
          project={project}
          onPickFolder={() => backend.pickFolder("Choose your Godot project folder")}
          onExport={(options) => run(() => backend.exportGodot(options))}
          onClose={() => setGodotOpen(false)}
        />
      )}
      {claudeOpen && (
        <ClaudePanel
          status={claude}
          onInstallDesktop={() => void run(() => backend.claudeInstallDesktop()).then((s) => s && setClaude(s))}
          onClose={() => setClaudeOpen(false)}
          onCopyReport={() => void copyReport()}
        />
      )}
      {toast && (
        <div className="toast" role="status">
          {toast}
        </div>
      )}
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
