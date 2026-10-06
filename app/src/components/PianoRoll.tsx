import { useEffect, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import { snap, snapDown } from "../format";
import { isBlackKey, noteName } from "../keymap";
import type { Clip, Command, DrumPad, Note, Track } from "../types";

const ROW_HEIGHT = 14;
const KEYS_WIDTH = 92;

const GRID_OPTIONS: { label: string; beats: number }[] = [
  { label: "1/4", beats: 1 },
  { label: "1/8", beats: 0.5 },
  { label: "1/8 triplet", beats: 1 / 3 },
  { label: "1/16", beats: 0.25 },
  { label: "1/16 triplet", beats: 1 / 6 },
  { label: "1/32", beats: 0.125 },
];

interface PianoRollProps {
  clip: Clip;
  track: Track;
  drumPads: DrumPad[];
  playheadBeats: number;
  beatsPerBar: number;
  selected: ReadonlySet<number>;
  onSelect: (ids: Set<number>) => void;
  onCommand: (command: Command) => Promise<unknown>;
  onEndGesture: () => void;
  onAudition: (note: number) => void;
}

type Drag =
  | { kind: "move"; x0: number; y0: number; originals: Note[]; lastKey: string }
  | { kind: "resize"; x0: number; originals: Note[]; lastKey: string };

/** Note editor for one clip. Click to add, drag to move, drag the right edge to resize. */
export default function PianoRoll(props: PianoRollProps) {
  const { clip, track } = props;
  const [ppb, setPpb] = useState(64);
  const [grid, setGrid] = useState(0.25);
  const [noteLength, setNoteLength] = useState(1);
  const gridRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const drag = useRef<Drag | null>(null);
  const drums = track.instrument.kind === "drums";
  const padNames = new Map(props.drumPads.map((p) => [p.note, p.name]));

  // Show some room after the clip so notes can be added past its end.
  const visibleBeats = Math.max(clip.length_beats + props.beatsPerBar, props.beatsPerBar * 4);
  const width = visibleBeats * ppb;

  // Scroll to the notes (or middle C / the drum kit) when a clip opens.
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const pitches = clip.notes.map((n) => n.pitch);
    const center = pitches.length > 0 ? (Math.max(...pitches) + Math.min(...pitches)) / 2 : drums ? 43 : 60;
    el.scrollTop = (127 - center) * ROW_HEIGHT - el.clientHeight / 2;
    // Only when switching clips, not on every edit.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [clip.id]);

  const pointToBeatPitch = (clientX: number, clientY: number) => {
    const rect = gridRef.current?.getBoundingClientRect();
    if (!rect) return { beat: 0, pitch: 60 };
    return {
      beat: Math.max(0, (clientX - rect.left) / ppb),
      pitch: Math.max(0, Math.min(127, 127 - Math.floor((clientY - rect.top) / ROW_HEIGHT))),
    };
  };

  const addNote = async (e: React.PointerEvent) => {
    const { beat, pitch } = pointToBeatPitch(e.clientX, e.clientY);
    const start = snapDown(beat, grid);
    props.onAudition(pitch);
    await props.onCommand({
      command: "add_notes",
      clip_id: clip.id,
      notes: [{ pitch, start_beats: start, length_beats: drums ? Math.min(noteLength, grid) : noteLength, velocity: 100 }],
    });
    props.onEndGesture();
  };

  const startNoteDrag = (e: ReactPointerEvent, note: Note, resize: boolean) => {
    e.stopPropagation();
    e.currentTarget.setPointerCapture?.(e.pointerId);
    let ids = new Set(props.selected);
    if (e.shiftKey) {
      if (ids.has(note.id)) ids.delete(note.id);
      else ids.add(note.id);
    } else if (!ids.has(note.id)) {
      ids = new Set([note.id]);
    }
    props.onSelect(ids);
    if (!resize) props.onAudition(note.pitch);
    setNoteLength(note.length_beats);
    const originals = clip.notes.filter((n) => ids.has(n.id));
    drag.current = resize
      ? { kind: "resize", x0: e.clientX, originals, lastKey: "" }
      : { kind: "move", x0: e.clientX, y0: e.clientY, originals, lastKey: "" };
  };

  const onPointerMove = (e: ReactPointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const dBeats = snap((e.clientX - d.x0) / ppb, grid);
    if (d.kind === "move") {
      const dPitch = -Math.round((e.clientY - d.y0) / ROW_HEIGHT);
      const key = `${dBeats}:${dPitch}`;
      if (key === d.lastKey) return;
      d.lastKey = key;
      void props.onCommand({
        command: "edit_notes",
        clip_id: clip.id,
        edits: d.originals.map((n) => ({
          id: n.id,
          pitch: Math.max(0, Math.min(127, n.pitch + dPitch)),
          start_beats: Math.max(0, n.start_beats + dBeats),
          length_beats: null,
          velocity: null,
        })),
      });
    } else {
      const key = `${dBeats}`;
      if (key === d.lastKey) return;
      d.lastKey = key;
      const lengths = d.originals.map((n) => Math.max(grid / 2, n.length_beats + dBeats));
      setNoteLength(lengths[0] ?? noteLength);
      void props.onCommand({
        command: "edit_notes",
        clip_id: clip.id,
        edits: d.originals.map((n, i) => ({
          id: n.id,
          pitch: null,
          start_beats: null,
          length_beats: lengths[i],
          velocity: null,
        })),
      });
    }
  };

  const onPointerUp = () => {
    if (drag.current) {
      drag.current = null;
      props.onEndGesture();
    }
  };

  const selectedNotes = clip.notes.filter((n) => props.selected.has(n.id));
  const targetIds = selectedNotes.length > 0 ? selectedNotes.map((n) => n.id) : null;
  const velocity = selectedNotes.length > 0 ? selectedNotes[0].velocity : 100;
  const rows = Array.from({ length: 128 }, (_, i) => 127 - i);

  return (
    <div className="piano-roll">
      <div className="roll-toolbar">
        <strong>{clip.name}</strong>
        <span className="muted">
          {clip.notes.length} notes · {clip.length_beats / props.beatsPerBar} bars
        </span>
        <label className="field">
          <span>Grid</span>
          <select
            aria-label="Grid"
            value={grid}
            onChange={(e) => {
              setGrid(Number(e.target.value));
              e.currentTarget.blur();
            }}
          >
            {GRID_OPTIONS.map((g) => (
              <option key={g.label} value={g.beats}>
                {g.label}
              </option>
            ))}
          </select>
        </label>
        <button
          onClick={() =>
            void props
              .onCommand({
                command: "quantize_notes",
                clip_id: clip.id,
                grid_beats: grid,
                strength: null,
                lengths: false,
                note_ids: targetIds,
              })
              .then(props.onEndGesture)
          }
          title="Snap note starts to the grid (selected notes, or all)"
        >
          Quantize
        </button>
        <div className="button-group" role="group" aria-label="Transpose">
          {[-12, -1, 1, 12].map((st) => (
            <button
              key={st}
              className="small"
              onClick={() =>
                void props
                  .onCommand({ command: "transpose_notes", clip_id: clip.id, semitones: st, note_ids: targetIds })
                  .then(props.onEndGesture)
              }
              title={`Transpose ${st > 0 ? "up" : "down"} ${Math.abs(st) === 12 ? "an octave" : "a semitone"}`}
            >
              {st > 0 ? `+${st}` : st}
            </button>
          ))}
        </div>
        <label className="field" title="Velocity of the selected notes">
          <span>Velocity</span>
          <input
            type="range"
            min={1}
            max={127}
            value={velocity}
            disabled={selectedNotes.length === 0}
            aria-label="Velocity"
            onChange={(e) =>
              void props.onCommand({
                command: "edit_notes",
                clip_id: clip.id,
                edits: selectedNotes.map((n) => ({
                  id: n.id,
                  pitch: null,
                  start_beats: null,
                  length_beats: null,
                  velocity: Number(e.target.value),
                })),
              })
            }
            onPointerUp={props.onEndGesture}
          />
          <span className="param-value">{selectedNotes.length > 0 ? velocity : "–"}</span>
        </label>
        <span className="spacer" />
        <button className="small" onClick={() => setPpb((p) => Math.max(16, p / 1.5))} title="Zoom out">
          −
        </button>
        <button className="small" onClick={() => setPpb((p) => Math.min(256, p * 1.5))} title="Zoom in">
          +
        </button>
      </div>

      <div className="roll-scroll" ref={scrollRef} onPointerMove={onPointerMove} onPointerUp={onPointerUp}>
        <div className="roll-content" style={{ width: width + KEYS_WIDTH, height: 128 * ROW_HEIGHT }}>
          <div className="roll-keys" style={{ width: KEYS_WIDTH }}>
            {rows.map((p) => (
              <div
                key={p}
                className={`roll-key${isBlackKey(p) ? " black" : ""}${drums && padNames.has(p) ? " drum-row" : ""}`}
                style={{ height: ROW_HEIGHT }}
                onPointerDown={() => props.onAudition(p)}
              >
                {drums ? (padNames.get(p) ?? "") : p % 12 === 0 ? noteName(p) : ""}
              </div>
            ))}
          </div>
          <div
            className="roll-grid"
            ref={gridRef}
            style={{
              width,
              backgroundSize: `${ppb * props.beatsPerBar}px 100%, ${ppb}px 100%, ${ppb * grid}px 100%, 100% ${ROW_HEIGHT * 12}px`,
            }}
            onPointerDown={(e) => {
              if (e.button !== 0) return;
              props.onSelect(new Set());
              void addNote(e);
            }}
          >
            {rows.map((p) =>
              isBlackKey(p) ? (
                <div key={p} className="roll-row-black" style={{ top: (127 - p) * ROW_HEIGHT, height: ROW_HEIGHT }} />
              ) : null,
            )}
            <div className="roll-clip-end" style={{ left: clip.length_beats * ppb }} />
            {clip.notes.map((n) => (
              <div
                key={n.id}
                className={`roll-note${props.selected.has(n.id) ? " selected" : ""}${
                  n.start_beats >= clip.length_beats ? " outside" : ""
                }`}
                style={{
                  left: n.start_beats * ppb,
                  top: (127 - n.pitch) * ROW_HEIGHT,
                  width: Math.max(3, n.length_beats * ppb - 1),
                  height: ROW_HEIGHT - 1,
                  opacity: 0.45 + (0.55 * n.velocity) / 127,
                }}
                title={`${drums ? (padNames.get(n.pitch) ?? noteName(n.pitch)) : noteName(n.pitch)} · velocity ${n.velocity}`}
                data-note-id={n.id}
                onPointerDown={(e) => startNoteDrag(e, n, false)}
                onDoubleClick={(e) => {
                  e.stopPropagation();
                  void props
                    .onCommand({ command: "remove_notes", clip_id: clip.id, note_ids: [n.id] })
                    .then(props.onEndGesture);
                }}
              >
                <div className="roll-note-resize" onPointerDown={(e) => startNoteDrag(e, n, true)} />
              </div>
            ))}
            {props.playheadBeats >= 0 && props.playheadBeats <= visibleBeats && (
              <div className="playhead" style={{ left: props.playheadBeats * ppb }} aria-hidden />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
