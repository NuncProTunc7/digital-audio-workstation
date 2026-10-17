// Mirrors of the Rust types in crates/daw-model, crates/daw-instruments, and
// app/src-tauri. Keep in sync until Phase 3 generates these from schemas.

export interface TimeSignature {
  numerator: number;
  denominator: number;
}

export type InstrumentKind = "synth" | "drums" | "audio" | "sampler" | "plugin" | "guitar";
export type EffectKind = "eq" | "compressor" | "reverb" | "delay" | "chorus" | "distortion" | "limiter" | "plugin";

export interface Instrument {
  kind: InstrumentKind;
  preset: string;
  params: Record<string, number>;
  /** Samplers: absolute path of the SFZ file. */
  sample_pack?: string | null;
  /** Plugin instruments: which VST3 plugin, and its settings. */
  plugin?: PluginRef | null;
}

/** A third-party VST3 plugin a track uses (daw-model/src/plugin.rs). */
export interface PluginRef {
  uid: string;
  name: string;
  vendor: string;
  path: string;
  /** Parameter id → value (0–1). */
  params: Record<string, number>;
  state?: string | null;
}

/** Whether a sampler's pack has loaded. */
export interface SamplePackStatus {
  state: "not_loaded" | "loading" | "ready" | "failed";
  name?: string;
  zones?: number;
  megabytes?: number;
  layers_kept?: number;
  layers_total?: number;
  /** Lowest and highest notes that play something (MIDI), when ready. */
  lowest_note?: number;
  highest_note?: number;
  error?: string;
}

export interface Effect {
  id: number;
  kind: EffectKind;
  enabled: boolean;
  params: Record<string, number>;
  /** Compressors: the track whose sound drives it (sidechain). */
  sidechain?: number | null;
  /** Plugin effects: which VST3 plugin, and its settings. */
  plugin?: PluginRef | null;
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
  /** How often it plays, 1–100 % (missing = 100, every time). */
  chance?: number;
}

/** Shuffle: every second `grid_beats` step plays late. */
export interface Swing {
  /** 0 = straight, ~67 = triplet feel, 100 = hardest. */
  amount_percent: number;
  grid_beats: number;
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
  /** Set when the clip follows the song tempo: the tempo it was recorded at. */
  source_bpm?: number | null;
}

export interface Clip {
  id: number;
  name: string;
  start_beats: number;
  length_beats: number;
  notes: Note[];
  /** Present on audio clips (audio tracks) only. */
  audio?: AudioRegion | null;
  /** Shuffle on note clips (missing = straight). */
  swing?: Swing | null;
  /** Kept but silent (an unused take). */
  muted?: boolean;
  /** Linked clips share this id and keep the same notes. */
  link?: number | null;
}

export type AutomationTarget =
  | { kind: "volume" }
  | { kind: "pan" }
  | { kind: "instrument_param"; param: string }
  | { kind: "effect_param"; effect_id: number; param: string };

export interface AutomationPoint {
  beats: number;
  value: number;
}

/** A curve that moves one setting over time. */
export interface AutomationLane {
  id: number;
  target: AutomationTarget;
  enabled: boolean;
  /** Sorted by beats. */
  points: AutomationPoint[];
}

export interface Track {
  id: number;
  name: string;
  instrument: Instrument;
  mixer: Mixer;
  clips: Clip[];
  automation?: AutomationLane[];
  /** A rendering played instead of the instrument and effects (saves CPU). */
  frozen?: { file: string; fingerprint: number } | null;
  /** The bus it plays into (missing/null = the master). */
  output?: number | null;
  /** Extra feeds into buses. */
  sends?: Send[];
}

/** Part of a track's sound sent to a bus. */
export interface Send {
  bus_id: number;
  level_db: number;
  pre_fader?: boolean;
}

/** A group bus: tracks play into it; it plays into the master. */
export interface Bus {
  id: number;
  name: string;
  mixer: Mixer;
}

export interface LoopRegion {
  enabled: boolean;
  start_beats: number;
  end_beats: number;
}

/** From `beats` on, the song plays at `bpm` (after the starting tempo). */
export interface TempoChange {
  id: number;
  beats: number;
  bpm: number;
  /** Glide from here to the next change's tempo. */
  ramp?: boolean;
}

