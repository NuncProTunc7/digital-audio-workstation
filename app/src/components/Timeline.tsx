import { useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import { meterPercent, snap, snapDown } from "../format";
import type { Clip, Command, Project, Track } from "../types";

export const TRACK_HEIGHT = 60;
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
}

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
  | { kind: "loop"; anchor: number };

/** The arrangement: track headers, clip lanes, ruler, loop region, playhead. */
export default function Timeline(props: TimelineProps) {
  const { project, pixelsPerBeat: ppb } = props;
  const beatsPerBar = project.time_signature.numerator;
  const totalBeats = MIN_BARS * beatsPerBar;
  const lanesRef = useRef<HTMLDivElement>(null);
  const drag = useRef<Drag | null>(null);
  const [renaming, setRenaming] = useState<number | null>(null);

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
    const i = Math.floor((clientY - rect.top) / TRACK_HEIGHT);
    return Math.max(0, Math.min(project.tracks.length - 1, i));
  };

  const startClipDrag = (e: ReactPointerEvent, clip: Clip, trackIndex: number, resize: boolean) => {
    e.stopPropagation();
    e.currentTarget.setPointerCapture?.(e.pointerId);
    props.onSelectClip(clip.id, project.tracks[trackIndex].id);
    drag.current = resize
      ? { kind: "resize", clip, x0: e.clientX, lastLength: clip.length_beats }
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
      const trackIndex = trackIndexAt(e.clientY);
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
      const length = Math.max(grid, d.clip.length_beats + snap((e.clientX - d.x0) / ppb, grid));
      if (length !== d.lastLength) {
        d.lastLength = length;
        void props.onCommand({ command: "resize_clip", clip_id: d.clip.id, length_beats: length });
      }
    } else {
      const here = snap(beatAt(e.clientX), beatsPerBar);
      const [start, end] = here >= d.anchor ? [d.anchor, here] : [here, d.anchor];
      if (end - start >= beatsPerBar) {
        void props.onCommand({ command: "set_loop", enabled: true, start_beats: start, end_beats: end });
      }
    }
  };

  const onPointerUp = () => {
    if (drag.current) {
      drag.current = null;
      props.onEndGesture();
    }
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
        </div>

        <div className="timeline-body">
          <div className="headers-column">
            {project.tracks.map((t, i) => (
              <div
                key={t.id}
                className={t.id === props.selectedTrackId ? "track-header selected" : "track-header"}
                style={{ height: TRACK_HEIGHT }}
                onClick={() => props.onSelectTrack(t.id)}
              >
                <span className="track-icon" aria-hidden>
                  {t.instrument.kind === "drums" ? "🥁" : "🎹"}
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
              <div
                key={t.id}
                className={t.id === props.selectedTrackId ? "lane selected" : "lane"}
                style={{ height: TRACK_HEIGHT }}
                data-track={t.id}
                onPointerDown={() => {
                  props.onSelectTrack(t.id);
                  props.onSelectClip(null, t.id);
                }}
                onDoubleClick={(e) => void createClipAt(e, t)}
                title="Double-click to add a clip"
              >
                {t.clips.map((c) => (
                  <ClipBox
                    key={c.id}
                    clip={c}
                    ppb={ppb}
                    selected={c.id === props.selectedClipId}
                    drums={t.instrument.kind === "drums"}
                    onPointerDown={(e, resize) => startClipDrag(e, c, i, resize)}
                    onDoubleClick={() => props.onOpenClip(c.id)}
                  />
                ))}
              </div>
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
  onPointerDown: (e: ReactPointerEvent, resize: boolean) => void;
  onDoubleClick: () => void;
}

function ClipBox({ clip, ppb, selected, drums, onPointerDown, onDoubleClick }: ClipBoxProps) {
  const pitches = clip.notes.map((n) => n.pitch);
  const lo = Math.min(...pitches, 127);
  const hi = Math.max(...pitches, 0);
  const span = Math.max(12, hi - lo + 1);
  const inner = TRACK_HEIGHT - 20;
  return (
    <div
      className={`clip${selected ? " selected" : ""}${drums ? " drums" : ""}`}
      style={{ left: clip.start_beats * ppb, width: Math.max(4, clip.length_beats * ppb) }}
      onPointerDown={(e) => onPointerDown(e, false)}
      onDoubleClick={(e) => {
        e.stopPropagation();
        onDoubleClick();
      }}
      title={`${clip.name} — double-click to edit notes`}
      data-clip={clip.id}
    >
      <span className="clip-name">{clip.name}</span>
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
      <div
        className="clip-resize"
        onPointerDown={(e) => onPointerDown(e, true)}
        title="Drag to change the clip length"
      />
    </div>
  );
}
