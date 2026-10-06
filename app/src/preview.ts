// In-memory stand-in for the Rust backend, so the UI can be developed,
// tested, and screenshotted in a plain browser. It makes no sound and only
// loosely validates; the Rust Commands are the source of truth.

import catalogJson from "./generated/instruments.json";
import type { Backend } from "./backend";
import type {
  AudioStatus,
  Catalog,
  Clip,
  Command,
  Effect,
  Instrument,
  InstrumentKind,
  Mixer,
  Note,
  NoteInput,
  Project,
  ProjectView,
  Track,
} from "./types";

const catalog = catalogJson as Catalog;

function defaultInstrument(kind: InstrumentKind, preset: string): Instrument {
  const description = catalog.instruments.find((i) => i.kind === kind);
  const params: Record<string, number> = {};
  for (const spec of description?.params ?? []) params[spec.id] = spec.default;
  return { kind, preset, params };
}

function defaultMixer(): Mixer {
  return { volume_db: 0, pan: 0, mute: false, solo: false, effects: [] };
}

export function defaultProject(): Project {
  const track = (id: number, name: string, kind: InstrumentKind, preset: string): Track => ({
    id,
    name,
    instrument: defaultInstrument(kind, preset),
    mixer: defaultMixer(),
    clips: [],
  });
  return {
    format_version: 1,
    name: "Untitled",
    tempo_bpm: 120,
    time_signature: { numerator: 4, denominator: 4 },
    tracks: [
      track(1, "Keys", "synth", "Warm Keys"),
      track(2, "Bass", "synth", "Fat Bass"),
      track(3, "Drums", "drums", "Classic Kit"),
    ],
    master: { volume_db: 0, effects: [] },
    loop_region: { enabled: false, start_beats: 0, end_beats: 16 },
    next_id: 4,
  };
}

