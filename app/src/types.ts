// Mirrors of the Rust types in crates/daw-model and app/src-tauri.
// Keep in sync until Phase 3 generates these from the Command JSON Schema.

export interface TimeSignature {
  numerator: number;
  denominator: number;
}

export interface Project {
  name: string;
  tempo_bpm: number;
  time_signature: TimeSignature;
}

export type Command =
  | { command: "rename_project"; name: string }
  | { command: "set_tempo"; bpm: number }
  | { command: "set_time_signature"; numerator: number; denominator: number };

export interface ProjectView {
  project: Project;
  can_undo: boolean;
  can_redo: boolean;
}

export interface AudioStatus {
  output_devices: string[];
  default_output: string | null;
  active_output: string | null;
  sample_rate_hz: number | null;
  test_tone_on: boolean;
}

export interface AppInfo {
  name: string;
  version: string;
  license: string;
}