export interface Project {
  format_version: number;
  name: string;
  /** The starting tempo. */
  tempo_bpm: number;
  /** Later tempo changes, in beat order. */
  tempo_changes?: TempoChange[];
  time_signature: TimeSignature;
  tracks: Track[];
  master: { volume_db: number; effects: Effect[] };
  loop_region: LoopRegion;
  next_id: number;
  /** Saved versions of the song. */
  snapshots?: Snapshot[];
  /** Section markers, in time order. */
  markers?: Marker[];
  /** Group buses. */
  buses?: Bus[];
  key?: Key | null;
  /** The chord track, in time order. */
  chords?: Chord[];
  /** Game sound effects, exported to Godot as files. */
  sounds?: SoundEffect[];
}

/** The basic tone of a sound effect. */
export type Wave = "square" | "saw" | "sine" | "triangle" | "noise";

/** A kind of sound effect to generate. */
export type SoundCategory = "coin" | "jump" | "laser" | "explosion" | "hit" | "power_up" | "blip" | "random";

/** How to make a sound effect (sfxr-style; 0..1, or -1..1 for ramps and the pitch jump). */
export interface SoundRecipe {
  wave: Wave;
  attack: number;
  sustain: number;
  punch: number;
  decay: number;
  base_freq: number;
  freq_limit: number;
  freq_ramp: number;
  freq_dramp: number;
  vib_strength: number;
  vib_speed: number;
  arp_mod: number;
  arp_speed: number;
  duty: number;
  duty_ramp: number;
  repeat_speed: number;
  phaser_offset: number;
  phaser_ramp: number;
  lpf_freq: number;
  lpf_ramp: number;
  lpf_resonance: number;
  hpf_freq: number;
  hpf_ramp: number;
  crush: number;
  /** -24..+6 dB. */
  volume_db: number;
}

/** Options for writing sound effects into a Godot project. */
export interface SoundExportOptions {
  project_dir: string;
  /** Folder inside the project (default "sfx"). */
  folder?: string | null;
  /** Only these sounds (default: all). */
  sound_ids?: number[] | null;
}

/** One sound effect file written. */
export interface SoundFile {
  sound_id: number;
  res_path: string;
  seconds: number;
  peak_dbfs: number;
  /** How much it was turned down so it doesn't clip (0 = not at all). */
  turned_down_db: number;
}

export interface SoundExportReport {
  files: SoundFile[];
  /** Files an earlier export wrote for these sounds before a rename (kept: a scene may use them). */
  left_behind: string[];
}

/** One game sound effect. */
export interface SoundEffect {
  id: number;
  name: string;
  recipe: SoundRecipe;
}

export type Mode =
  | "major"
  | "minor"
  | "dorian"
  | "phrygian"
  | "lydian"
  | "mixolydian"
  | "harmonic_minor"
  | "major_pentatonic"
  | "minor_pentatonic";

/** The song's key: tonic pitch class (0 = C) and mode. */
export interface Key {
  tonic: number;
  mode: Mode;
}

export type ChordQuality =
  | "major"
  | "minor"
  | "diminished"
  | "augmented"
  | "sus2"
  | "sus4"
  | "major7"
  | "minor7"
  | "dominant7"
  | "half_diminished7"
  | "power";

/** One chord on the chord track; lasts until the next. */
export interface Chord {
  id: number;
  start_beats: number;
  root: number;
  quality: ChordQuality;
  bass?: number | null;
}

/** A named point where a section starts. */
export interface Marker {
  id: number;
  name: string;
  start_beats: number;
}

/** The music a saved version keeps. */
export interface SongState {
  tempo_bpm: number;
  tempo_changes?: TempoChange[];
  time_signature: TimeSignature;
  tracks: Track[];
  master: { volume_db: number; effects: Effect[] };
  loop_region: LoopRegion;
  markers?: Marker[];
}

export interface Snapshot {
  id: number;
  name: string;
  song: SongState;
}

export type CompareSide = "current" | "version";

/** A sound the user saved under a name. */
/** A download in progress, or the last one's failure. */
export type LibraryJob =
  | { state: "downloading"; bytes: number; total: number | null }
  | { state: "unpacking" }
  | { state: "failed"; error: string };

/** A free instrument the app can download (crates/daw-control/src/library.rs). */
/** An installed VST3 plugin (crates/daw-plugins PluginInfo). */
export interface PluginInfo {
  uid: string;
  name: string;
  vendor: string;
  version: string;
  kind: "instrument" | "effect";
  categories: string;
  path: string;
}

/** Installed plugins, and ones that couldn't be used. */
export interface PluginList {
  plugins: PluginInfo[];
  could_not_use: { path: string; error: string }[];
}

