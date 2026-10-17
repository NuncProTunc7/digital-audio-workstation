import { useEffect, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import { snap, snapDown } from "../format";
import { isBlackKey, noteName } from "../keymap";
import type { Chord, Clip, Command, DrumPad, GuitarNote, Key, Note, Track } from "../types";
import { chordTones, keyName, scaleOf } from "../music";

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
  /** For drum clips: back to the step grid. */
  onShowSteps?: () => void;
  /** The song's key and chord track, to highlight scale and chord notes. */
  songKey?: Key | null;
  chords?: Chord[];
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
  // What the last Quantize did, so a click that changes nothing doesn't
  // look broken.
  const [quantized, setQuantized] = useState<{ clipId: number; text: string } | null>(null);
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
  const scale = props.songKey && !drums ? scaleOf(props.songKey) : null;
  // Chord-tone bands for each chord that overlaps the visible part of the clip.
  const chordBands = drums
    ? []
    : (props.chords ?? []).flatMap((c, i, all) => {
        const from = Math.max(0, c.start_beats - clip.start_beats);
        const nextStart = all[i + 1]?.start_beats ?? Number.POSITIVE_INFINITY;
        const to = Math.min(visibleBeats, nextStart - clip.start_beats);
        if (to <= from) return [];
        const tones = chordTones(c);
        return rows
          .filter((p) => tones.has(p % 12))
          .map((p) => ({ key: `${c.id}-${p}`, from, to, pitch: p }));
      });

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
          onClick={() => {
            const notes = targetIds ? clip.notes.filter((n) => targetIds.includes(n.id)) : clip.notes;
            const moved = notes.filter((n) => Math.abs(n.start_beats - Math.round(n.start_beats / grid) * grid) > 1e-6).length;
            const label = GRID_OPTIONS.find((g) => g.beats === grid)?.label ?? `${grid} beat`;
            setQuantized({
              clipId: clip.id,
              text:
                moved === 0
                  ? `Already on the ${label} grid`
                  : `${moved} note${moved === 1 ? "" : "s"} moved to the ${label} grid`,
            });
            void props
              .onCommand({
                command: "quantize_notes",
                clip_id: clip.id,
                grid_beats: grid,
                strength: null,
                lengths: false,
                note_ids: targetIds,
              })
              .then(props.onEndGesture);
          }}
          title="Snap note starts to the grid (selected notes, or all)"
        >
          Quantize
        </button>
        {quantized?.clipId === clip.id && (
          <span className="muted quantize-result" role="status" aria-label="Quantize result">
            {quantized.text}
          </span>
        )}
        <button
          onClick={() =>
            void props
              .onCommand({
                command: "humanize_notes",
                clip_id: clip.id,
                timing_beats: Math.min(0.03, grid / 8),
                velocity: 10,
                seed: Math.floor(Math.random() * 1_000_000),
                note_ids: targetIds,
              })
              .then(props.onEndGesture)
          }
          title="Nudge timing and loudness slightly (selected notes, or all), so a programmed part sounds played. Click again for a different take; Ctrl+Z undoes."
        >
          Humanize
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
        {track.instrument.kind === "guitar" && (
          <GuitarControls
            notes={selectedNotes}
            onChange={(change) =>
              void props
                .onCommand({
                  command: "edit_notes",
                  clip_id: clip.id,
                  edits: selectedNotes.map((n) => ({ id: n.id, guitar: change(n.guitar ?? {}) })),
                })
                .then(props.onEndGesture)
            }
          />
        )}
        <div className="button-group" role="group" aria-label="Copy clip">
          {[false, true].map((linked) => (
            <button
              key={String(linked)}
              className="small"
              onClick={() =>
                void props
                  .onCommand({ command: "duplicate_clip", clip_id: clip.id, start_beats: null, linked })
                  .then(props.onEndGesture)
              }
              title={
                linked
                  ? "Put a linked copy right after this clip: editing either one changes both (Ctrl+Shift+D)"
                  : "Put an independent copy right after this clip (Ctrl+D)"
              }
            >
              {linked ? "Linked copy" : "Duplicate"}
            </button>
          ))}
        </div>
        {clip.link && (
          <span className="linked-note" title="Editing these notes changes every linked copy too">
            🔗 Linked
            <button
              className="small"
              onClick={() =>
                void props.onCommand({ command: "unlink_clip", clip_id: clip.id }).then(props.onEndGesture)
              }
              title="Make this clip independent: later edits only change it"
            >
              Unlink
            </button>
          </span>
        )}
        {scale && props.songKey && (
          <span className="muted" title="Rows in the key are lighter; notes of the chord playing at that moment are marked">
            In {keyName(props.songKey)}
          </span>
        )}
        <span className="spacer" />
        {props.onShowSteps && (
          <button className="small" onClick={props.onShowSteps} title="Edit this drum clip as a grid of steps">
            Steps
          </button>
        )}
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
            {scale &&
              rows.map((p) =>
                scale.has(p % 12) ? (
                  <div
                    key={`s${p}`}
                    className={p % 12 === props.songKey?.tonic ? "roll-row-scale tonic" : "roll-row-scale"}
                    style={{ top: (127 - p) * ROW_HEIGHT, height: ROW_HEIGHT }}
                  />
                ) : null,
              )}
            {chordBands.map((b) => (
              <div
                key={b.key}
                className="roll-chord-tone"
                style={{ left: b.from * ppb, width: (b.to - b.from) * ppb, top: (127 - b.pitch) * ROW_HEIGHT, height: ROW_HEIGHT }}
              />
            ))}
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
                {n.guitar && techniqueMark(n.guitar) && <span className="roll-note-mark">{techniqueMark(n.guitar)}</span>}
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

/** A guitar note's technique as a short key ("picked", "slide", "legato", "bend_1"). */
function techniqueKey(g: GuitarNote | undefined): string {
  if (g?.slide) return "slide";
  if (g?.legato) return "legato";
  if (g?.bend_semitones) return `bend_${g.bend_semitones}`;
  return "picked";
}

/** The mark drawn on a note: / slide, H hammer-on/pull-off, ↑ bend. */
function techniqueMark(g: GuitarNote): string {
  if (g.slide) return "/";
  if (g.legato) return "H";
  if (g.bend_semitones) return `↑${g.bend_semitones}`;
  return "";
}

/** The guitar detail without empty fields. */
function tidy(g: GuitarNote): GuitarNote {
  const out: GuitarNote = {};
  if (g.bend_semitones) out.bend_semitones = g.bend_semitones;
  if (g.legato) out.legato = true;
  if (g.slide) out.slide = true;
  if (g.string) out.string = g.string;
  return out;
}

const TECHNIQUES: [string, string][] = [
  ["picked", "Picked"],
  ["slide", "Slide in (/)"],
  ["legato", "Hammer-on / pull-off (H)"],
  ["bend_0.5", "Bend ½ step"],
  ["bend_1", "Bend 1 step (half)"],
  ["bend_2", "Bend 2 (full)"],
  ["bend_3", "Bend 3 (overbend)"],
];

/** String and technique of the selected notes on a guitar track. */
function GuitarControls({
  notes,
  onChange,
}: {
  notes: Note[];
  onChange: (change: (g: GuitarNote) => GuitarNote) => void;
}) {
  const common = <T,>(f: (n: Note) => T): T | "" => {
    const values = new Set(notes.map(f));
    return values.size === 1 ? [...values][0] : "";
  };
  const string = common((n) => (n.guitar?.string ? String(n.guitar.string) : "auto"));
  const technique = common((n) => techniqueKey(n.guitar));
  const disabled = notes.length === 0;
  return (
    <div className="button-group guitar-controls" role="group" aria-label="Guitar">
      <label className="field" title="Which string plays the selected notes (Auto: the highest string that reaches them)">
        <span>String</span>
        <select
          aria-label="String"
          disabled={disabled}
          value={string}
          onChange={(e) => {
            const v = e.target.value;
            e.currentTarget.blur();
            onChange((g) => tidy({ ...g, string: v === "auto" ? undefined : Number(v) }));
          }}
        >
          {string === "" && <option value="">Mixed</option>}
          <option value="auto">Auto</option>
          {[1, 2, 3, 4, 5, 6].map((s) => (
            <option key={s} value={s}>
              {s === 1 ? "1 (thinnest)" : s === 6 ? "6 (thickest)" : String(s)}
            </option>
          ))}
        </select>
      </label>
      <label className="field" title="How the selected notes are played (slides and hammer-ons join the note before on the same string)">
        <span>Technique</span>
        <select
          aria-label="Technique"
          disabled={disabled}
          value={technique}
          onChange={(e) => {
            const v = e.target.value;
            e.currentTarget.blur();
            onChange((g) =>
              tidy({
                string: g.string,
                slide: v === "slide",
                legato: v === "legato",
                bend_semitones: v.startsWith("bend_") ? Number(v.slice(5)) : undefined,
              }),
            );
          }}
        >
          {technique === "" && <option value="">Mixed</option>}
          {TECHNIQUES.map(([v, label]) => (
            <option key={v} value={v}>
              {label}
            </option>
          ))}
        </select>
      </label>
    </div>
  );
}
