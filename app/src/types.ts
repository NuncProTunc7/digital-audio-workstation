// Mirrors of the Rust types in crates/daw-model, crates/daw-instruments, and
// app/src-tauri. Keep in sync until Phase 3 generates these from schemas.

export interface TimeSignature {
  numerator: number;
  denominator: number;
}

export type InstrumentKind = "synth" | "drums";

export interface Instrument {
  kind: InstrumentKind;
  preset: string;
  params: Record<string, number>;
}

export interface Track {
  id: number;
  name: string;
  instrument: Instrument;
}

export interface Project {
  name: string;
  tempo_bpm: number;
  time_signature: TimeSignature;
  tracks: Track[];
}

export type Command =
  | { command: "rename_project"; name: string }
  | { command: "set_tempo"; bpm: number }
  | { command: "set_time_signature"; numerator: number; denominator: number }
  | { command: "set_instrument_param"; track_id: number; param: string; value: number }
  | { command: "load_preset"; track_id: number; preset: string }
  | { command: "set_instrument"; track_id: number; instrument: Instrument };

export interface ProjectView {
  project: Project;
  can_undo: boolean;
  can_redo: boolean;
}

export type Unit = "none" | "hertz" | "seconds" | "decibels" | "semitones" | "cents" | "percent";

export interface ParamSpec {
  id: string;
  name: string;
  group: string;
  min: number;
  max: number;
  default: number;
  unit: Unit;
  log_scale: boolean;
  choices: string[];
}

export interface Preset {
  name: string;
  description: string;
}

export interface InstrumentDescription {
  kind: InstrumentKind;
  params: ParamSpec[];
  presets: Preset[];
}

export interface DrumPad {
  note: number;
  name: string;
  group: string;
}

export interface Catalog {
  instruments: InstrumentDescription[];
  drum_pads: DrumPad[];
}

export interface TransportStatus {
  playing: boolean;
  position_beats: number;
  metronome_on: boolean;
  peak_left: number;
  peak_right: number;
  cpu_load: number;
  /** Sound card buffer size in frames; 0 until audio starts. */
  buffer_frames: number;
}

export interface AudioStatus {
  output_devices: string[];
  default_output: string | null;
  active_output: string | null;
  sample_rate_hz: number | null;
  error: string | null;
  midi_inputs: string[];
}

export interface AppInfo {
  name: string;
  version: string;
  license: string;
}