/** Applies a Command to a copy of the project. Throws on obvious errors. */
export function applyCommand(project: Project, command: Command): Project {
  const p: Project = structuredClone(project);
  const id = () => p.next_id++;
  const track = (trackId: number) => {
    const t = p.tracks.find((x) => x.id === trackId);
    if (!t) throw new Error(`there is no track with id ${trackId}`);
    return t;
  };
  const clipOf = (clipId: number): [Track, Clip] => {
    for (const t of p.tracks) {
      const c = t.clips.find((x) => x.id === clipId);
      if (c) return [t, c];
    }
    throw new Error(`there is no clip with id ${clipId}`);
  };
  const chain = (trackId: number | null) => (trackId === null ? p.master.effects : track(trackId).mixer.effects);
  const effect = (trackId: number | null, effectId: number): Effect => {
    const e = chain(trackId).find((x) => x.id === effectId);
    if (!e) throw new Error(`there is no effect with id ${effectId} there`);
    return e;
  };
  const toNotes = (inputs: NoteInput[]): Note[] =>
    inputs.map((n) => ({
      id: n.id ?? id(),
      pitch: n.pitch,
      start_beats: n.start_beats,
      length_beats: n.length_beats,
      velocity: n.velocity ?? 100,
    }));
  const sortNotes = (c: Clip) => c.notes.sort((a, b) => a.start_beats - b.start_beats || a.pitch - b.pitch);

  switch (command.command) {
    case "rename_project":
      p.name = command.name;
      break;
    case "set_tempo":
      if (!(command.bpm >= 20 && command.bpm <= 999)) throw new Error("tempo must be between 20 and 999 BPM");
      p.tempo_bpm = command.bpm;
      break;
    case "set_time_signature":
      p.time_signature = { numerator: command.numerator, denominator: command.denominator };
      break;
    case "set_loop":
      p.loop_region = {
        enabled: command.enabled ?? p.loop_region.enabled,
        start_beats: command.start_beats ?? p.loop_region.start_beats,
        end_beats: command.end_beats ?? p.loop_region.end_beats,
      };
      break;
    case "add_track": {
      const description = catalog.instruments.find((i) => i.kind === command.instrument);
      const preset = command.preset ?? description?.presets[0]?.name ?? "Init";
      const t: Track = {
        id: id(),
        name: command.name,
        instrument: defaultInstrument(command.instrument, preset),
        mixer: defaultMixer(),
        clips: [],
      };
      p.tracks.splice(command.index ?? p.tracks.length, 0, t);
      break;
    }
    case "remove_track":
      track(command.track_id);
      p.tracks = p.tracks.filter((t) => t.id !== command.track_id);
      break;
    case "rename_track":
      track(command.track_id).name = command.name;
      break;
    case "move_track": {
      const t = track(command.track_id);
      p.tracks = p.tracks.filter((x) => x !== t);
      p.tracks.splice(command.index, 0, t);
      break;
    }
    case "set_track_mixer": {
      const m = track(command.track_id).mixer;
      if (command.volume_db !== null) m.volume_db = command.volume_db;
      if (command.pan !== null) m.pan = command.pan;
      if (command.mute !== null) m.mute = command.mute;
      if (command.solo !== null) m.solo = command.solo;
      break;
    }
    case "set_master_volume":
      p.master.volume_db = command.volume_db;
      break;
    case "set_instrument_param":
      track(command.track_id).instrument.params[command.param] = command.value;
      break;
    case "load_preset": {
      const t = track(command.track_id);
      t.instrument = defaultInstrument(t.instrument.kind, command.preset);
      break;
    }
    case "set_instrument":
      track(command.track_id).instrument = command.instrument;
      break;
    case "add_effect": {
      const description = catalog.effects.find((e) => e.kind === command.kind);
      const params: Record<string, number> = {};
      for (const s of description?.params ?? []) params[s.id] = s.default;
      const c = chain(command.track_id);
      c.splice(command.index ?? c.length, 0, { id: id(), kind: command.kind, enabled: true, params });
      break;
    }
    case "remove_effect": {
      effect(command.track_id, command.effect_id);
      const c = chain(command.track_id);
      c.splice(
        c.findIndex((e) => e.id === command.effect_id),
        1,
      );
      break;
    }
    case "set_effect_param":
      effect(command.track_id, command.effect_id).params[command.param] = command.value;
      break;
    case "set_effect_enabled":
      effect(command.track_id, command.effect_id).enabled = command.enabled;
      break;
    case "create_clip": {
      const t = track(command.track_id);
      const notes = toNotes(command.notes);
      t.clips.push({
        id: id(),
        name: command.name ?? t.name,
        start_beats: command.start_beats,
        length_beats: command.length_beats,
        notes,
      });
      t.clips.sort((a, b) => a.start_beats - b.start_beats);
      break;
    }
    case "delete_clip": {
      const [t] = clipOf(command.clip_id);
      t.clips = t.clips.filter((c) => c.id !== command.clip_id);
      break;
    }
    case "move_clip": {
      const [from, c] = clipOf(command.clip_id);
      if (command.start_beats !== null) c.start_beats = command.start_beats;
      if (command.track_id !== null && command.track_id !== from.id) {
        from.clips = from.clips.filter((x) => x !== c);
        track(command.track_id).clips.push(c);
      }
      break;
    }
    case "resize_clip":
      clipOf(command.clip_id)[1].length_beats = command.length_beats;
      break;
    case "rename_clip":
      clipOf(command.clip_id)[1].name = command.name;
      break;
    case "duplicate_clip": {
      const [t, c] = clipOf(command.clip_id);
      t.clips.push({
        ...structuredClone(c),
        id: id(),
        start_beats: command.start_beats ?? c.start_beats + c.length_beats,
        notes: c.notes.map((n) => ({ ...n, id: id() })),
      });
      break;
    }
    case "add_notes": {
      const [, c] = clipOf(command.clip_id);
      c.notes.push(...toNotes(command.notes));
      sortNotes(c);
      break;
    }
    case "remove_notes": {
      const [, c] = clipOf(command.clip_id);
      c.notes = c.notes.filter((n) => !command.note_ids.includes(n.id));
      break;
    }
    case "edit_notes": {
      const [, c] = clipOf(command.clip_id);
      for (const e of command.edits) {
        const n = c.notes.find((x) => x.id === e.id);
        if (!n) throw new Error(`clip has no note with id ${e.id}`);
        if (e.pitch != null) n.pitch = e.pitch;
        if (e.start_beats != null) n.start_beats = e.start_beats;
        if (e.length_beats != null) n.length_beats = e.length_beats;
        if (e.velocity != null) n.velocity = e.velocity;
      }
      sortNotes(c);
      break;
    }
    case "quantize_notes": {
      const [, c] = clipOf(command.clip_id);
      const g = command.grid_beats;
      for (const n of c.notes) {
        if (command.note_ids && !command.note_ids.includes(n.id)) continue;
        n.start_beats = Math.round(n.start_beats / g) * g;
        if (command.lengths) n.length_beats = Math.max(g, Math.round(n.length_beats / g) * g);
      }
      sortNotes(c);
      break;
    }
    case "transpose_notes": {
      const [, c] = clipOf(command.clip_id);
      for (const n of c.notes) {
        if (command.note_ids && !command.note_ids.includes(n.id)) continue;
        n.pitch = Math.max(0, Math.min(127, n.pitch + command.semitones));
      }
      break;
    }
  }
  return p;
}