/** One of a plugin's parameters (crates/daw-plugins ParamInfo). */
export interface PluginParam {
  id: number;
  name: string;
  units: string;
  /** 0–1. */
  value: number;
  default: number;
  /** What the plugin shows for the value, e.g. "-6.0 dB". */
  display: string;
  /** 0 = continuous. */
  steps: number;
  automatable: boolean;
}

/** Whether a track's plugin has loaded. */
export type PluginStatus =
  | { state: "none" | "loading" | "ready" }
  | { state: "failed"; error: string };

export interface LibraryPack {
  id: string;
  name: string;
  description: string;
  license: string;
  credit: string;
  /** (name, .sfz path) once downloaded. */
  programs: [string, string][];
  megabytes: number;
  installed: [string, string][] | null;
  job: LibraryJob | null;
}

export interface UserPreset {
  name: string;
  kind: InstrumentKind;
  params: Record<string, number>;
  sample_pack?: string | null;
}

/** A part of the song between markers. */
export interface SongSection {
  name: string;
  start_beats: number;
  end_beats: number;
}

/** What the game preview can switch and fade. */
export interface PreviewPlan {
  sections: SongSection[];
  layers: { track_id: number; name: string }[];
}

/** Loudness of the song and a saved version, and how they are matched. */
export interface Comparison {
  snapshot_id: number;
  name: string;
  current_lufs: number | null;
  version_lufs: number | null;
  current_gain_db: number;
  version_gain_db: number;
}

export interface NoteInput {
  pitch: number;
  start_beats: number;
  length_beats: number;
  velocity?: number;
  /** 1–100 % (default 100). */
  chance?: number;
  id?: number;
}

export interface NoteEdit {
  id: number;
  pitch?: number | null;
  start_beats?: number | null;
  length_beats?: number | null;
  velocity?: number | null;
  chance?: number | null;
}

type Opt<T> = T | null;

export type Command =
  | { command: "rename_project"; name: string }
  | { command: "set_tempo"; bpm: number }
  | { command: "add_tempo_change"; beats: number; bpm: number; ramp?: boolean }
  | { command: "move_tempo_change"; tempo_change_id: number; beats: number }
  | { command: "set_tempo_change"; tempo_change_id: number; bpm: Opt<number>; ramp: Opt<boolean> }
  | { command: "remove_tempo_change"; tempo_change_id: number }
  | { command: "restore_tempo_change"; change: TempoChange }
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
  | { command: "add_plugin_effect"; track_id: Opt<number>; plugin: PluginRef; index: Opt<number> }
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
  | { command: "set_clip_swing"; clip_id: number; swing: Swing | null }
  | { command: "set_clip_muted"; clip_id: number; muted: boolean }
  | { command: "comp_take"; clip_id: number }
  | { command: "set_effect_sidechain"; track_id: number | null; effect_id: number; source: number | null }
  | {
      command: "humanize_notes";
      clip_id: number;
      timing_beats: number;
      velocity: number;
      seed: number;
      note_ids: number[] | null;
    }
  | { command: "set_key"; key: Key | null }
  | { command: "add_chord"; start_beats: number; root: number; quality: ChordQuality; bass: number | null }
  | { command: "set_chord"; chord_id: number; root: number; quality: ChordQuality; bass: number | null }
  | { command: "move_chord"; chord_id: number; start_beats: number }
  | { command: "remove_chord"; chord_id: number }
  | { command: "restore_chord"; chord: Chord }
  | { command: "add_bus"; name: string }
  | { command: "remove_bus"; bus_id: number }
  | { command: "rename_bus"; bus_id: number; name: string }
  | { command: "set_bus_mixer"; bus_id: number; volume_db: number | null; pan: number | null; mute: boolean | null }
  | { command: "set_track_output"; track_id: number; bus_id: number | null }
  | { command: "freeze_track"; track_id: number; frozen: { file: string; fingerprint: number } }
  | { command: "unfreeze_track"; track_id: number }
  | { command: "set_send"; track_id: number; bus_id: number; level_db: number | null; pre_fader: boolean | null }
  | { command: "remove_send"; track_id: number; bus_id: number }
  | { command: "add_marker"; name: string; start_beats: number }
  | { command: "move_marker"; marker_id: number; start_beats: number }
  | { command: "rename_marker"; marker_id: number; name: string }
  | { command: "remove_marker"; marker_id: number }
  | { command: "restore_marker"; marker: Marker }
  | {
      command: "add_sound";
      name?: string | null;
      recipe?: Partial<SoundRecipe> | null;
      category?: SoundCategory | null;
      seed?: number | null;
    }
  /** Only the settings given change. */
  | { command: "set_sound"; sound_id: number; recipe: Partial<SoundRecipe> }
  | { command: "vary_sound"; sound_id: number; seed: number }
  | { command: "rename_sound"; sound_id: number; name: string }
  | { command: "remove_sound"; sound_id: number }
  | { command: "restore_sound"; sound: SoundEffect; index: number }
  | { command: "take_snapshot"; name: string }
  | { command: "load_snapshot"; snapshot_id: number }
  | { command: "rename_snapshot"; snapshot_id: number; name: string }
  | { command: "delete_snapshot"; snapshot_id: number }
  | { command: "restore_snapshot"; snapshot: Snapshot; index: number }
  | { command: "set_song_state"; song: SongState }
  | { command: "duplicate_clip"; clip_id: number; start_beats: Opt<number>; linked?: boolean }
  | { command: "link_clips"; clip_ids: number[] }
  | { command: "unlink_clip"; clip_id: number }
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
  /** Bars count from 1 like the ruler; every track, marker, chord and automation point moves. */
  | { command: "insert_bars"; at_bar: number; bars: number }
  | { command: "delete_bars"; from_bar: number; bars: number }
  | { command: "repeat_bars"; from_bar: number; bars: number; times?: number }
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
  | { command: "set_clip_tempo"; clip_id: number; source_bpm: number | null }
  | { command: "load_sample_pack"; track_id: number; path: string | null }
  | { command: "set_plugin_params"; track_id: number; effect_id?: number | null; params: Record<string, number> }
  | { command: "add_automation_lane"; track_id: number; target: AutomationTarget; points: AutomationPoint[] }
  | { command: "remove_automation_lane"; track_id: number; lane_id: number }
  | { command: "set_automation_points"; track_id: number; lane_id: number; points: AutomationPoint[] }
  | { command: "set_automation_enabled"; track_id: number; lane_id: number; enabled: boolean }
  | { command: "batch"; commands: Command[] };

