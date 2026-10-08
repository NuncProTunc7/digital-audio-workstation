// In-memory stand-in for the Rust backend, so the UI can be developed,
// tested, and screenshotted in a plain browser. It makes no sound and only
// loosely validates; the Rust Commands are the source of truth.

import catalogJson from "./generated/instruments.json";
import type { Backend } from "./backend";
import type {
  AudioStatus,
  Catalog,
  ClaudeStatus,
  CompareSide,
  LibraryPack,
  PluginInfo,
  PluginParam,
  UserPreset,
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
  RecordingDelay,
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
  const chain = (trackId: number | null) => {
    if (trackId === null) return p.master.effects;
    // Buses' effects are addressed by the bus id.
    const bus = (p.buses ?? []).find((b) => b.id === trackId);
    return bus ? bus.mixer.effects : track(trackId).mixer.effects;
  };
  const busOf = (busId: number) => {
    const b = (p.buses ?? []).find((x) => x.id === busId);
    if (!b) throw new Error(`there is no bus with id ${busId}`);
    return b;
  };
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
      ...(n.chance !== undefined && n.chance < 100 ? { chance: n.chance } : {}),
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
    case "add_plugin_effect": {
      const c = chain(command.track_id);
      c.splice(command.index ?? c.length, 0, {
        id: id(),
        kind: "plugin",
        enabled: true,
        params: {},
        plugin: structuredClone(command.plugin),
      });
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
    case "set_key":
      p.key = command.key;
      break;
    case "add_chord":
    case "restore_chord": {
      const c =
        command.command === "add_chord"
          ? { id: id(), start_beats: command.start_beats, root: command.root, quality: command.quality, bass: command.bass }
          : command.chord;
      if ((p.chords ?? []).some((x) => Math.abs(x.start_beats - c.start_beats) < 1e-9)) {
        throw new Error("there is already a chord at that beat");
      }
      p.chords = [...(p.chords ?? []), c].sort((a, b) => a.start_beats - b.start_beats);
      break;
    }
    case "set_chord": {
      const c = (p.chords ?? []).find((x) => x.id === command.chord_id);
      if (!c) throw new Error(`there is no chord with id ${command.chord_id}`);
      Object.assign(c, { root: command.root, quality: command.quality, bass: command.bass });
      break;
    }
    case "move_chord": {
      const c = (p.chords ?? []).find((x) => x.id === command.chord_id);
      if (!c) throw new Error(`there is no chord with id ${command.chord_id}`);
      if ((p.chords ?? []).some((x) => x.id !== c.id && Math.abs(x.start_beats - command.start_beats) < 1e-9)) {
        throw new Error("there is already a chord at that beat");
      }
      c.start_beats = command.start_beats;
      p.chords = [...(p.chords ?? [])].sort((a, b) => a.start_beats - b.start_beats);
      break;
    }
    case "remove_chord":
      p.chords = (p.chords ?? []).filter((x) => x.id !== command.chord_id);
      break;
    case "humanize_notes": {
      const [, c] = clipOf(command.clip_id);
      // Not the app's exact numbers, just repeatable ones.
      const jitter = (n: number) => Math.sin(command.seed * 12.9898 + n * 78.233) % 1;
      for (const n of c.notes) {
        if (command.note_ids && !command.note_ids.includes(n.id)) continue;
        n.start_beats = Math.max(0, n.start_beats + jitter(n.id * 2) * command.timing_beats);
        n.velocity = Math.min(127, Math.max(1, Math.round(n.velocity + jitter(n.id * 2 + 1) * command.velocity)));
      }
      sortNotes(c);
      break;
    }
    case "set_effect_sidechain": {
      const e = effect(command.track_id, command.effect_id);
      if (e.kind !== "compressor") throw new Error("only compressors can listen to another track");
      if (command.source !== null) track(command.source);
      e.sidechain = command.source;
      break;
    }
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
    case "add_bus": {
      const name = command.name.trim();
      if (!name) throw new Error("bus name can't be empty");
      p.buses = [
        ...(p.buses ?? []),
        { id: id(), name, mixer: { volume_db: 0, pan: 0, mute: false, solo: false, effects: [] } },
      ];
      break;
    }
    case "remove_bus":
      busOf(command.bus_id);
      p.buses = (p.buses ?? []).filter((b) => b.id !== command.bus_id);
      for (const t of p.tracks) {
        if (t.output === command.bus_id) t.output = null;
        t.sends = (t.sends ?? []).filter((s) => s.bus_id !== command.bus_id);
      }
      break;
    case "rename_bus":
      busOf(command.bus_id).name = command.name.trim();
      break;
    case "set_bus_mixer": {
      const m = busOf(command.bus_id).mixer;
      if (command.volume_db !== null) m.volume_db = command.volume_db;
      if (command.pan !== null) m.pan = command.pan;
      if (command.mute !== null) m.mute = command.mute;
      break;
    }
    case "freeze_track":
      track(command.track_id).frozen = command.frozen;
      break;
    case "unfreeze_track":
      track(command.track_id).frozen = null;
      break;
    case "set_track_output":
      if (command.bus_id !== null) busOf(command.bus_id);
      track(command.track_id).output = command.bus_id;
      break;
    case "set_send": {
      busOf(command.bus_id);
      const t = track(command.track_id);
      const sends = (t.sends = [...(t.sends ?? [])]);
      const s = sends.find((x) => x.bus_id === command.bus_id);
      if (s) {
        if (command.level_db !== null) s.level_db = command.level_db;
        if (command.pre_fader !== null) s.pre_fader = command.pre_fader;
      } else {
        sends.push({ bus_id: command.bus_id, level_db: command.level_db ?? -6, pre_fader: command.pre_fader ?? false });
      }
      break;
    }
    case "remove_send": {
      const t = track(command.track_id);
      t.sends = (t.sends ?? []).filter((s) => s.bus_id !== command.bus_id);
      break;
    }
    case "add_marker":
    case "restore_marker": {
      const m =
        command.command === "add_marker"
          ? { id: id(), name: command.name.trim(), start_beats: command.start_beats }
          : command.marker;
      if (!m.name) throw new Error("marker name can't be empty");
      if ((p.markers ?? []).some((x) => Math.abs(x.start_beats - m.start_beats) < 1e-9)) {
        throw new Error("there is already a marker at that beat");
      }
      p.markers = [...(p.markers ?? []), m].sort((a, b) => a.start_beats - b.start_beats);
      break;
    }
    case "move_marker": {
      const m = (p.markers ?? []).find((x) => x.id === command.marker_id);
      if (!m) throw new Error(`there is no marker with id ${command.marker_id}`);
      if ((p.markers ?? []).some((x) => x.id !== m.id && Math.abs(x.start_beats - command.start_beats) < 1e-9)) {
        throw new Error("there is already a marker at that beat");
      }
      m.start_beats = command.start_beats;
      p.markers = [...(p.markers ?? [])].sort((a, b) => a.start_beats - b.start_beats);
      break;
    }
    case "rename_marker": {
      const m = (p.markers ?? []).find((x) => x.id === command.marker_id);
      if (!m) throw new Error(`there is no marker with id ${command.marker_id}`);
      m.name = command.name.trim();
      break;
    }
    case "remove_marker":
      p.markers = (p.markers ?? []).filter((x) => x.id !== command.marker_id);
      break;
    case "take_snapshot": {
      const name = command.name.trim();
      if (!name) throw new Error("version name can't be empty");
      const { snapshots: _, name: __, format_version: ___, next_id: ____, ...song } = p;
      p.snapshots = [...(p.snapshots ?? []), { id: id(), name, song: structuredClone(song) }];
      break;
    }
    case "load_snapshot": {
      const s = (p.snapshots ?? []).find((x) => x.id === command.snapshot_id);
      if (!s) throw new Error(`there is no saved version with id ${command.snapshot_id}`);
      Object.assign(p, structuredClone(s.song));
      break;
    }
    case "set_song_state":
      Object.assign(p, structuredClone(command.song));
      break;
    case "rename_snapshot": {
      const s = (p.snapshots ?? []).find((x) => x.id === command.snapshot_id);
      if (!s) throw new Error(`there is no saved version with id ${command.snapshot_id}`);
      s.name = command.name;
      break;
    }
    case "delete_snapshot":
      p.snapshots = (p.snapshots ?? []).filter((x) => x.id !== command.snapshot_id);
      break;
    case "restore_snapshot": {
      const list = [...(p.snapshots ?? [])];
      list.splice(command.index, 0, command.snapshot);
      p.snapshots = list;
      break;
    }
    case "set_clip_muted":
      clipOf(command.clip_id)[1].muted = command.muted;
      break;
    case "comp_take": {
      const [t, c] = clipOf(command.clip_id);
      const [start, end] = [c.start_beats, c.start_beats + c.length_beats];
      for (const o of t.clips) {
        if (o.id !== c.id && o.start_beats < end - 1e-9 && o.start_beats + o.length_beats > start + 1e-9) o.muted = true;
      }
      c.muted = false;
      if (c.audio) {
        c.audio.fade_in_seconds = Math.max(c.audio.fade_in_seconds, 0.01);
        c.audio.fade_out_seconds = Math.max(c.audio.fade_out_seconds, 0.01);
      }
      break;
    }
    case "set_clip_swing": {
      const [, c] = clipOf(command.clip_id);
      if (c.audio) throw new Error("only note clips swing");
      const s = command.swing;
      if (s && (s.amount_percent < 0 || s.amount_percent > 100)) throw new Error("swing amount must be 0–100 %");
      c.swing = s && s.amount_percent > 0 ? s : null;
      break;
    }
    case "duplicate_clip": {
      const [t, c] = clipOf(command.clip_id);
      const linked = Boolean(command.linked) && !c.audio;
      if (linked && !c.link) c.link = id();
      t.clips.push({
        ...structuredClone(c),
        id: id(),
        start_beats: command.start_beats ?? c.start_beats + c.length_beats,
        notes: c.notes.map((n) => ({ ...n, id: id() })),
        link: linked ? c.link : null,
      });
      break;
    }
    case "link_clips": {
      const [, first] = clipOf(command.clip_ids[0]);
      const group = first.link ?? id();
      for (const cid of command.clip_ids) {
        const [, c] = clipOf(cid);
        if (c.audio) throw new Error("only note clips can be linked");
        c.link = group;
        if (c !== first) c.notes = first.notes.map((n) => ({ ...n, id: id() }));
      }
      break;
    }
    case "unlink_clip":
      clipOf(command.clip_id)[1].link = null;
      break;
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
        if (e.chance != null) n.chance = e.chance;
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
        second.audio.offset_seconds = c.audio.offset_seconds + (cut * 60) / (c.audio.source_bpm ?? p.tempo_bpm);
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
        const offset = c.audio.offset_seconds + (delta * 60) / (c.audio.source_bpm ?? p.tempo_bpm);
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
    case "set_clip_tempo": {
      const a = clipOf(command.clip_id)[1].audio;
      if (!a) throw new Error("only audio clips can be stretched");
      a.source_bpm = command.source_bpm;
      break;
    }
    case "load_sample_pack": {
      const t = track(command.track_id);
      if (t.instrument.kind !== "sampler") throw new Error(`"${t.name}" isn't a sampler track`);
      t.instrument.sample_pack = command.path;
      break;
    }
    case "set_plugin_params": {
      const p = track(command.track_id).instrument.plugin;
      if (!p) throw new Error("this track doesn't play a plugin");
      for (const [id, v] of Object.entries(command.params)) {
        if (!(id in p.params)) throw new Error(`${p.name} has no parameter ${id}`);
        if (v < 0 || v > 1) throw new Error("plugin parameters are 0 to 1");
        p.params[id] = v;
      }
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
  // Linked clips follow the one whose notes changed.
  const noteCommands = [
    "add_notes",
    "remove_notes",
    "edit_notes",
    "quantize_notes",
    "transpose_notes",
    "humanize_notes",
    "set_clip_swing",
  ];
  if (noteCommands.includes(command.command) && "clip_id" in command) {
    const [, src] = clipOf(command.clip_id);
    if (src.link) {
      for (const t of p.tracks) {
        for (const c of t.clips) {
          if (c.id !== src.id && c.link === src.link) {
            c.notes = src.notes.map((n) => ({ ...n, id: id() }));
            c.swing = src.swing;
          }
        }
      }
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
  let countInBars = 1;
  let comparing: { snapshot_id: number; side: CompareSide } | null = null;
  const uiErrors: string[] = [];
  let userPresets: UserPreset[] = [];
  // Stand-ins for installed plugins.
  const previewPlugins: PluginInfo[] = [
    {
      uid: "4E50543153594E544800000000000001",
      name: "NPT Test Synth",
      vendor: "Nunc Pro Tune",
      version: "1.0.0",
      kind: "instrument",
      categories: "Instrument|Synth",
      path: "C:/Program Files/Common Files/VST3/NPT Test.vst3",
    },
    {
      uid: "4E5054314741494E0000000000000001",
      name: "NPT Test Gain",
      vendor: "Nunc Pro Tune",
      version: "1.0.0",
      kind: "effect",
      categories: "Fx",
      path: "C:/Program Files/Common Files/VST3/NPT Test.vst3",
    },
  ];
  // A tiny stand-in for the free instrument library: a download finishes
  // after a couple of looks.
  const library: LibraryPack[] = [
    {
      id: "cello",
      name: "Cello",
      description: "Solo cello, bowed and plucked.",
      license: "CC0-1.0",
      credit: "Cello by Karoryfer Samples and Bigcat Instruments (CC0)",
      programs: [
        ["Bowed", "Programs/01- Bowed (velocity layer).sfz"],
        ["Plucked", "Programs/03- Plucked.sfz"],
      ],
      megabytes: 130,
      installed: null,
      job: null,
    },
    {
      id: "flute",
      name: "Flute",
      description: "Concert flute.",
      license: "CC-BY-4.0",
      credit: "Flute by Xavier Hosxe / Ixox (CC BY 4.0)",
      programs: [["Flute", "Ixox Flute.sfz"]],
      megabytes: 10,
      installed: null,
      job: null,
    },
  ];
  let startedAt = 0;
  let startBeats = 0;

  const view = (): ProjectView => ({
    project,
    can_undo: undoStack.length > 0,
    can_redo: redoStack.length > 0,
    dirty: project !== saved,
    file_path: filePath,
    missing_audio: [],
    // The preview has no fingerprints: a frozen track counts as current.
    frozen_current: project.tracks.filter((t) => t.frozen).map((t) => t.id),
  });
  let bufferSetting: number | null = null;
  const audio = (): AudioStatus => ({
    output_devices: ["Preview (no audio)"],
    default_output: "Preview (no audio)",
    active_output: "Preview (no audio)",
    sample_rate_hz: 48000,
    buffer_setting: bufferSetting,
    buffer_active: bufferSetting,
    buffer_options: [128, 256, 512, 1024, 2048],
    error: null,
    midi_inputs: [],
  });
  const position = () => (playing ? startBeats + ((performance.now() - startedAt) / 60000) * project.tempo_bpm : startBeats);
  const noop = async () => {};
  const changeListeners = new Set<(description: string) => void>();
  const dropListeners = new Set<(paths: string[], x: number, y: number) => void>();
  let inputOpen = false;
  let offsetMs = 0;
  const delay = (): RecordingDelay => ({ device: "Preview microphone", offset_ms: offsetMs, bluetooth: false });
  const input = (): InputStatus => ({
    devices: ["Preview microphone"],
    default_device: "Preview microphone",
    active: inputOpen ? "Preview microphone" : null,
    sample_rate_hz: inputOpen ? 48000 : null,
    level: inputOpen ? 0.2 : 0,
    error: null,
    delay: delay(),
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
    setCountIn: async (bars) => {
      countInBars = Math.max(0, Math.min(2, Math.round(bars)));
      return countInBars;
    },
    transportStatus: async () => ({
      playing,
      position_beats: position(),
      count_in_beats: 0,
      count_in_bars: countInBars,
      metronome_on: metronome,
      peak_left: 0,
      peak_right: 0,
      cpu_load: 0,
      buffer_frames: bufferSetting ?? 480,
      overloads: 0,
      struggling: false,
      comparing,
      track_peaks: project.tracks.map(() => 0),
      recording,
    }),
    audioStatus: async () => audio(),
    setOutputDevice: async () => audio(),
    checkForUpdate: async () => null,
    installUpdate: async () => {},
    freezeTrack: async (trackId) => {
      const t = project.tracks.find((x) => x.id === trackId);
      if (!t || t.instrument.kind === "audio") throw new Error("this track can't be frozen");
      undoStack.push(project);
      redoStack.length = 0;
      project = applyCommand(project, {
        command: "freeze_track",
        track_id: trackId,
        frozen: { file: `freeze-${trackId}.wav`, fingerprint: 1 },
      });
      return view();
    },
    inspectExport: async () => ({
      findings: [
        { level: project.tracks.some((t) => t.clips.length > 0) ? "ok" : "problem", message: project.tracks.some((t) => t.clips.length > 0) ? "Something plays." : "The export would be silent: nothing plays in this part of the song." },
        { level: "ok", message: "The loop joins smoothly where it repeats." },
      ],
      integrated_lufs: -16,
      peak_dbfs: -3,
      seconds: 8,
    }),
    auditionSeam: async (_start, endBeats) => {
      // The preview just plays from a bar before the loop point.
      startBeats = Math.max(0, endBeats - 4);
      startedAt = performance.now();
      playing = true;
    },
    userPresets: async () => structuredClone(userPresets),
    savePreset: async (trackId, name) => {
      const t = project.tracks.find((x) => x.id === trackId);
      if (!t) throw new Error(`there is no track with id ${trackId}`);
      const n = name.trim();
      if (!n) throw new Error("give the preset a name");
      userPresets = userPresets.filter((p) => !(p.kind === t.instrument.kind && p.name.toLowerCase() === n.toLowerCase()));
      userPresets.push({ name: n, kind: t.instrument.kind, params: { ...t.instrument.params } });
      return structuredClone(userPresets);
    },
    deletePreset: async (kind, name) => {
      userPresets = userPresets.filter((p) => !(p.kind === kind && p.name === name));
      return structuredClone(userPresets);
    },
    loadUserPreset: async (trackId, name) => {
      const t = project.tracks.find((x) => x.id === trackId);
      const p = userPresets.find((x) => x.kind === t?.instrument.kind && x.name.toLowerCase() === name.toLowerCase());
      if (!t || !p) throw new Error(`there is no saved preset called "${name}"`);
      undoStack.push(project);
      redoStack.length = 0;
      project = applyCommand(project, {
        command: "set_instrument",
        track_id: trackId,
        instrument: { kind: p.kind, preset: p.name, params: { ...p.params } },
      });
      return view();
    },
    previewStart: async (index) => {
      const sections = (project.markers ?? []).map((m, i, all) => ({
        name: m.name,
        start_beats: m.start_beats,
        end_beats: all[i + 1]?.start_beats ?? Math.max(m.start_beats + 4, 16),
      }));
      if (sections.length === 0) sections.push({ name: "Whole song", start_beats: 0, end_beats: 16 });
      const s = sections[index];
      if (!s) throw new Error(`there is no section ${index}`);
      startBeats = s.start_beats;
      startedAt = performance.now();
      playing = true;
      return { sections, layers: project.tracks.map((t) => ({ track_id: t.id, name: t.name })) };
    },
    previewSwitch: async () => {},
    previewLayer: async () => {},
    previewStop: async () => {
      startBeats = position();
      playing = false;
    },
    compareStart: async (snapshotId) => {
      const s = (project.snapshots ?? []).find((x) => x.id === snapshotId);
      if (!s) throw new Error(`there is no saved version with id ${snapshotId}`);
      comparing = { snapshot_id: snapshotId, side: "current" };
      return {
        snapshot_id: snapshotId,
        name: s.name,
        current_lufs: -16,
        version_lufs: -18,
        current_gain_db: -2,
        version_gain_db: 0,
      };
    },
    compareListen: async (side) => {
      if (!comparing) throw new Error("not comparing versions");
      comparing = { ...comparing, side };
    },
    compareStop: async () => {
      comparing = null;
    },
    setBufferSize: async (frames) => {
      bufferSetting = frames;
      return audio();
    },
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
    plugins: async () => ({ plugins: structuredClone(previewPlugins), could_not_use: [] }),
    loadPlugin: async (trackId, uid) => {
      const p = previewPlugins.find((x) => x.uid === uid);
      if (!p) throw new Error(`no installed plugin has id ${uid}`);
      if (p.kind !== "instrument") throw new Error(`${p.name} is an effect, not an instrument`);
      if (!project.tracks.some((x) => x.id === trackId)) throw new Error(`there is no track ${trackId}`);
      undoStack.push(project);
      redoStack.length = 0;
      project = applyCommand(project, {
        command: "set_instrument",
        track_id: trackId,
        instrument: {
          kind: "plugin",
          preset: p.name,
          params: {},
          plugin: { uid: p.uid, name: p.name, vendor: p.vendor, path: p.path, params: { "0": 0.5, "1": 0.25 } },
        },
      });
      return view();
    },
    addPluginEffect: async (trackId, uid) => {
      const p = previewPlugins.find((x) => x.uid === uid);
      if (!p) throw new Error(`no installed plugin has id ${uid}`);
      if (p.kind !== "effect") throw new Error(`${p.name} is an instrument`);
      undoStack.push(project);
      redoStack.length = 0;
      project = applyCommand(project, {
        command: "add_plugin_effect",
        track_id: trackId,
        index: null,
        plugin: { uid: p.uid, name: p.name, vendor: p.vendor, path: p.path, params: { "0": 0.5 } },
      });
      return view();
    },
    pluginParams: async (pluginId) => {
      const all = [
        ...project.tracks.map((t) => (t.id === pluginId ? t.instrument.plugin : null)),
        ...[...project.tracks.map((t) => t.mixer), ...(project.buses ?? []).map((b) => b.mixer), project.master]
          .flatMap((m) => m.effects)
          .map((e) => (e.id === pluginId ? e.plugin : null)),
      ];
      const p = all.find((x) => x);
      if (!p) throw new Error("there's no plugin with that id");
      const names = ["Level", "Brightness"];
      return Object.entries(p.params).map(
        ([id, value]): PluginParam => ({
          id: Number(id),
          name: names[Number(id)] ?? `Param ${id}`,
          units: "%",
          value,
          default: 0.5,
          display: `${Math.round(value * 100)}`,
          steps: 0,
          automatable: true,
        }),
      );
    },
    pluginStatus: async () => ({ state: "ready" }),
    openPluginWindow: async () => {
      throw new Error("Plugin windows need the desktop app");
    },
    sampleLibrary: async () => {
      for (const p of library) {
        if (p.job?.state === "downloading") {
          const mb = p.megabytes * 1_000_000;
          const bytes = Math.min(mb, p.job.bytes + mb / 2);
          p.job = bytes >= mb ? { state: "unpacking" } : { state: "downloading", bytes, total: mb };
        } else if (p.job?.state === "unpacking") {
          p.job = null;
          p.installed = p.programs.map(([name, sfz]) => [name, `C:/Library/${p.id}/${sfz}`]);
        }
      }
      return structuredClone(library);
    },
    downloadSamplePack: async (id) => {
      const p = library.find((x) => x.id === id);
      if (!p) throw new Error(`there is no instrument "${id}" in the library`);
      if (!p.installed && !p.job) p.job = { state: "downloading", bytes: 0, total: null };
    },
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
    setRecordingOffset: async (ms) => {
      offsetMs = Math.max(-500, Math.min(500, ms));
      return delay();
    },
    calibrateRecording: async () => {
      throw new Error("Calibrating needs the desktop app");
    },
    recoveryCheck: async () => null,
    recoveryResolve: async () => view(),
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
    diagnosticReport: async () =>
      [
        "Nunc Pro Tune diagnostic report",
        "App: browser preview (no audio engine)",
        `Song: "${project.name}" · ${project.tempo_bpm} BPM · ${project.tracks.length} tracks`,
        "",
        `Recent errors (${uiErrors.length}):`,
        ...uiErrors.map((e) => `  ${e}`),
      ].join("\n"),
    logError: async (message) => {
      uiErrors.push(message);
      if (uiErrors.length > 10) uiErrors.shift();
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
