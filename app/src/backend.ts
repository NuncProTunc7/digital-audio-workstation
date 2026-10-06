import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ask, open, save } from "@tauri-apps/plugin-dialog";
import { createPreviewBackend } from "./preview";
import type { AppInfo, AudioStatus, Catalog, Command, ProjectView, TransportStatus } from "./types";

export { createPreviewBackend };

const PROJECT_FILTER = [{ name: "Nunc Pro Tune project", extensions: ["nptune"] }];

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

  // Files.
  newProject(): Promise<ProjectView>;
  openProject(path: string): Promise<ProjectView>;
  /** Saves to `path`, or to the current file when null. */
  saveProject(path: string | null): Promise<ProjectView>;
  /** Shows an open dialog; null if cancelled. */
  pickOpenPath(): Promise<string | null>;
  /** Shows a save dialog; null if cancelled. */
  pickSavePath(defaultName: string): Promise<string | null>;
  confirm(message: string): Promise<boolean>;
  setTitle(title: string): Promise<void>;
  /** Runs before the window closes; returning false keeps it open. */
  onCloseRequested(allowClose: () => Promise<boolean>): Promise<() => void>;

  // Live actions.
  noteOn(trackId: number, note: number, velocity: number): Promise<void>;
  noteOff(trackId: number, note: number): Promise<void>;
  allNotesOff(): Promise<void>;
  selectTrack(trackId: number): Promise<void>;
  play(): Promise<void>;
  stop(): Promise<void>;
  locate(beats: number): Promise<void>;
  recordStart(trackId: number): Promise<void>;
  /** Stops recording; the returned project contains the new clip. */
  recordStop(): Promise<ProjectView>;
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
  newProject: () => invoke("new_project"),
  openProject: (path) => invoke("open_project", { path }),
  saveProject: (path) => invoke("save_project", { path }),
  pickOpenPath: async () => {
    const picked = await open({ multiple: false, directory: false, filters: PROJECT_FILTER });
    return typeof picked === "string" ? picked : null;
  },
  pickSavePath: async (defaultName) => (await save({ defaultPath: `${defaultName}.nptune`, filters: PROJECT_FILTER })) ?? null,
  confirm: (message) => ask(message, { title: "Nunc Pro Tune", kind: "warning" }),
  setTitle: (title) => getCurrentWindow().setTitle(title),
  onCloseRequested: (allowClose) =>
    getCurrentWindow().onCloseRequested(async (event) => {
      if (!(await allowClose())) event.preventDefault();
    }),
  noteOn: (trackId, note, velocity) => invoke("note_on", { trackId, note, velocity }),
  noteOff: (trackId, note) => invoke("note_off", { trackId, note }),
  allNotesOff: () => invoke("all_notes_off"),
  selectTrack: (trackId) => invoke("select_track", { trackId }),
  play: () => invoke("play"),
  stop: () => invoke("stop"),
  locate: (beats) => invoke("locate", { beats }),
  recordStart: (trackId) => invoke("record_start", { trackId }),
  recordStop: () => invoke("record_stop"),
  setMetronome: (on) => invoke("set_metronome", { on }),
  transportStatus: () => invoke("transport_status"),
  audioStatus: () => invoke("audio_status"),
  setOutputDevice: (name) => invoke("set_output_device", { name }),
  refreshMidi: () => invoke("refresh_midi"),
  onMidiNote: async (callback) =>
    listen<{ note: number; on: boolean }>("midi-note", (e) => callback(e.payload.note, e.payload.on)),
};

export function defaultBackend(): Backend {
  return isTauri() ? tauriBackend : createPreviewBackend();
}