export interface ProjectView {
  project: Project;
  can_undo: boolean;
  can_redo: boolean;
  dirty: boolean;
  file_path: string | null;
  /** Audio files clips refer to that can't be found (those clips are silent). */
  missing_audio: string[];
  /** Frozen tracks whose rendering is up to date (others play live). */
  frozen_current?: number[];
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
  /** How late this microphone's recordings arrive, and the correction. */
  delay: RecordingDelay | null;
}

/** A microphone's recording delay correction (mirrors `daw_control::RecordingDelay`). */
export interface RecordingDelay {
  device: string | null;
  /** Takes are moved this much earlier (ms). */
  offset_ms: number;
  /** The device looks like a Bluetooth headset. */
  bluetooth: boolean;
}

/** What clapping along measured (mirrors `daw_control::CalibrationResult`). */
export interface CalibrationResult {
  offset_ms: number;
  claps: number;
  spread_ms: number;
  /** Steady enough, and now in use. */
  saved: boolean;
  device: string | null;
}

/** Unsaved work left by a run that didn't close properly. */
export interface Recoverable {
  name: string;
  project_path: string | null;
  /** Milliseconds since the Unix epoch. */
  saved_at_ms: number;
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
  /** Guitar tunings in the order of the `string.tuning` choice. */
  guitar_tunings: GuitarTuning[];
  guitar_frets: number;
}

/** A guitar tuning: open strings as MIDI notes, lowest first. */
export interface GuitarTuning {
  name: string;
  strings: number[];
}

/** What the window has selected, reported so an AI can act on "this"
 * (mirrors daw_control::Selection). A live view, not a project edit. */
export interface Selection {
  /** The open tab ("instrument", "pianoroll", "sheet", "mixer", "sounds"). */
  view: string | null;
  track_id: number | null;
  clip_id: number | null;
  /** Notes selected in the piano roll. */
  note_ids: number[];
  /** The sound selected in the Sound FX panel, while it is open. */
  sound_id: number | null;
}

export interface TransportStatus {
  playing: boolean;
  position_beats: number;
  /** Beats of count-in left before the song starts (0 when not counting in). */
  count_in_beats: number;
  /** Bars of clicks before recording starts (0-2). */
  count_in_bars: number;
  metronome_on: boolean;
  peak_left: number;
  peak_right: number;
  cpu_load: number;
  /** Sound card buffer size in frames; 0 until audio starts. */
  buffer_frames: number;
  /** Times the computer couldn't keep up since audio started (heard as crackles). */
  overloads: number;
  /** CPU near its limit, or a crackle in the last few seconds: suggest a bigger buffer. */
  struggling: boolean;
  /** A/B listening against a saved version. */
  comparing: { snapshot_id: number; side: CompareSide } | null;
  /** Game preview: where a queued section change happens (beats). */
  jump_at_beats?: number | null;
  /** Peak level per track, in track order. */
  track_peaks: number[];
  /** Peak level per bus, in bus order. */
  bus_peaks?: number[];
  recording: boolean;
}

