// In-memory stand-in for the Rust backend, so the UI can be developed,
// tested, and screenshotted in a plain browser. It makes no sound and only
// loosely validates; the Rust Commands are the source of truth.

import catalogJson from "./generated/instruments.json";
import type { Backend } from "./backend";
import type {
  AudioStatus,
  Catalog,
  ClaudeStatus,
  InputStatus,
  Peaks,
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
      if (t.instrument.kind === "audio") throw new Error(`"${t.name}" is an audio track`);
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
    case "split_clip": {
      const [t, c] = clipOf(command.clip_id);
      const cut = command.at_beats - c.start_beats;
      if (cut <= 0 || cut >= c.length_beats) throw new Error("split point must be inside the clip");
      const second: Clip = {
        ...structuredClone(c),
        id: id(),
        start_beats: command.at_beats,
        length_beats: c.length_beats - cut,
        notes: c.notes.filter((n) => n.start_beats >= cut).map((n) => ({ ...n, start_beats: n.start_beats - cut })),
      };
      c.length_beats = cut;
      c.notes = c.notes.filter((n) => n.start_beats < cut);
      if (c.audio && second.audio) {
        second.audio.offset_seconds = c.audio.offset_seconds + (cut * 60) / p.tempo_bpm;
        c.audio.fade_out_seconds = 0;
        second.audio.fade_in_seconds = 0;
      }
      t.clips.push(second);
      t.clips.sort((a, b) => a.start_beats - b.start_beats);
      break;
    }
    case "trim_clip_start": {
      const [, c] = clipOf(command.clip_id);
      const delta = command.start_beats - c.start_beats;
      if (delta >= c.length_beats) throw new Error("clip length must stay positive");
      if (c.audio) {
        const offset = c.audio.offset_seconds + (delta * 60) / p.tempo_bpm;
        if (offset < -1e-6) throw new Error("can't move before the beginning of the recording");
        c.audio.offset_seconds = Math.max(0, offset);
      }
      c.notes = c.notes.map((n) => ({ ...n, start_beats: n.start_beats - delta })).filter((n) => n.start_beats >= 0);
      c.start_beats = command.start_beats;
      c.length_beats -= delta;
      break;
    }
    case "add_audio_clip": {
      const t = track(command.track_id);
      if (t.instrument.kind !== "audio") throw new Error(`"${t.name}" is not an audio track`);
      const remaining = command.audio.file_seconds - command.audio.offset_seconds;
      t.clips.push({
        id: id(),
        name: command.name ?? command.audio.file.replace(/(-[0-9a-f]{8})?\.[^.]+$/, ""),
        start_beats: command.start_beats,
        length_beats: command.length_beats ?? (remaining * p.tempo_bpm) / 60,
        notes: [],
        audio: structuredClone(command.audio),
      });
      t.clips.sort((a, b) => a.start_beats - b.start_beats);
      break;
    }
    case "set_audio_clip": {
      const a = clipOf(command.clip_id)[1].audio;
      if (!a) throw new Error("is a note clip, not an audio clip");
      if (command.gain_db !== null) a.gain_db = command.gain_db;
      if (command.fade_in_seconds !== null) a.fade_in_seconds = command.fade_in_seconds;
      if (command.fade_out_seconds !== null) a.fade_out_seconds = command.fade_out_seconds;
      break;
    }
    case "load_sample_pack": {
      const t = track(command.track_id);
      if (t.instrument.kind !== "sampler") throw new Error(`"${t.name}" isn't a sampler track`);
      t.instrument.sample_pack = command.path;
      break;
    }
    case "add_automation_lane": {
      const t = track(command.track_id);
      t.automation ??= [];
      if (t.automation.some((l) => JSON.stringify(l.target) === JSON.stringify(command.target))) {
        throw new Error("an automation lane for that setting already exists on this track");
      }
      t.automation.push({
        id: id(),
        target: structuredClone(command.target),
        enabled: true,
        points: [...command.points].sort((a, b) => a.beats - b.beats),
      });
      break;
    }
    case "remove_automation_lane": {
      const t = track(command.track_id);
      t.automation = (t.automation ?? []).filter((l) => l.id !== command.lane_id);
      break;
    }
    case "set_automation_points": {
      const lane = track(command.track_id).automation?.find((l) => l.id === command.lane_id);
      if (!lane) throw new Error(`automation lane ${command.lane_id} isn't on this track`);
      lane.points = [...command.points].sort((a, b) => a.beats - b.beats);
      break;
    }
    case "set_automation_enabled": {
      const lane = track(command.track_id).automation?.find((l) => l.id === command.lane_id);
      if (!lane) throw new Error(`automation lane ${command.lane_id} isn't on this track`);
      lane.enabled = command.enabled;
      break;
    }
    case "batch": {
      // All-or-nothing: a throw leaves the original project untouched.
      let next = p;
      for (const c of command.commands) next = applyCommand(next, c);
      return next;
    }
  }
  return p;
}

/** The preview backend, plus a hook for tests to act like Claude. */
export interface PreviewBackend extends Backend {
  /** Applies a Command as if Claude sent it, and fires the change event. */
  simulateRemoteChange(command: Command, description: string): void;
  /** Acts as if files were dropped on the window at (x, y). */
  simulateFileDrop(paths: string[], x: number, y: number): void;
}