export function createPreviewBackend(): Backend {
  let project = defaultProject();
  let saved = project;
  let filePath: string | null = null;
  const undoStack: Project[] = [];
  const redoStack: Project[] = [];
  let playing = false;
  let recording = false;
  let metronome = true;
  let startedAt = 0;
  let startBeats = 0;

  const view = (): ProjectView => ({
    project,
    can_undo: undoStack.length > 0,
    can_redo: redoStack.length > 0,
    dirty: project !== saved,
    file_path: filePath,
  });
  const audio = (): AudioStatus => ({
    output_devices: ["Preview (no audio)"],
    default_output: "Preview (no audio)",
    active_output: "Preview (no audio)",
    sample_rate_hz: 48000,
    error: null,
    midi_inputs: [],
  });
  const position = () => (playing ? startBeats + ((performance.now() - startedAt) / 60000) * project.tempo_bpm : startBeats);
  const noop = async () => {};

  return {
    preview: true,
    appInfo: async () => ({ name: "Nunc Pro Tune", version: "preview", license: "GPL-3.0-or-later" }),
    catalog: async () => catalog,
    getProject: async () => view(),
    execute: async (command) => {
      const next = applyCommand(project, command);
      undoStack.push(project);
      redoStack.length = 0;
      project = next;
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
    newProject: async () => {
      project = defaultProject();
      saved = project;
      filePath = null;
      undoStack.length = 0;
      redoStack.length = 0;
      return view();
    },
    openProject: async (path) => {
      throw new Error(`Opening files needs the desktop app (${path})`);
    },
    saveProject: async (path) => {
      filePath = path ?? filePath ?? "preview.nptune";
      saved = project;
      return view();
    },
    pickOpenPath: async () => null,
    pickSavePath: async () => null,
    confirm: async (message) => window.confirm(message),
    setTitle: async (title) => {
      document.title = title;
    },
    onCloseRequested: async () => () => {},
    noteOn: noop,
    noteOff: noop,
    allNotesOff: noop,
    selectTrack: noop,
    play: async () => {
      if (!playing) startedAt = performance.now();
      playing = true;
    },
    stop: async () => {
      startBeats = playing ? position() : 0;
      playing = false;
    },
    locate: async (beats) => {
      startBeats = beats;
      startedAt = performance.now();
    },
    recordStart: async () => {
      recording = true;
      if (!playing) startedAt = performance.now();
      playing = true;
    },
    recordStop: async () => {
      recording = false;
      startBeats = position();
      playing = false;
      return view();
    },
    setMetronome: async (on) => {
      metronome = on;
    },
    transportStatus: async () => ({
      playing,
      position_beats: position(),
      metronome_on: metronome,
      peak_left: 0,
      peak_right: 0,
      cpu_load: 0,
      buffer_frames: 480,
      track_peaks: project.tracks.map(() => 0),
      recording,
    }),
    audioStatus: async () => audio(),
    setOutputDevice: async () => audio(),
    refreshMidi: async () => audio(),
    onMidiNote: async () => () => {},
  };
}
