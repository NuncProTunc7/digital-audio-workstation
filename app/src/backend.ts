import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import catalogJson from "./generated/instruments.json";
import type {
  AppInfo,
  AudioStatus,
  Catalog,
  Command,
  Instrument,
  Project,
  ProjectView,
  TransportStatus,
} from "./types";

/** Everything the UI can ask of the Rust side. */
export interface Backend {
  /** True when running in a plain browser with no engine behind it. */
  readonly preview: boolean;
  appInfo(): Promise<AppInfo>;
  catalog(): Promise<Catalog>;

  // Project edits (undoable).
  getProject(): Promise<ProjectView>;
  execute(command: Command): Promise<ProjectView>;
  endGesture(): Promise<void>;
  undo(): Promise<ProjectView>;
  redo(): Promise<ProjectView>;

  // Live actions.
  noteOn(trackId: number, note: number, velocity: number): Promise<void>;
  noteOff(trackId: number, note: number): Promise<void>;
  allNotesOff(): Promise<void>;
  selectTrack(trackId: number): Promise<void>;
  play(): Promise<void>;
  stop(): Promise<void>;
  setMetronome(on: boolean): Promise<void>;
  transportStatus(): Promise<TransportStatus>;
  audioStatus(): Promise<AudioStatus>;
  setOutputDevice(name: string | null): Promise<AudioStatus>;
  refreshMidi(): Promise<AudioStatus>;
  /** Calls back for each MIDI keyboard note. Returns an unsubscribe function. */
  onMidiNote(callback: (note: number, on: boolean) => void): Promise<() => void>;
}

export const tauriBackend: Backend = {
  preview: false,
  appInfo: () => invoke("app_info"),
  catalog: () => invoke("instrument_catalog"),
  getProject: () => invoke("get_project"),
  execute: (command) => invoke("execute", { command }),
  endGesture: () => invoke("end_gesture"),
  undo: () => invoke("undo"),
  redo: () => invoke("redo"),
  noteOn: (trackId, note, velocity) => invoke("note_on", { trackId, note, velocity }),
  noteOff: (trackId, note) => invoke("note_off", { trackId, note }),
  allNotesOff: () => invoke("all_notes_off"),
  selectTrack: (trackId) => invoke("select_track", { trackId }),
  play: () => invoke("play"),
  stop: () => invoke("stop"),
  setMetronome: (on) => invoke("set_metronome", { on }),
  transportStatus: () => invoke("transport_status"),
  audioStatus: () => invoke("audio_status"),
  setOutputDevice: (name) => invoke("set_output_device", { name }),
  refreshMidi: () => invoke("refresh_midi"),
  onMidiNote: async (callback) =>
    listen<{ note: number; on: boolean }>("midi-note", (e) => callback(e.payload.note, e.payload.on)),
};

const catalog = catalogJson as Catalog;

function defaultInstrument(kind: Instrument["kind"], preset: string): Instrument {
  const description = catalog.instruments.find((i) => i.kind === kind);
  const params: Record<string, number> = {};
  for (const spec of description?.params ?? []) params[spec.id] = spec.default;
  return { kind, preset, params };
}

/**
 * In-memory stand-in so the UI can be developed and screenshotted in a
 * browser. It makes no sound and only loosely validates.
 */
export function createPreviewBackend(): Backend {
  let project: Project = {
    name: "Untitled",
    tempo_bpm: 120,
    time_signature: { numerator: 4, denominator: 4 },
    tracks: [
      { id: 1, name: "Keys", instrument: defaultInstrument("synth", "Warm Keys") },
      { id: 2, name: "Bass", instrument: defaultInstrument("synth", "Fat Bass") },
      { id: 3, name: "Drums", instrument: defaultInstrument("drums", "Classic Kit") },
    ],
  };
  const undoStack: Project[] = [];
  const redoStack: Project[] = [];
  let playing = false;
  let metronome = true;
  let startedAt = 0;

  const view = (): ProjectView => ({
    project,
    can_undo: undoStack.length > 0,
    can_redo: redoStack.length > 0,
  });
  const audio = (): AudioStatus => ({
    output_devices: ["Preview (no audio)"],
    default_output: "Preview (no audio)",
    active_output: "Preview (no audio)",
    sample_rate_hz: 48000,
    error: null,
    midi_inputs: [],
  });
  const updateTrack = (trackId: number, f: (instrument: Instrument) => Instrument) => {
    project = {
      ...project,
      tracks: project.tracks.map((t) => (t.id === trackId ? { ...t, instrument: f(t.instrument) } : t)),
    };
  };
  const noop = async () => {};

  return {
    preview: true,
    appInfo: async () => ({ name: "Nunc Pro Tune", version: "preview", license: "GPL-3.0-or-later" }),
    catalog: async () => catalog,
    getProject: async () => view(),
    execute: async (command) => {
      undoStack.push(project);
      redoStack.length = 0;
      switch (command.command) {
        case "rename_project":
          project = { ...project, name: command.name };
          break;
        case "set_tempo":
          project = { ...project, tempo_bpm: command.bpm };
          break;
        case "set_time_signature":
          project = {
            ...project,
            time_signature: { numerator: command.numerator, denominator: command.denominator },
          };
          break;
        case "set_instrument_param":
          updateTrack(command.track_id, (i) => ({
            ...i,
            params: { ...i.params, [command.param]: command.value },
          }));
          break;
        case "load_preset":
          updateTrack(command.track_id, (i) => defaultInstrument(i.kind, command.preset));
          break;
        case "set_instrument":
          updateTrack(command.track_id, () => command.instrument);
          break;
      }
      return view();
    },
    endGesture: noop,
    undo: async () => {
      const previous = undoStack.pop();
      if (previous) {
        redoStack.push(project);
        project = previous;
      }
      return view();
    },
    redo: async () => {
      const next = redoStack.pop();
      if (next) {
        undoStack.push(project);
        project = next;
      }
      return view();
    },
    noteOn: noop,
    noteOff: noop,
    allNotesOff: noop,
    selectTrack: noop,
    play: async () => {
      if (!playing) startedAt = performance.now();
      playing = true;
    },
    stop: async () => {
      playing = false;
    },
    setMetronome: async (on) => {
      metronome = on;
    },
    transportStatus: async () => ({
      playing,
      position_beats: playing ? ((performance.now() - startedAt) / 60000) * project.tempo_bpm : 0,
      metronome_on: metronome,
      peak_left: 0,
      peak_right: 0,
      cpu_load: 0,
      buffer_frames: 480,
    }),
    audioStatus: async () => audio(),
    setOutputDevice: async () => audio(),
    refreshMidi: async () => audio(),
    onMidiNote: async () => () => {},
  };
}

export function defaultBackend(): Backend {
  return isTauri() ? tauriBackend : createPreviewBackend();
}
