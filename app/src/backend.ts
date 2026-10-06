import { invoke, isTauri } from "@tauri-apps/api/core";
import type { AppInfo, AudioStatus, Command, Project, ProjectView } from "./types";

/** Everything the UI can ask of the Rust side. */
export interface Backend {
  /** True when running in a plain browser with no engine behind it. */
  readonly preview: boolean;
  appInfo(): Promise<AppInfo>;
  getProject(): Promise<ProjectView>;
  execute(command: Command): Promise<ProjectView>;
  undo(): Promise<ProjectView>;
  redo(): Promise<ProjectView>;
  audioStatus(): Promise<AudioStatus>;
  setTestTone(on: boolean): Promise<AudioStatus>;
}

export const tauriBackend: Backend = {
  preview: false,
  appInfo: () => invoke("app_info"),
  getProject: () => invoke("get_project"),
  execute: (command) => invoke("execute", { command }),
  undo: () => invoke("undo"),
  redo: () => invoke("redo"),
  audioStatus: () => invoke("audio_status"),
  setTestTone: (on) => invoke("set_test_tone", { on }),
};

/**
 * In-memory stand-in so the UI can be developed and screenshotted in a
 * browser. It does not validate like the real engine and makes no sound.
 */
export function createPreviewBackend(): Backend {
  let project: Project = {
    name: "Untitled",
    tempo_bpm: 120,
    time_signature: { numerator: 4, denominator: 4 },
  };
  const undoStack: Project[] = [];
  const redoStack: Project[] = [];
  let toneOn = false;

  const view = (): ProjectView => ({
    project,
    can_undo: undoStack.length > 0,
    can_redo: redoStack.length > 0,
  });
  const audio = (): AudioStatus => ({
    output_devices: ["Preview (no audio)"],
    default_output: "Preview (no audio)",
    active_output: toneOn ? "Preview (no audio)" : null,
    sample_rate_hz: toneOn ? 48000 : null,
    test_tone_on: toneOn,
  });

  return {
    preview: true,
    appInfo: async () => ({ name: "Nunc Pro Tune", version: "preview", license: "GPL-3.0-or-later" }),
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
      }
      return view();
    },
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
    audioStatus: async () => audio(),
    setTestTone: async (on) => {
      toneOn = on;
      return audio();
    },
  };
}

export function defaultBackend(): Backend {
  return isTauri() ? tauriBackend : createPreviewBackend();
}
