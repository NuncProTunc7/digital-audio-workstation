import { Fragment, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import { meterPercent, snap, snapDown } from "../format";
import type { AutomationTarget, Catalog, Chord, ChordQuality, Clip, Command, Marker, Peaks, Project, Track } from "../types";
import { CHORD_QUALITIES, NOTE_NAMES, chordName, homeChord } from "../music";
import { AUTOMATION_HEIGHT, automatableTargets, targetScale } from "../automation";
import AutomationLaneEditor from "./AutomationLane";
import Waveform from "./Waveform";

export const TRACK_HEIGHT = 60;
/** Height of one take lane when an audio track shows its takes. */
const TAKE_LANE_HEIGHT = 40;

/** Puts overlapping clips on separate lanes (first free lane, by start). */
function takeLanes(clips: Clip[]): { lane: Map<number, number>; count: number } {
  const ends: number[] = [];
  const lane = new Map<number, number>();
  for (const c of [...clips].sort((a, b) => a.start_beats - b.start_beats || a.id - b.id)) {
    let i = ends.findIndex((end) => end <= c.start_beats + 1e-9);
    if (i < 0) {
      i = ends.length;
      ends.push(0);
    }
    ends[i] = c.start_beats + c.length_beats;
    lane.set(c.id, i);
  }
  return { lane, count: Math.max(1, ends.length) };
}
const MIN_BARS = 32;

interface TimelineProps {
  project: Project;
  selectedTrackId: number;
  selectedClipId: number | null;
  playheadBeats: number;
  recording: boolean;
  trackPeaks: number[];
  pixelsPerBeat: number;
  onZoom: (pixelsPerBeat: number) => void;
  onSelectTrack: (id: number) => void;
  onSelectClip: (id: number | null, trackId: number) => void;
  onOpenClip: (id: number) => void;
  onCommand: (command: Command) => Promise<Project | undefined>;
  onEndGesture: () => void;
  onLocate: (beats: number) => void;
  onRemoveTrack: (track: Track) => void;
  /** Waveforms, by audio file name (loaded as needed). */
  peaks: Readonly<Record<string, Peaks>>;
  /** Audio files that can't be found. */
  missingAudio: ReadonlySet<string>;
  /** Frozen tracks whose rendering is up to date. */
  frozenCurrent: ReadonlySet<number>;
  /** Renders a track and plays that instead (freeze). */
  onFreeze: (trackId: number) => void;
  /** Asks for audio files and imports them onto `trackId` (null: new tracks) at `beats`. */
  onImportAudio: (trackId: number | null, beats: number) => void;
  catalog: Catalog;
}

const isAudio = (t: Track) => t.instrument.kind === "audio";

const TRACK_ICONS: Record<Track["instrument"]["kind"], string> = {
  synth: "🎹",
  drums: "🥁",
  audio: "🎤",
  sampler: "🎻",
  plugin: "🔌",
};

type Drag =
  | {
      kind: "move";
      clip: Clip;
      x0: number;
      lastStart: number;
      lastTrack: number;
      moved: boolean;
    }
  | { kind: "resize"; clip: Clip; x0: number; lastLength: number }
  | { kind: "trim"; clip: Clip; x0: number; lastStart: number }
  | { kind: "marker"; marker: Marker; x0: number; lastStart: number }
  | { kind: "chord"; chord: Chord; x0: number; lastStart: number; moved: boolean }
  | { kind: "loop"; anchor: number };

/** The arrangement: track headers, clip lanes, ruler, loop region, playhead. */
export default function Timeline(props: TimelineProps) {
  const { project, pixelsPerBeat: ppb } = props;
  const beatsPerBar = project.time_signature.numerator;
  const totalBeats = MIN_BARS * beatsPerBar;
  const lanesRef = useRef<HTMLDivElement>(null);
  const drag = useRef<Drag | null>(null);
  const [renaming, setRenaming] = useState<number | null>(null);
  const [renamingMarker, setRenamingMarker] = useState<number | null>(null);
  const markers = project.markers ?? [];
  const chords = project.chords ?? [];
  // The chord being edited in its little menu, and where to show it.
  const [chordMenu, setChordMenu] = useState<{ id: number; x: number; y: number } | null>(null);
  // A note clip's new start while its left edge is dragged. Trimming a note
  // clip removes the notes before the start, so it's applied once, on
  // release: dragging back out must not lose them.
  const [trimPreview, setTrimPreview] = useState<{ clipId: number; start: number } | null>(null);
  // Tracks showing their automation row, and the lane each row shows.
  const [openAutomation, setOpenAutomation] = useState<ReadonlySet<number>>(new Set());
  const [shownLane, setShownLane] = useState<Record<number, number>>({});
  // Audio tracks showing their takes on separate lanes.
  const [openTakes, setOpenTakes] = useState<ReadonlySet<number>>(new Set());
  const lanesOf = (t: Track) => takeLanes(t.clips);
  const laneHeight = (t: Track) =>
    isAudio(t) && openTakes.has(t.id) ? Math.max(TRACK_HEIGHT, lanesOf(t).count * TAKE_LANE_HEIGHT) : TRACK_HEIGHT;
  const rowHeight = (t: Track) => laneHeight(t) + (openAutomation.has(t.id) ? AUTOMATION_HEIGHT : 0);
  /** Splits every clip under the playhead on a track (to comp takes). */
  const splitTakesAt = (t: Track, beats: number) => {
    const commands: Command[] = t.clips
      .filter((c) => c.start_beats < beats - 1e-6 && c.start_beats + c.length_beats > beats + 1e-6)
      .map((c) => ({ command: "split_clip", clip_id: c.id, at_beats: beats }));
    if (commands.length > 0) void props.onCommand({ command: "batch", commands }).then(props.onEndGesture);
  };

  const songEnd = Math.max(
    0,
    ...project.tracks.flatMap((t) => t.clips.map((c) => c.start_beats + c.length_beats)),
  );
  const beats = Math.max(totalBeats, Math.ceil((songEnd + 8 * beatsPerBar) / beatsPerBar) * beatsPerBar);
  const width = beats * ppb;
  // Snap clips to beats when zoomed in, bars when zoomed out.
  const grid = ppb >= 16 ? 1 : beatsPerBar;

  const beatAt = (clientX: number) => {
    const rect = lanesRef.current?.getBoundingClientRect();
    return rect ? Math.max(0, (clientX - rect.left) / ppb) : 0;
  };
  const trackIndexAt = (clientY: number) => {
    const rect = lanesRef.current?.getBoundingClientRect();
    if (!rect) return 0;
    let y = clientY - rect.top;
    for (let i = 0; i < project.tracks.length; i++) {
      y -= rowHeight(project.tracks[i]);
      if (y < 0) return i;
    }
    return Math.max(0, project.tracks.length - 1);
  };
  const toggleAutomation = (id: number) =>
    setOpenAutomation((open) => {
      const next = new Set(open);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const laneOf = (t: Track) => {
    const lanes = t.automation ?? [];
    return lanes.find((l) => l.id === shownLane[t.id]) ?? lanes[0];
  };
  const pickLane = async (t: Track, value: string) => {
    if (value.startsWith("new:")) {
      const target = JSON.parse(value.slice(4)) as AutomationTarget;
      const before = new Set((t.automation ?? []).map((l) => l.id));
      const p = await props.onCommand({ command: "add_automation_lane", track_id: t.id, target, points: [] });
      props.onEndGesture();
      const created = p?.tracks.find((x) => x.id === t.id)?.automation?.find((l) => !before.has(l.id));
      if (created) setShownLane((s) => ({ ...s, [t.id]: created.id }));
    } else {
      setShownLane((s) => ({ ...s, [t.id]: Number(value) }));
    }
  };

  const startClipDrag = (e: ReactPointerEvent, clip: Clip, trackIndex: number, edge: "move" | "end" | "start") => {
    e.stopPropagation();
    e.currentTarget.setPointerCapture?.(e.pointerId);
    props.onSelectClip(clip.id, project.tracks[trackIndex].id);
    drag.current =
      edge === "end"
        ? { kind: "resize", clip, x0: e.clientX, lastLength: clip.length_beats }
        : edge === "start"
          ? { kind: "trim", clip, x0: e.clientX, lastStart: clip.start_beats }
          : {
          kind: "move",
          clip,
          x0: e.clientX,
          lastStart: clip.start_beats,
          lastTrack: trackIndex,
          moved: false,
        };
  };

  const onPointerMove = (e: ReactPointerEvent) => {
    const d = drag.current;
    if (!d) return;
    if (d.kind === "move") {
      const start = Math.max(0, d.clip.start_beats + snap((e.clientX - d.x0) / ppb, grid));
      // Audio clips only go on audio tracks, note clips only on instrument tracks.
      const over = trackIndexAt(e.clientY);
      const trackIndex =
        isAudio(project.tracks[over]) === Boolean(d.clip.audio) ? over : d.lastTrack;
      if (start !== d.lastStart || trackIndex !== d.lastTrack) {
        d.lastStart = start;
        d.lastTrack = trackIndex;
        d.moved = true;
        // Always send both fields so a whole drag merges into one undo step.
        void props.onCommand({
          command: "move_clip",
          clip_id: d.clip.id,
          start_beats: start,
          track_id: project.tracks[trackIndex].id,
        });
      }
    } else if (d.kind === "resize") {
      // Audio edits snap finer (sixteenths), and stop at the end of the recording.
      const g = d.clip.audio ? grid / 4 : grid;
      let length = Math.max(g, d.clip.length_beats + snap((e.clientX - d.x0) / ppb, g));
      if (d.clip.audio) {
        const fileBpm = d.clip.audio.source_bpm ?? project.tempo_bpm;
        const available = ((d.clip.audio.file_seconds - d.clip.audio.offset_seconds) * fileBpm) / 60;
        length = Math.min(length, Math.max(available, g));
      }
      if (length !== d.lastLength) {
        d.lastLength = length;
        void props.onCommand({ command: "resize_clip", clip_id: d.clip.id, length_beats: length });
      }
    } else if (d.kind === "trim") {
      const g = d.clip.audio ? grid / 4 : grid;
      const end = d.clip.start_beats + d.clip.length_beats;
      // Can't reveal audio from before the recording started.
      const earliest = d.clip.audio
        ? d.clip.start_beats - (d.clip.audio.offset_seconds * (d.clip.audio.source_bpm ?? project.tempo_bpm)) / 60
        : 0;
      const start = Math.min(
        end - g,
        Math.max(earliest, 0, d.clip.start_beats + snap((e.clientX - d.x0) / ppb, g)),
      );
      if (start !== d.lastStart) {
        d.lastStart = start;
        if (d.clip.audio) {
          void props.onCommand({ command: "trim_clip_start", clip_id: d.clip.id, start_beats: start });
        } else {
          setTrimPreview({ clipId: d.clip.id, start });
        }
      }
    } else if (d.kind === "chord") {
      const start = Math.max(0, d.chord.start_beats + snap((e.clientX - d.x0) / ppb, grid));
      const taken = chords.some((c) => c.id !== d.chord.id && Math.abs(c.start_beats - start) < 1e-9);
      if (start !== d.lastStart && !taken) {
        d.lastStart = start;
        d.moved = true;
        void props.onCommand({ command: "move_chord", chord_id: d.chord.id, start_beats: start });
      }
    } else if (d.kind === "marker") {
      const start = Math.max(0, d.marker.start_beats + snap((e.clientX - d.x0) / ppb, grid));
      const taken = markers.some((m) => m.id !== d.marker.id && Math.abs(m.start_beats - start) < 1e-9);
      if (start !== d.lastStart && !taken) {
        d.lastStart = start;
        void props.onCommand({ command: "move_marker", marker_id: d.marker.id, start_beats: start });
      }
    } else {
      const here = snap(beatAt(e.clientX), beatsPerBar);
      const [start, end] = here >= d.anchor ? [d.anchor, here] : [here, d.anchor];
      if (end - start >= beatsPerBar) {
        void props.onCommand({ command: "set_loop", enabled: true, start_beats: start, end_beats: end });
      }
    }
  };

  const onPointerUp = (e: ReactPointerEvent) => {
    const d = drag.current;
    if (!d) return;
    drag.current = null;
    // A click (no drag) on a chord opens its menu.
    if (d.kind === "chord" && !d.moved) {
      setChordMenu({ id: d.chord.id, x: e.clientX, y: e.clientY });
    }
    if (d.kind === "trim" && !d.clip.audio) {
      const changed = d.lastStart !== d.clip.start_beats;
      void (changed
        ? props.onCommand({ command: "trim_clip_start", clip_id: d.clip.id, start_beats: d.lastStart })
        : Promise.resolve()
      ).finally(() => {
        setTrimPreview(null);
        props.onEndGesture();
      });
      return;
    }
    props.onEndGesture();
  };

  const createClipAt = async (e: React.MouseEvent, track: Track) => {
    const start = snapDown(beatAt(e.clientX), beatsPerBar);
    const before = new Set(track.clips.map((c) => c.id));
    const p = await props.onCommand({
      command: "create_clip",
      track_id: track.id,
      start_beats: start,
      length_beats: beatsPerBar,
      name: null,
      notes: [],
    });
    const created = p?.tracks.find((t) => t.id === track.id)?.clips.find((c) => !before.has(c.id));
    if (created) props.onOpenClip(created.id);
  };

  /** Adds the key's home chord on the bar under the pointer and opens it. */
  const addChordAt = async (clientX: number, clientY: number) => {
    const start = snapDown(beatAt(clientX), beatsPerBar);
    if (chords.some((c) => Math.abs(c.start_beats - start) < 1e-9)) return;
    const home = homeChord(project.key);
    const p = await props.onCommand({ command: "add_chord", start_beats: start, ...home, bass: null });
    props.onEndGesture();
    const created = p?.chords?.find((c) => Math.abs(c.start_beats - start) < 1e-9);
    if (created) setChordMenu({ id: created.id, x: clientX, y: clientY });
  };
  const editChord = (c: Chord, change: { root?: number; quality?: ChordQuality; bass?: number | null }) =>
    void props
      .onCommand({
        command: "set_chord",
        chord_id: c.id,
        root: change.root ?? c.root,
        quality: change.quality ?? c.quality,
        bass: change.bass === undefined ? (c.bass ?? null) : change.bass,
      })
      .then(props.onEndGesture);
  const menuChord = chordMenu ? chords.find((c) => c.id === chordMenu.id) : undefined;

  /** Adds a marker on the bar under the pointer and starts naming it. */
  const addMarkerAt = async (clientX: number) => {
    const start = snapDown(beatAt(clientX), beatsPerBar);
    if (markers.some((m) => Math.abs(m.start_beats - start) < 1e-9)) return;
    const p = await props.onCommand({
      command: "add_marker",
      name: `Section ${markers.length + 1}`,
      start_beats: start,
    });
    props.onEndGesture();
    const created = p?.markers?.find((m) => Math.abs(m.start_beats - start) < 1e-9);
    if (created) setRenamingMarker(created.id);
  };

  const loop = project.loop_region;
  const bars = Math.ceil(beats / beatsPerBar);

  return (
    <div className="timeline" onPointerMove={onPointerMove} onPointerUp={onPointerUp} onPointerCancel={onPointerUp}>
      <div className="timeline-content" style={{ width: width + HEADER_WIDTH_PX }}>
        <div className="timeline-row ruler-row">
          <div className="track-header ruler-corner">
            <button className="small" onClick={() => props.onZoom(Math.max(4, ppb / 1.5))} title="Zoom out">
              −
            </button>
            <button className="small" onClick={() => props.onZoom(Math.min(128, ppb * 1.5))} title="Zoom in">
              +
            </button>
          </div>
          <div className="ruler-stack" style={{ width }}>
          <div
            className="ruler"
            style={{ width }}
            onPointerDown={(e) => {
              const rect = e.currentTarget.getBoundingClientRect();
              if (e.clientY - rect.top < 10) {
                e.currentTarget.setPointerCapture?.(e.pointerId);
                drag.current = { kind: "loop", anchor: snapDown(beatAt(e.clientX), beatsPerBar) };
              } else {
                props.onLocate(snapDown(beatAt(e.clientX), 1));
              }
            }}
            title="Click to move the playhead. Drag along the top strip to set the loop."
          >
            <div
              className={loop.enabled ? "loop-region on" : "loop-region"}
              style={{ left: loop.start_beats * ppb, width: (loop.end_beats - loop.start_beats) * ppb }}
            />
            {Array.from({ length: bars }, (_, i) => (
              <span key={i} className="bar-label" style={{ left: i * beatsPerBar * ppb }}>
                {i + 1}
              </span>
            ))}
          </div>
          <div
            className="marker-strip"
            role="group"
            aria-label="Section markers"
            title="Double-click to mark where a section starts (for example Explore, Combat). Drag a marker to move it."
            onDoubleClick={(e) => {
              if (e.target === e.currentTarget) void addMarkerAt(e.clientX);
            }}
          >
            {markers.length === 0 && <span className="marker-hint">Double-click here to mark sections</span>}
            {markers.map((m) => (
              <div
                key={m.id}
                className="marker"
                style={{ left: m.start_beats * ppb }}
                onPointerDown={(e) => {
                  if (e.button !== 0 || renamingMarker === m.id) return;
                  e.stopPropagation();
                  e.currentTarget.setPointerCapture?.(e.pointerId);
                  drag.current = { kind: "marker", marker: m, x0: e.clientX, lastStart: m.start_beats };
                }}
                onDoubleClick={(e) => {
                  e.stopPropagation();
                  setRenamingMarker(m.id);
                }}
                title={`${m.name}: double-click to rename, drag to move`}
              >
                {renamingMarker === m.id ? (
                  <input
                    autoFocus
                    aria-label="Marker name"
                    defaultValue={m.name}
                    onPointerDown={(e) => e.stopPropagation()}
                    onKeyDown={(e) => {
                      e.stopPropagation();
                      if (e.key === "Enter") e.currentTarget.blur();
                      if (e.key === "Escape") setRenamingMarker(null);
                    }}
                    onBlur={(e) => {
                      setRenamingMarker(null);
                      const name = e.target.value.trim();
                      if (name && name !== m.name) {
                        void props
                          .onCommand({ command: "rename_marker", marker_id: m.id, name })
                          .then(props.onEndGesture);
                      }
                    }}
                  />
                ) : (
                  <span className="marker-name">{m.name}</span>
                )}
                <button
                  className="marker-delete"
                  aria-label={`Delete marker ${m.name}`}
                  onPointerDown={(e) => e.stopPropagation()}
                  onClick={(e) => {
                    e.stopPropagation();
                    void props.onCommand({ command: "remove_marker", marker_id: m.id }).then(props.onEndGesture);
                  }}
                >
                  ✕
                </button>
              </div>
            ))}
          </div>
          <div
            className="chord-strip"
            role="group"
            aria-label="Chord track"
            title="The chord track: double-click to add a chord, click one to change it, drag to move it. It makes no sound; it guides the notes."
            onDoubleClick={(e) => {
              if (e.target === e.currentTarget) void addChordAt(e.clientX, e.clientY);
            }}
          >
            {chords.length === 0 && <span className="marker-hint">Double-click here to add chords</span>}
            {chords.map((c, i) => {
              const next = chords[i + 1]?.start_beats ?? c.start_beats + beatsPerBar;
              return (
                <button
                  key={c.id}
                  className="chord"
                  style={{ left: c.start_beats * ppb, width: Math.max(18, (next - c.start_beats) * ppb - 2) }}
                  aria-label={`Chord ${chordName(c)}`}
                  onPointerDown={(e) => {
                    if (e.button !== 0) return;
                    e.stopPropagation();
                    e.currentTarget.setPointerCapture?.(e.pointerId);
                    drag.current = { kind: "chord", chord: c, x0: e.clientX, lastStart: c.start_beats, moved: false };
                  }}
                >
                  {chordName(c)}
                </button>
              );
            })}
          </div>
          </div>
        </div>
        {menuChord && chordMenu && (
          <div
            className="step-menu chord-menu"
            role="dialog"
            aria-label={`Edit chord ${chordName(menuChord)}`}
            style={{ left: chordMenu.x, top: chordMenu.y + 12 }}
          >
            <div className="step-menu-row">
              <span>Root</span>
              <select
                aria-label="Chord root"
                value={menuChord.root}
                onChange={(e) => editChord(menuChord, { root: Number(e.target.value) })}
              >
                {NOTE_NAMES.map((n, i) => (
                  <option key={n} value={i}>
                    {n}
                  </option>
                ))}
              </select>
              <select
                aria-label="Chord type"
                value={menuChord.quality}
                onChange={(e) => editChord(menuChord, { quality: e.target.value as ChordQuality })}
              >
                {CHORD_QUALITIES.map((q) => (
                  <option key={q.quality} value={q.quality}>
                    {q.symbol === "" ? "major" : q.symbol}
                  </option>
                ))}
              </select>
            </div>
            <div className="step-menu-row">
              <span>Bass</span>
              <select
                aria-label="Chord bass note"
                value={menuChord.bass ?? ""}
                onChange={(e) => editChord(menuChord, { bass: e.target.value === "" ? null : Number(e.target.value) })}
              >
                <option value="">Root</option>
                {NOTE_NAMES.map((n, i) => (
                  <option key={n} value={i}>
                    /{n}
                  </option>
                ))}
              </select>
            </div>
            <div className="step-menu-row">
              <button
                className="small"
                onClick={() => {
                  setChordMenu(null);
                  void props.onCommand({ command: "remove_chord", chord_id: menuChord.id }).then(props.onEndGesture);
                }}
              >
                Delete chord
              </button>
              <button className="small" onClick={() => setChordMenu(null)}>
                Done
              </button>
            </div>
          </div>
        )}

        <div className="timeline-body">
          <div className="headers-column">
            {project.tracks.map((t, i) => (
              <Fragment key={t.id}>
              <div
                className={t.id === props.selectedTrackId ? "track-header selected" : "track-header"}
                style={{ height: laneHeight(t) }}
                onClick={() => props.onSelectTrack(t.id)}
              >
                <span className="track-icon" aria-hidden>
                  {TRACK_ICONS[t.instrument.kind]}
                </span>
                <div className="track-header-main">
                  {renaming === t.id ? (
                    <input
                      autoFocus
                      aria-label="Track name"
                      defaultValue={t.name}
                      onBlur={(e) => {
                        setRenaming(null);
                        const name = e.target.value.trim();
                        if (name && name !== t.name) {
                          void props.onCommand({ command: "rename_track", track_id: t.id, name });
                        }
                      }}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") e.currentTarget.blur();
                        if (e.key === "Escape") setRenaming(null);
                      }}
                    />
                  ) : (
                    <span className="track-name" onDoubleClick={() => setRenaming(t.id)} title="Double-click to rename">
                      {props.recording && t.id === props.selectedTrackId && <span className="rec-dot">● </span>}
                      {t.name}
                    </span>
                  )}
                  <div className="track-controls">
                    <button
                      className={t.mixer.mute ? "tiny on mute" : "tiny"}
                      aria-pressed={t.mixer.mute}
                      title="Mute"
                      onClick={(e) => {
                        e.stopPropagation();
                        void props.onCommand({
                          command: "set_track_mixer",
                          track_id: t.id,
                          volume_db: null,
                          pan: null,
                          mute: !t.mixer.mute,
                          solo: null,
                        });
                        props.onEndGesture();
                      }}
                    >
                      M
                    </button>
                    <button
                      className={t.mixer.solo ? "tiny on solo" : "tiny"}
                      aria-pressed={t.mixer.solo}
                      title="Solo"
                      onClick={(e) => {
                        e.stopPropagation();
                        void props.onCommand({
                          command: "set_track_mixer",
                          track_id: t.id,
                          volume_db: null,
                          pan: null,
                          mute: null,
                          solo: !t.mixer.solo,
                        });
                        props.onEndGesture();
                      }}
                    >
                      S
                    </button>
                    <div className="track-meter" aria-hidden>
                      <div style={{ width: `${meterPercent(props.trackPeaks[i] ?? 0)}%` }} />
                    </div>
                    {isAudio(t) && lanesOf(t).count > 1 && (
                      <button
                        className={openTakes.has(t.id) ? "tiny on" : "tiny"}
                        aria-pressed={openTakes.has(t.id)}
                        aria-label={`Takes on ${t.name}`}
                        title="Show the takes recorded over each other, one per lane, to pick the best parts"
                        onClick={(e) => {
                          e.stopPropagation();
                          setOpenTakes((open) => {
                            const next = new Set(open);
                            if (next.has(t.id)) next.delete(t.id);
                            else next.add(t.id);
                            return next;
                          });
                        }}
                      >
                        T{lanesOf(t).count}
                      </button>
                    )}
                    {isAudio(t) && openTakes.has(t.id) && (
                      <button
                        className="tiny"
                        aria-label={`Split takes on ${t.name} at the playhead`}
                        title="Cut every take at the playhead, so you can pick a different take before and after"
                        onClick={(e) => {
                          e.stopPropagation();
                          splitTakesAt(t, props.playheadBeats);
                        }}
                      >
                        ✂
                      </button>
                    )}
                    {!isAudio(t) && (
                      <button
                        className={
                          t.frozen ? (props.frozenCurrent.has(t.id) ? "tiny on frozen" : "tiny stale") : "tiny"
                        }
                        aria-pressed={Boolean(t.frozen)}
                        aria-label={`${t.frozen ? "Unfreeze" : "Freeze"} ${t.name}`}
                        title={
                          !t.frozen
                            ? "Freeze: play a rendering of this track instead of its instrument and effects, to save CPU"
                            : props.frozenCurrent.has(t.id)
                              ? "Frozen (saving CPU). Click to unfreeze and play it live again."
                              : "Frozen, but out of date: you changed it, so it plays live. Click to freeze it again."
                        }
                        onClick={(e) => {
                          e.stopPropagation();
                          if (t.frozen && props.frozenCurrent.has(t.id)) {
                            void props.onCommand({ command: "unfreeze_track", track_id: t.id }).then(props.onEndGesture);
                          } else {
                            props.onFreeze(t.id);
                          }
                        }}
                      >
                        ❄
                      </button>
                    )}
                    <button
                      className={openAutomation.has(t.id) ? "tiny on" : "tiny"}
                      aria-pressed={openAutomation.has(t.id)}
                      aria-label={`Automation for ${t.name}`}
                      title="Show automation: draw volume, pan, or any setting changing over time"
                      onClick={(e) => {
                        e.stopPropagation();
                        toggleAutomation(t.id);
                      }}
                    >
                      A
                    </button>
                    <button
                      className="tiny delete"
                      title="Delete track"
                      aria-label={`Delete ${t.name}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        props.onRemoveTrack(t);
                      }}
                    >
                      ✕
                    </button>
                  </div>
                </div>
              </div>
              {openAutomation.has(t.id) && (
                <AutomationHeader
                  track={t}
                  catalog={props.catalog}
                  laneId={laneOf(t)?.id}
                  onPick={(v) => void pickLane(t, v)}
                  onCommand={props.onCommand}
                  onEndGesture={props.onEndGesture}
                />
              )}
            </Fragment>
            ))}
            <div className="add-track">
              <button
                className="small"
                onClick={() =>
                  void props.onCommand({ command: "add_track", name: "Synth", instrument: "synth", preset: null, index: null })
                }
              >
                + Synth track
              </button>
              <button
                className="small"
                onClick={() =>
                  void props.onCommand({ command: "add_track", name: "Drums", instrument: "drums", preset: null, index: null })
                }
              >
                + Drum track
              </button>
              <button
                className="small"
                onClick={() =>
                  void props.onCommand({ command: "add_track", name: "Piano", instrument: "sampler", preset: null, index: null })
                }
                title="Plays a sample pack: a real recorded piano, bass, strings..."
              >
                + Sampler track
              </button>
              <button
                className="small"
                onClick={() =>
                  void props.onCommand({ command: "add_track", name: "Audio", instrument: "audio", preset: null, index: null })
                }
                title="A track for recording your voice or an instrument, or for imported audio"
              >
                + Audio track
              </button>
              <button
                className="small"
                onClick={() => props.onImportAudio(null, snapDown(Math.max(0, props.playheadBeats), beatsPerBar))}
                title="Bring in WAV, MP3, M4A (phone recordings), FLAC or OGG files. You can also drag files onto the timeline."
              >
                Import audio…
              </button>
            </div>
          </div>

          <div
            className="lanes"
            ref={lanesRef}
            style={{
              width,
              backgroundSize: `${ppb * beatsPerBar}px ${TRACK_HEIGHT}px, ${ppb}px ${TRACK_HEIGHT}px`,
            }}
          >
            {project.tracks.map((t, i) => (
              <Fragment key={t.id}>
              <div
                className={t.id === props.selectedTrackId ? "lane selected" : "lane"}
                style={{ height: laneHeight(t) }}
                data-track={t.id}
                onPointerDown={() => {
                  props.onSelectTrack(t.id);
                  props.onSelectClip(null, t.id);
                }}
                onDoubleClick={(e) =>
                  isAudio(t)
                    ? props.onImportAudio(t.id, snapDown(beatAt(e.clientX), beatsPerBar))
                    : void createClipAt(e, t)
                }
                title={isAudio(t) ? "Double-click to import audio here, or select the track and press R to record" : "Double-click to add a clip"}
              >
                {/* Muted takes first, so the ones heard are drawn on top. */}
                {[...t.clips].sort((a, b) => Number(Boolean(b.muted)) - Number(Boolean(a.muted))).map((c) =>
                  c.audio ? (
                    <AudioClipBox
                      key={c.id}
                      clip={c}
                      ppb={ppb}
                      tempoBpm={project.tempo_bpm}
                      peaks={props.peaks[c.audio.file]}
                      missing={props.missingAudio.has(c.audio.file)}
                      selected={c.id === props.selectedClipId}
                      lane={
                        openTakes.has(t.id)
                          ? { top: (lanesOf(t).lane.get(c.id) ?? 0) * TAKE_LANE_HEIGHT, height: TAKE_LANE_HEIGHT }
                          : undefined
                      }
                      onUse={
                        openTakes.has(t.id) && c.muted
                          ? () => void props.onCommand({ command: "comp_take", clip_id: c.id }).then(props.onEndGesture)
                          : undefined
                      }
                      onPointerDown={(e, edge) => startClipDrag(e, c, i, edge)}
                    />
                  ) : (
                    <ClipBox
                      key={c.id}
                      clip={trimPreview?.clipId === c.id ? trimmedClip(c, trimPreview.start) : c}
                      ppb={ppb}
                      selected={c.id === props.selectedClipId}
                      drums={t.instrument.kind === "drums"}
                      onPointerDown={(e, edge) => startClipDrag(e, c, i, edge)}
                      onDoubleClick={() => props.onOpenClip(c.id)}
                    />
                  ),
                )}
              </div>
              {openAutomation.has(t.id) && (
                <div className="automation-row" style={{ height: AUTOMATION_HEIGHT }}>
                  {(() => {
                    const lane = laneOf(t);
                    const scale = lane ? targetScale(lane.target, t, props.catalog) : null;
                    if (!lane || !scale) {
                      return <span className="automation-hint">Pick a setting on the left to automate it.</span>;
                    }
                    return (
                      <AutomationLaneEditor
                        lane={lane}
                        scale={scale}
                        ppb={ppb}
                        width={width}
                        grid={grid / 4}
                        onPoints={(points) =>
                          void props.onCommand({ command: "set_automation_points", track_id: t.id, lane_id: lane.id, points })
                        }
                        onEndGesture={props.onEndGesture}
                      />
                    );
                  })()}
                </div>
              )}
            </Fragment>
            ))}
            {markers.map((m) => (
              <div key={m.id} className="marker-line" style={{ left: m.start_beats * ppb }} aria-hidden />
            ))}
            <div className="playhead" style={{ left: props.playheadBeats * ppb }} aria-hidden />
          </div>
        </div>
      </div>
    </div>
  );
}

export const HEADER_WIDTH_PX = 220;

interface ClipBoxProps {
  clip: Clip;
  ppb: number;
  selected: boolean;
  drums: boolean;
  onPointerDown: (e: ReactPointerEvent, edge: "move" | "end" | "start") => void;
  onDoubleClick: () => void;
}

/** How a note clip looks with its start moved to `start` (end kept): what
 * trim_clip_start will make of it. */
function trimmedClip(clip: Clip, start: number): Clip {
  const delta = start - clip.start_beats;
  return {
    ...clip,
    start_beats: start,
    length_beats: clip.start_beats + clip.length_beats - start,
    notes: clip.notes
      .map((n) => ({ ...n, start_beats: n.start_beats - delta }))
      .filter((n) => n.start_beats >= 0),
  };
}

function ClipBox({ clip, ppb, selected, drums, onPointerDown, onDoubleClick }: ClipBoxProps) {
  const pitches = clip.notes.map((n) => n.pitch);
  const lo = Math.min(...pitches, 127);
  const hi = Math.max(...pitches, 0);
  const span = Math.max(12, hi - lo + 1);
  const inner = TRACK_HEIGHT - 20;
  return (
    <div
      className={`clip${selected ? " selected" : ""}${drums ? " drums" : ""}${clip.muted ? " muted-clip" : ""}`}
      style={{ left: clip.start_beats * ppb, width: Math.max(4, clip.length_beats * ppb) }}
      onPointerDown={(e) => onPointerDown(e, "move")}
      onDoubleClick={(e) => {
        e.stopPropagation();
        onDoubleClick();
      }}
      title={`${clip.name} — double-click to edit notes`}
      data-clip={clip.id}
    >
      <span className="clip-name">
        {clip.link ? "🔗 " : ""}
        {clip.name}
      </span>
      <div className="clip-notes">
        {clip.notes.slice(0, 600).map((n) =>
          n.start_beats < clip.length_beats ? (
            <div
              key={n.id}
              className="clip-note"
              style={{
                left: n.start_beats * ppb,
                width: Math.max(1, Math.min(n.length_beats, clip.length_beats - n.start_beats) * ppb - 1),
                top: ((hi - n.pitch) / span) * inner,
                height: Math.max(2, inner / span),
              }}
            />
          ) : null,
        )}
      </div>
      <div className="clip-trim" onPointerDown={(e) => onPointerDown(e, "start")} title="Drag to trim the start" />
      <div
        className="clip-resize"
        onPointerDown={(e) => onPointerDown(e, "end")}
        title="Drag to change the clip length"
      />
    </div>
  );
}

interface AudioClipBoxProps {
  clip: Clip;
  ppb: number;
  tempoBpm: number;
  peaks: Peaks | undefined;
  missing: boolean;
  selected: boolean;
  /** Where it sits when the track shows its takes on lanes. */
  lane?: { top: number; height: number };
  /** For an unused take in the lanes view: make it the one heard. */
  onUse?: () => void;
  onPointerDown: (e: ReactPointerEvent, edge: "move" | "end" | "start") => void;
}

/** An audio clip: waveform, fades, and handles to trim either end. */
function AudioClipBox({ clip, ppb, tempoBpm, peaks, missing, selected, lane, onUse, onPointerDown }: AudioClipBoxProps) {
  const audio = clip.audio;
  if (!audio) return null;
  const width = Math.max(4, clip.length_beats * ppb);
  const secondsToPx = (s: number) => ((s * tempoBpm) / 60) * ppb;
  const gain = 10 ** (audio.gain_db / 20);
  // How much of the file the clip shows (at the recording's own tempo when
  // it follows the song's).
  const seconds = (clip.length_beats * 60) / (audio.source_bpm ?? tempoBpm);
  return (
    <div
      className={`clip audio${selected ? " selected" : ""}${missing ? " missing" : ""}${clip.muted ? " muted-clip" : ""}`}
      style={
        lane
          ? { left: clip.start_beats * ppb, width, top: lane.top + 2, height: lane.height - 4, bottom: "auto" }
          : { left: clip.start_beats * ppb, width }
      }
      onPointerDown={(e) => onPointerDown(e, "move")}
      title={
        missing
          ? `${clip.name}: the audio file ${audio.file} is missing`
          : `${clip.name} — drag the edges to trim${audio.source_bpm ? ` (follows the tempo; recorded at ${audio.source_bpm} BPM)` : ""}`
      }
      data-clip={clip.id}
    >
      <span className="clip-name">
        {missing && "⚠ "}
        {audio.source_bpm ? "⇿ " : ""}
        {clip.name}
      </span>
      {peaks && !missing && (
        <Waveform
          peaks={peaks}
          offsetSeconds={audio.offset_seconds}
          seconds={seconds}
          gain={gain}
          width={width}
          height={(lane ? lane.height : TRACK_HEIGHT) - 18}
        />
      )}
      {audio.fade_in_seconds > 0 && (
        <div className="clip-fade in" style={{ width: Math.min(width, secondsToPx(audio.fade_in_seconds)) }} aria-hidden />
      )}
      {audio.fade_out_seconds > 0 && (
        <div className="clip-fade out" style={{ width: Math.min(width, secondsToPx(audio.fade_out_seconds)) }} aria-hidden />
      )}
      {onUse && (
        <button
          className="tiny take-use"
          aria-label={`Use take ${clip.name}`}
          title="Make this take the one heard here (the others are kept, muted)"
          onPointerDown={(e) => e.stopPropagation()}
          onClick={(e) => {
            e.stopPropagation();
            onUse();
          }}
        >
          ▶ Use
        </button>
      )}
      <div className="clip-trim" onPointerDown={(e) => onPointerDown(e, "start")} title="Drag to trim the start" />
      <div className="clip-resize" onPointerDown={(e) => onPointerDown(e, "end")} title="Drag to trim the end" />
    </div>
  );
}

interface AutomationHeaderProps {
  track: Track;
  catalog: Catalog;
  laneId: number | undefined;
  onPick: (value: string) => void;
  onCommand: (command: Command) => Promise<Project | undefined>;
  onEndGesture: () => void;
}

/** Left side of an automation row: which setting, on/off, delete. */
function AutomationHeader({ track, catalog, laneId, onPick, onCommand, onEndGesture }: AutomationHeaderProps) {
  const lanes = track.automation ?? [];
  const lane = lanes.find((l) => l.id === laneId);
  const unused = automatableTargets(track, catalog).filter(
    (o) => !lanes.some((l) => JSON.stringify(l.target) === JSON.stringify(o.target)),
  );
  return (
    <div className="automation-header" style={{ height: AUTOMATION_HEIGHT }}>
      <select
        aria-label={`Automation lane for ${track.name}`}
        value={lane ? String(lane.id) : ""}
        onChange={(e) => {
          onPick(e.target.value);
          e.currentTarget.blur();
        }}
      >
        {!lane && <option value="">Automate…</option>}
        {lanes.map((l) => (
          <option key={l.id} value={l.id}>
            {targetScale(l.target, track, catalog)?.label ?? "Missing setting"}
          </option>
        ))}
        <optgroup label="Add a lane">
          {unused.map((o) => (
            <option key={o.label} value={`new:${JSON.stringify(o.target)}`}>
              + {o.label}
            </option>
          ))}
        </optgroup>
      </select>
      {lane && (
        <div className="track-controls">
          <button
            className={lane.enabled ? "tiny on" : "tiny"}
            aria-pressed={lane.enabled}
            title={lane.enabled ? "Automation on (click to bypass)" : "Automation bypassed"}
            onClick={() => {
              void onCommand({ command: "set_automation_enabled", track_id: track.id, lane_id: lane.id, enabled: !lane.enabled });
              onEndGesture();
            }}
          >
            On
          </button>
          <button
            className="tiny delete"
            aria-label="Delete automation lane"
            title="Delete this automation lane"
            onClick={() => {
              void onCommand({ command: "remove_automation_lane", track_id: track.id, lane_id: lane.id });
              onEndGesture();
            }}
          >
            ✕
          </button>
        </div>
      )}
    </div>
  );
}