export interface AudioStatus {
  output_devices: string[];
  default_output: string | null;
  active_output: string | null;
  sample_rate_hz: number | null;
  /** Buffer size asked for, in frames (null = the device's default). */
  buffer_setting: number | null;
  /** Fixed buffer size in use (null = the device's default). */
  buffer_active: number | null;
  buffer_options: number[];
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
/** Where AI apps that take a web address connect (this computer only). */
export interface WebAddress {
  url: string;
  key: string;
  /** The address with the key in it. */
  url_with_key: string;
  /** Offers the short tool list for small models. */
  core: boolean;
}

export interface AiStatus {
  /** The control server is running, so an AI can connect. */
  listening: boolean;
  error: string | null;
  /** Seconds since an AI last did something (null = not this session). */
  last_activity_secs: number | null;
  /** The AI that last did something ("Claude", "Codex"...). */
  last_ai: string | null;
  /** Newest first. */
  activity: RemoteActivity[];
  bridge_path: string;
  bridge_found: boolean;
  desktop_config_path: string;
  desktop_configured: boolean;
  claude_code_command: string;
  codex_config_path: string;
  codex_configured: boolean;
  codex_command: string;
  /** `mcpServers` JSON for any other MCP app; `_core` adds the short tool list for small local models. */
  generic_config: string;
  generic_config_core: string;
  /** The web address for apps that take one, while web mode is on. */
  web?: WebAddress | null;
}

/** Options for exporting into a Godot project (mirrors daw_control::GodotOptions). */
export interface GodotOptions {
  project_dir: string;
  folder: string | null;
  name: string | null;
  format: "ogg" | "wav" | null;
  start_beats: number | null;
  end_beats: number | null;
  looped: boolean | null;
  /** With stems: one stem per bus instead of per track. */
  bus_stems?: boolean | null;
  /** With looped: start at the song start and loop only the region (an intro). */
  intro?: boolean | null;
  /** Export the song's marker sections as an AudioStreamInteractive. */
  sections_from_markers?: boolean | null;
  stems: boolean | null;
  layers: boolean | null;
  target_lufs: number | null;
  normalize: boolean | null;
  /** Layers (track names, or bus names and "other") that start silent in the layers resource. */
  silent_layers?: string[];
}

/** One thing the export check found. */
export interface Finding {
  level: "problem" | "warning" | "ok";
  message: string;
}

/** A frequency band's share of the mix's energy. */
export interface AnalysisBand {
  /** sub, bass, low_mids, mids, high_mids, highs */
  name: string;
  from_hz: number;
  to_hz: number;
  share_percent: number;
}

/** Loudness, peak, stereo and tone of the whole mix (daw-analysis `Measurements`). */
export interface MixMeasurements {
  duration_s: number;
  /** Null when silent. */
  integrated_lufs: number | null;
  max_short_term_lufs: number | null;
  true_peak_dbtp: number;
  sample_peak_dbfs: number;
  rms_dbfs: number;
  peak_to_loudness_db: number | null;
  /** +1 mono, 0 wide, negative out of phase. */
  stereo_correlation: number;
  bands: AnalysisBand[];
}

/** One track's level in the mix. */
export interface TrackLevel {
  id: number;
  name: string;
  lufs: number | null;
  peak_dbfs: number;
  energy_share_percent: number;
}

/** What `analyze_mix` found (the same the AI gets). */
export interface MixAnalysis {
  start_beats: number;
  end_beats: number;
  mix: MixMeasurements;
  tracks: TrackLevel[];
  hints: string[];
  spectrogram_summary?: string[];
  spectrogram_png_base64?: string;
}

/** What checking an export found. */
export interface Inspection {
  findings: Finding[];
  integrated_lufs: number | null;
  peak_dbfs: number;
  seconds: number;
}

/** What a Godot export wrote. */
export interface ExportReport {
  /** res:// paths. */
  files: string[];
  integrated_lufs: number | null;
  gain_db: number;
  seconds: number;
  looped: boolean;
  /** Seconds of intro before the loop starts (0 = loops from the start). */
  loop_start_seconds?: number;
}
