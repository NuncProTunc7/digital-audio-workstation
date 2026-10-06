// Mirrors of the Rust types in crates/daw-model, crates/daw-instruments, and
// app/src-tauri. Keep in sync until Phase 3 generates these from schemas.

export interface TimeSignature {
  numerator: number;
  denominator: number;
}

export type InstrumentKind = "synth" | "drums" | "audio";
export type EffectKind = "eq" | "compressor" | "reverb" | "delay" | "chorus" | "distortion" | "limiter";

export interface Instrument {
  kind: InstrumentKind;
  preset: string;
  params: Record<string, number>;
}

export interface Effect {
  id: number;
  kind: EffectKind;
  enabled: boolean;
  params: Record<string, number>;
}

export interface Mixer {
  volume_db: number;
  pan: number;
  mute: boolean;
  solo: boolean;
  effects: Effect[];
}

export interface Note {
  id: number;
  pitch: number;
  start_beats: number;
  length_beats: number;
  velocity: number;
}

/** The part of an audio file an audio clip plays. */
export interface AudioRegion {
  /** File name inside the project's audio folder. */
  file: string;
  file_seconds: number;
  offset_seconds: number;
  gain_db: number;
  fade_in_seconds: number;
  fade_out_seconds: number;
}

export interface Clip {
  id: number;
  name: string;
  start_beats: number;
  length_beats: number;
  notes: Note[];
  /** Present on audio clips (audio tracks) only. */
  audio?: AudioRegion | null;
}

export interface Track {
  id: number;
  name: string;
  instrument: Instrument;
  mixer: Mixer;
  clips: Clip[];
}

export interface LoopRegion {
  enabled: boolean;
  start_beats: number;
  end_beats: number;
}

export interface Project {
  format_version: number;
  name: string;
  tempo_bpm: number;
  time_signature: TimeSignature;
  tracks: Track[];
  master: { volume_db: number; effects: Effect[] };
  loop_region: LoopRegion;
  next_id: number;
}

export interface NoteInput {
  pitch: number;
  start_beats: number;
  length_beats: number;
  velocity?: number;
  id?: number;
}

export interface NoteEdit {
  id: number;
  pitch?: number | null;
  start_beats?: number | null;
  length_beats?: number | null;
  velocity?: number | null;
}

type Opt<T> = T | null;

export type Command =
  | { command: "rename_project"; name: string }
  | { command: "set_tempo"; bpm: number }
  | { command: "set_time_signature"; numerator: number; denominator: number }
  | { command: "set_loop"; enabled: Opt<boolean>; start_beats: Opt<number>; end_beats: Opt<number> }
  | { command: "add_track"; name: string; instrument: InstrumentKind; preset: Opt<string>; index: Opt<number> }
  | { command: "remove_track"; track_id: number }
  | { command: "rename_track"; track_id: number; name: string }
  | { command: "move_track"; track_id: number; index: number }
  | {
      command: "set_track_mixer";
      track_id: number;
      volume_db: Opt<number>;
      pan: Opt<number>;
      mute: Opt<boolean>;
      solo: Opt<boolean>;
    }
  | { command: "set_master_volume"; volume_db: number }
  | { command: "set_instrument_param"; track_id: number; param: string; value: number }
  | { command: "load_preset"; track_id: number; preset: string }
  | { command: "set_instrument"; track_id: number; instrument: Instrument }
  | { command: "add_effect"; track_id: Opt<number>; kind: EffectKind; index: Opt<number> }
  | { command: "remove_effect"; track_id: Opt<number>; effect_id: number }
  | { command: "set_effect_param"; track_id: Opt<number>; effect_id: number; param: string; value: number }
  | { command: "set_effect_enabled"; track_id: Opt<number>; effect_id: number; enabled: boolean }
  | {
      command: "create_clip";
      track_id: number;
      start_beats: number;
      length_beats: number;
      name: Opt<string>;
      notes: NoteInput[];
    }
  | { command: "delete_clip"; clip_id: number }
  | { command: "move_clip"; clip_id: number; start_beats: Opt<number>; track_id: Opt<number> }
  | { command: "resize_clip"; clip_id: number; length_beats: number }
  | { command: "rename_clip"; clip_id: number; name: string }
  | { command: "duplicate_clip"; clip_id: number; start_beats: Opt<number> }
  | { command: "add_notes"; clip_id: number; notes: NoteInput[] }
  | { command: "remove_notes"; clip_id: number; note_ids: number[] }
  | { command: "edit_notes"; clip_id: number; edits: NoteEdit[] }
  | {
      command: "quantize_notes";
      clip_id: number;
      grid_beats: number;
      strength: Opt<number>;
      lengths: boolean;
      note_ids: Opt<number[]>;
    }
  | { command: "transpose_notes"; clip_id: number; semitones: number; note_ids: Opt<number[]> }
  | { command: "split_clip"; clip_id: number; at_beats: number }
  | { command: "trim_clip_start"; clip_id: number; start_beats: number }
  | {
      command: "add_audio_clip";
      track_id: number;
      start_beats: number;
      audio: AudioRegion;
      length_beats: Opt<number>;
      name: Opt<string>;
    }
  | {
      command: "set_audio_clip";
      clip_id: number;
      gain_db: Opt<number>;
      fade_in_seconds: Opt<number>;
      fade_out_seconds: Opt<number>;
    }
  | { command: "batch"; commands: Command[] };

export interface ProjectView {
  project: Project;
  can_undo: boolean;
  can_redo: boolean;
  dirty: boolean;
  file_path: string | null;
  /** Audio files clips refer to that can't be found (those clips are silent). */
  missing_audio: string[];
}

/** Waveform overview of an audio file. */
export interface Peaks {
  per_second: number;
  /** Interleaved [min, max, min, max, ...], one pair per bucket. */
  min_max: number[];
  /** Loudest sample (1.0 = full scale). */
  peak: number;
  seconds: number;
}

export interface InputStatus {
  devices: string[];
  default_device: string | null;
  /** The open microphone, if any. */
  active: string | null;
  sample_rate_hz: number | null;
  /** Peak level since the last poll, 0–1. */
  level: number;
  error: string | null;
}

export type Unit = "none" | "hertz" | "seconds" | "decibels" | "semitones" | "cents" | "percent" | "ratio";

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

export interface EffectDescription {
  kind: EffectKind;
  name: string;
  description: string;
  params: ParamSpec[];
}

export interface DrumPad {
  note: number;
  name: string;
  group: string;
}

export interface Catalog {
  instruments: InstrumentDescription[];
  effects: EffectDescription[];
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
  /** Peak level per track, in track order. */
  track_peaks: number[];
  recording: boolean;
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

/** One project change Claude made through the MCP bridge. */
export interface RemoteActivity {
  description: string;
  /** Milliseconds since the Unix epoch. */
  at_ms: number;
}

/** Whether Claude can connect, and how to set it up. */
export interface ClaudeStatus {
  /** The control server is running, so Claude can connect. */
  listening: boolean;
  error: string | null;
  /** Seconds since Claude last did something (null = not this session). */
  last_activity_secs: number | null;
  /** Newest first. */
  activity: RemoteActivity[];
  bridge_path: string;
  bridge_found: boolean;
  desktop_config_path: string;
  desktop_configured: boolean;
  claude_code_command: string;
}