export function createPreviewBackend(): PreviewBackend {
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
    missing_audio: [],
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
  const changeListeners = new Set<(description: string) => void>();
  const dropListeners = new Set<(paths: string[], x: number, y: number) => void>();
  let inputOpen = false;
  const input = (): InputStatus => ({
    devices: ["Preview microphone"],
    default_device: "Preview microphone",
    active: inputOpen ? "Preview microphone" : null,
    sample_rate_hz: inputOpen ? 48000 : null,
    level: inputOpen ? 0.2 : 0,
    error: null,
  });
  // Every "file" is a two-second tone; enough to draw and edit.
  const PREVIEW_SECONDS = 2;
  const claude = (): ClaudeStatus => ({
    listening: false,
    error: "Claude needs the desktop app",
    last_activity_secs: null,
    activity: [],
    bridge_path: "npt-mcp",
    bridge_found: false,
    desktop_config_path: "",
    desktop_configured: false,
    claude_code_command: 'claude mcp add --scope user nunc-pro-tune -- "npt-mcp"',
  });

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
    importAudio: async (path, trackId, startBeats) => {
      const name = path.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? "Audio";
      const audio = {
        file: `${name}-0000beef.wav`,
        file_seconds: PREVIEW_SECONDS,
        offset_seconds: 0,
        gain_db: 0,
        fade_in_seconds: 0,
        fade_out_seconds: 0,
      };
      let target = trackId;
      let next = project;
      if (target === null) {
        target = next.next_id;
        next = applyCommand(next, { command: "add_track", name, instrument: "audio", preset: null, index: null });
      }
      next = applyCommand(next, {
        command: "add_audio_clip",
        track_id: target,
        start_beats: startBeats ?? 0,
        audio,
        length_beats: null,
        name,
      });
      undoStack.push(project);
      redoStack.length = 0;
      project = next;
      return view();
    },
    pickAudioFiles: async () => [],
    pickImportFiles: async () => [],
    importMidi: async (path) => {
      throw new Error(`Opening MIDI files needs the desktop app (${path})`);
    },
    pickExportPath: async () => null,
    exportWav: noop,
    exportMidi: noop,
    importMusicXml: async (path) => {
      throw new Error(`Opening sheet music needs the desktop app (${path})`);
    },
    exportMusicXml: noop,
    pickFolder: async () => null,
    pickSamplePack: async () => null,
    samplePackStatus: async () => ({ state: "ready", name: "Preview", zones: 0, megabytes: 0 }),
    exportGodot: async (options) => {
      const name = (options.name ?? project.name).toLowerCase().replace(/[^a-z0-9]+/g, "_");
      const folder = options.folder ?? "music";
      const files = [`res://${folder}/${name}.${options.format ?? "ogg"}`];
      if (options.stems) {
        for (const t of project.tracks) files.push(`res://${folder}/${name}_${t.name.toLowerCase()}.ogg`);
        if (options.layers ?? true) files.push(`res://${folder}/${name}_layers.tres`);
      }
      return { files, integrated_lufs: options.normalize === false ? null : (options.target_lufs ?? -16), gain_db: 0, seconds: 8, looped: options.looped ?? true };
    },
    // A one-bar score per track, so the view has something to draw.
    sheetMusic: async (trackIds) => {
      const tracks = project.tracks.filter((t) => t.instrument.kind !== "audio" && (!trackIds || trackIds.includes(t.id)));
      const parts = tracks.map((t, i) => `<score-part id="P${i + 1}"><part-name>${t.name}</part-name></score-part>`);
      const bodies = tracks.map(
        (_, i) =>
          `<part id="P${i + 1}"><measure number="1"><attributes><divisions>4</divisions><time><beats>4</beats><beat-type>4</beat-type></time><clef><sign>G</sign><line>2</line></clef></attributes><note><rest measure="yes"/><duration>16</duration></note></measure></part>`,
      );
      return `<?xml version="1.0"?><score-partwise version="4.0"><part-list>${parts.join("")}</part-list>${bodies.join("")}</score-partwise>`;
    },
    audioPeaks: async (): Promise<Peaks> => {
      const perSecond = 200;
      const minMax: number[] = [];
      for (let i = 0; i < PREVIEW_SECONDS * perSecond; i++) {
        const a = 0.5 * Math.abs(Math.sin(i / 25));
        minMax.push(-a, a);
      }
      return { per_second: perSecond, min_max: minMax, peak: 0.5, seconds: PREVIEW_SECONDS };
    },
    inputStatus: async () => input(),
    monitorInput: async (on) => {
      inputOpen = on;
      return input();
    },
    setInputDevice: async () => input(),
    onFileDrop: async (callback) => {
      dropListeners.add(callback);
      return () => dropListeners.delete(callback);
    },
    simulateFileDrop: (paths: string[], x: number, y: number) => {
      for (const l of dropListeners) l(paths, x, y);
    },
    claudeStatus: async () => claude(),
    claudeInstallDesktop: async () => {
      throw new Error("Connecting Claude needs the desktop app");
    },
    onProjectChanged: async (callback) => {
      changeListeners.add(callback);
      return () => changeListeners.delete(callback);
    },
    simulateRemoteChange: (command: Command, description: string) => {
      const next = applyCommand(project, command);
      undoStack.push(project);
      redoStack.length = 0;
      project = next;
      for (const l of changeListeners) l(description);
    },
  };
}
