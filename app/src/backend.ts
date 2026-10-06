import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ask, open, save } from "@tauri-apps/plugin-dialog";
import { createPreviewBackend } from "./preview";
import type {
  AppInfo,
  AudioStatus,
  CalibrationResult,
  Catalog,
  ClaudeStatus,
  Command,
  ExportReport,
  GodotOptions,
  InputStatus,
  Peaks,
  ProjectView,
  RecordingDelay,
  Recoverable,
  SamplePackStatus,
  TransportStatus,
} from "./types";

export { createPreviewBackend };

const PROJECT_FILTER = [{ name: "Nunc Pro Tune project", extensions: ["nptune"] }];
/** What the audio importer reads (see daw_audio::SUPPORTED_EXTENSIONS). */
export const AUDIO_EXTENSIONS = ["wav", "wave", "mp3", "m4a", "mp4", "aac", "flac", "ogg", "oga"];
const AUDIO_FILTER = [{ name: "Audio (WAV, MP3, M4A, FLAC, OGG)", extensions: AUDIO_EXTENSIONS }];
export const MIDI_EXTENSIONS = ["mid", "midi"];
export const MUSICXML_EXTENSIONS = ["musicxml", "xml", "mxl"];
/** Things the export menu can write, with their file extension. */
export type ExportKind = "wav" | "mid" | "musicxml";
const EXPORT_FILTERS: Record<ExportKind, { name: string; extensions: string[] }[]> = {
  wav: [{ name: "WAV audio", extensions: ["wav"] }],
  mid: [{ name: "MIDI file", extensions: ["mid"] }],
  musicxml: [{ name: "Sheet music (MusicXML)", extensions: ["musicxml"] }],
};

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

  // Audio files and the microphone.
  /** Imports a file onto `trackId` (null: a new audio track) at `startBeats` (null: 0). */
  importAudio(path: string, trackId: number | null, startBeats: number | null): Promise<ProjectView>;
  /** Shows an open dialog for audio files; empty if cancelled. */
  pickAudioFiles(): Promise<string[]>;
  audioPeaks(file: string): Promise<Peaks>;
  inputStatus(): Promise<InputStatus>;
  /** Opens (true) or closes (false) the microphone for metering. */
  monitorInput(on: boolean): Promise<InputStatus>;
  setInputDevice(name: string | null): Promise<InputStatus>;
  /** Sets how late the current microphone's recordings arrive (ms). */
  setRecordingOffset(ms: number): Promise<RecordingDelay>;
  /** Plays clicks for the user to clap along with (about 10 s); measures and keeps the delay. */
  calibrateRecording(): Promise<CalibrationResult>;
  /** Unsaved work from a run that didn't close properly, if any. */
  recoveryCheck(): Promise<Recoverable | null>;
  /** Opens the recovered work (true) or deletes it (false). */
  recoveryResolve(recover: boolean): Promise<ProjectView>;
  /** Shows an open dialog for anything importable (audio, MIDI, sheet music); empty if cancelled. */
  pickImportFiles(): Promise<string[]>;
  /** Reads a MIDI file into new tracks. */
  importMidi(path: string): Promise<ProjectView>;
  /** Shows a save dialog for an export; null if cancelled. */
  pickExportPath(kind: ExportKind, defaultName: string): Promise<string | null>;
  exportWav(path: string): Promise<void>;
  exportMidi(path: string): Promise<void>;
  /** Reads sheet music (.musicxml, .xml, .mxl) into new tracks. */
  importMusicXml(path: string): Promise<ProjectView>;
  exportMusicXml(path: string, trackIds: number[] | null): Promise<void>;
  /** Shows an open dialog for an .sfz sample pack; null if cancelled. */
  pickSamplePack(): Promise<string | null>;
  samplePackStatus(path: string): Promise<SamplePackStatus>;
  /** Shows a folder picker; null if cancelled. */
  pickFolder(title: string): Promise<string | null>;
  exportGodot(options: GodotOptions): Promise<ExportReport>;
  /** The song (or some tracks) as MusicXML, for the sheet music view. */
  sheetMusic(trackIds: number[] | null): Promise<string>;
  /** Files dropped on the window, with the drop point in CSS pixels. */
  onFileDrop(callback: (paths: string[], x: number, y: number) => void): Promise<() => void>;

  // Claude.
  claudeStatus(): Promise<ClaudeStatus>;
  /** Adds Nunc Pro Tune to Claude Desktop's settings. */
  claudeInstallDesktop(): Promise<ClaudeStatus>;
  /** Calls back when Claude changes the project, with a short description. */
  onProjectChanged(callback: (description: string) => void): Promise<() => void>;
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
  importAudio: (path, trackId, startBeats) => invoke("import_audio", { path, trackId, startBeats }),
  pickAudioFiles: async () => {
    const picked = await open({ multiple: true, directory: false, filters: AUDIO_FILTER });
    if (picked === null) return [];
    return Array.isArray(picked) ? picked : [picked];
  },
  audioPeaks: (file) => invoke("audio_peaks", { file }),
  pickImportFiles: async () => {
    const picked = await open({
      multiple: true,
      directory: false,
      filters: [
        {
          name: "Audio, MIDI, or sheet music",
          extensions: [...AUDIO_EXTENSIONS, ...MIDI_EXTENSIONS, ...MUSICXML_EXTENSIONS],
        },
        ...AUDIO_FILTER,
        { name: "MIDI file", extensions: MIDI_EXTENSIONS },
        { name: "Sheet music (MusicXML)", extensions: MUSICXML_EXTENSIONS },
      ],
    });
    if (picked === null) return [];
    return Array.isArray(picked) ? picked : [picked];
  },
  importMidi: (path) => invoke("import_midi", { path }),
  pickExportPath: async (kind, defaultName) =>
    (await save({ defaultPath: `${defaultName}.${kind}`, filters: EXPORT_FILTERS[kind] })) ?? null,
  exportWav: (path) => invoke("export_wav", { path }),
  exportMidi: (path) => invoke("export_midi", { path }),
  importMusicXml: (path) => invoke("import_musicxml", { path }),
  exportMusicXml: (path, trackIds) => invoke("export_musicxml", { path, trackIds }),
  sheetMusic: (trackIds) => invoke("sheet_music", { trackIds }),
  pickSamplePack: async () => {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "SFZ sample pack", extensions: ["sfz"] }],
    });
    return typeof picked === "string" ? picked : null;
  },
  samplePackStatus: (path) => invoke("sample_pack_status", { path }),
  pickFolder: async (title) => {
    const picked = await open({ directory: true, multiple: false, title });
    return typeof picked === "string" ? picked : null;
  },
  exportGodot: (options) => invoke("export_godot", { options }),
  inputStatus: () => invoke("input_status"),
  monitorInput: (on) => invoke("monitor_input", { on }),
  setInputDevice: (name) => invoke("set_input_device", { name }),
  setRecordingOffset: (ms) => invoke("set_recording_offset", { ms }),
  calibrateRecording: () => invoke("calibrate_recording"),
  recoveryCheck: () => invoke("recovery_check"),
  recoveryResolve: (recover) => invoke("recovery_resolve", { recover }),
  onFileDrop: (callback) =>
    getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type !== "drop") return;
      const scale = window.devicePixelRatio || 1;
      callback(e.payload.paths, e.payload.position.x / scale, e.payload.position.y / scale);
    }),
  claudeStatus: () => invoke("claude_status"),
  claudeInstallDesktop: () => invoke("claude_install_desktop"),
  onProjectChanged: async (callback) => listen<string>("project-changed", (e) => callback(e.payload)),
};

export function defaultBackend(): Backend {
  return isTauri() ? tauriBackend : createPreviewBackend();
}
