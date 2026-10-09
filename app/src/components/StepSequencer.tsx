import { useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import { noteName } from "../keymap";
import FixedMenu from "./FixedMenu";
import type { Clip, Command, DrumPad, Note, NoteInput } from "../types";

/** Rows from the most used drums down to the rarer ones. */
const ROW_ORDER = [36, 38, 39, 42, 46, 44, 37, 40, 45, 47, 48, 50, 41, 43, 49, 51];

const STEP_OPTIONS: { label: string; beats: number }[] = [
  { label: "1/8", beats: 0.5 },
  { label: "1/16", beats: 0.25 },
  { label: "1/32", beats: 0.125 },
];

/** Velocity levels a step can have. */
const STEP_VELOCITIES: { label: string; velocity: number }[] = [
  { label: "Soft", velocity: 60 },
  { label: "Normal", velocity: 100 },
  { label: "Accent", velocity: 127 },
];
const ROLLS = [1, 2, 3, 4];
const CHANCES = [100, 75, 50, 25];

interface StepSequencerProps {
  clip: Clip;
  drumPads: DrumPad[];
  beatsPerBar: number;
  playheadBeats: number;
  onCommand: (command: Command) => Promise<unknown>;
  onEndGesture: () => void;
  onAudition: (note: number) => void;
  /** Switches to the piano roll for free-form editing. */
  onShowPianoRoll: () => void;
}

/** A step: which row and which column. */
interface Cell {
  pitch: number;
  step: number;
}

const key = (c: Cell) => `${c.pitch}:${c.step}`;

type Paint = { mode: "add" | "erase"; cells: Map<string, Cell> };

/**
 * Drum grid: one row per drum, one column per step. Click or drag to add
 * or clear hits (a drag is one undo step); right-click a step for
 * velocity, rolls, and chance. Edits are ordinary note Commands, so the
 * piano roll, Claude, and undo all see the same notes.
 */
export default function StepSequencer(props: StepSequencerProps) {
  const { clip } = props;
  const [stepBeats, setStepBeats] = useState(0.25);
  const [paint, setPaint] = useState<Paint | null>(null);
  const [menu, setMenu] = useState<(Cell & { x: number; y: number }) | null>(null);
  const paintRef = useRef<Paint | null>(null);

  const steps = Math.max(1, Math.round(clip.length_beats / stepBeats));
  const stepsPerBeat = Math.max(1, Math.round(1 / stepBeats));
  const padNames = new Map(props.drumPads.map((p) => [p.note, p.name]));
  const used = [...new Set(clip.notes.map((n) => n.pitch))];
  const rows = [
    ...ROW_ORDER.filter((p) => padNames.has(p)),
    ...props.drumPads.map((p) => p.note).filter((p) => !ROW_ORDER.includes(p)),
    ...used.filter((p) => !padNames.has(p)).sort((a, b) => a - b),
  ];
  const rowName = (p: number) => padNames.get(p) ?? noteName(p);

  /** Notes that start inside a step, earliest first. */
  const notesIn = (c: Cell): Note[] =>
    clip.notes
      .filter(
        (n) =>
          n.pitch === c.pitch &&
          n.start_beats >= c.step * stepBeats - 1e-6 &&
          n.start_beats < (c.step + 1) * stepBeats - 1e-6,
      )
      .sort((a, b) => a.start_beats - b.start_beats);

  /** Notes for a step split into `roll` hits. */
  const hits = (c: Cell, roll: number, velocity: number, chance: number): NoteInput[] =>
    Array.from({ length: roll }, (_, i) => ({
      pitch: c.pitch,
      start_beats: c.step * stepBeats + (i * stepBeats) / roll,
      length_beats: stepBeats / roll,
      velocity,
      chance,
    }));

  const run = (commands: Command[]) => {
    if (commands.length === 0) return Promise.resolve();
    const command: Command = commands.length === 1 ? commands[0] : { command: "batch", commands };
    return props.onCommand(command).then(props.onEndGesture);
  };

  const cellAt = (e: ReactPointerEvent): Cell | null => {
    const el = (document.elementFromPoint?.(e.clientX, e.clientY) ?? e.target) as HTMLElement | null;
    const cell = el?.closest<HTMLElement>("[data-step]");
    if (!cell) return null;
    return { pitch: Number(cell.dataset.pitch), step: Number(cell.dataset.step) };
  };

  const startPaint = (e: ReactPointerEvent, c: Cell) => {
    if (e.button !== 0) return;
    e.preventDefault();
    setMenu(null);
    const p: Paint = { mode: notesIn(c).length > 0 ? "erase" : "add", cells: new Map([[key(c), c]]) };
    if (p.mode === "add") props.onAudition(c.pitch);
    paintRef.current = p;
    setPaint(p);
  };

  const movePaint = (e: ReactPointerEvent) => {
    const p = paintRef.current;
    if (!p) return;
    const c = cellAt(e);
    if (!c || p.cells.has(key(c))) return;
    const on = notesIn(c).length > 0;
    // Adding skips steps that already have a hit; erasing skips empty ones.
    if ((p.mode === "add") === on) return;
    const next: Paint = { mode: p.mode, cells: new Map(p.cells).set(key(c), c) };
    paintRef.current = next;
    setPaint(next);
  };

  const endPaint = () => {
    const p = paintRef.current;
    if (!p) return;
    paintRef.current = null;
    const cells = [...p.cells.values()];
    const done =
      p.mode === "add"
        ? run([
            {
              command: "add_notes",
              clip_id: clip.id,
              notes: cells.filter((c) => notesIn(c).length === 0).flatMap((c) => hits(c, 1, 100, 100)),
            },
          ])
        : run([
            {
              command: "remove_notes",
              clip_id: clip.id,
              note_ids: cells.flatMap((c) => notesIn(c).map((n) => n.id)),
            },
          ]);
    void done.finally(() => setPaint(null));
  };

  /** Rebuilds a step with new settings (creating it if it was empty). */
  const setStep = (c: Cell, change: { velocity?: number; roll?: number; chance?: number }) => {
    const old = notesIn(c);
    const velocity = change.velocity ?? old[0]?.velocity ?? 100;
    const chance = change.chance ?? old[0]?.chance ?? 100;
    const roll = change.roll ?? Math.max(1, old.length);
    const commands: Command[] = [];
    if (old.length > 0) commands.push({ command: "remove_notes", clip_id: clip.id, note_ids: old.map((n) => n.id) });
    commands.push({ command: "add_notes", clip_id: clip.id, notes: hits(c, roll, velocity, chance) });
    setMenu(null);
    void run(commands);
  };

  const swing = clip.swing ?? null;
  const setSwing = (amount: number, grid: number) =>
    void props.onCommand({
      command: "set_clip_swing",
      clip_id: clip.id,
      swing: amount > 0 ? { amount_percent: amount, grid_beats: grid } : null,
    });

  const menuNotes = menu ? notesIn(menu) : [];
  const playStep = Math.floor(props.playheadBeats / stepBeats);

  return (
    <div className="step-sequencer" onPointerMove={movePaint} onPointerUp={endPaint} onPointerCancel={endPaint}>
      <div className="roll-toolbar">
        <strong>{clip.name}</strong>
        <span className="muted">
          {clip.notes.length} hits · {clip.length_beats / props.beatsPerBar} bars
        </span>
        <label className="field">
          <span>Steps</span>
          <select
            aria-label="Step size"
            value={stepBeats}
            onChange={(e) => {
              setStepBeats(Number(e.target.value));
              e.currentTarget.blur();
            }}
          >
            {STEP_OPTIONS.map((o) => (
              <option key={o.label} value={o.beats}>
                {o.label}
              </option>
            ))}
          </select>
        </label>
        <label className="field" title="Swing: plays every second step late for a shuffled, bouncy groove">
          <span>Swing</span>
          <input
            type="range"
            min={0}
            max={100}
            step={1}
            aria-label="Swing amount"
            value={swing?.amount_percent ?? 0}
            onChange={(e) => setSwing(Number(e.target.value), swing?.grid_beats ?? 0.25)}
            onPointerUp={props.onEndGesture}
            onKeyUp={props.onEndGesture}
          />
          <span className="param-value">{Math.round(swing?.amount_percent ?? 0)}%</span>
          <select
            aria-label="Swing step"
            value={swing?.grid_beats ?? 0.25}
            disabled={!swing}
            onChange={(e) => {
              if (swing) {
                void props
                  .onCommand({
                    command: "set_clip_swing",
                    clip_id: clip.id,
                    swing: { amount_percent: swing.amount_percent, grid_beats: Number(e.target.value) },
                  })
                  .then(props.onEndGesture);
              }
              e.currentTarget.blur();
            }}
          >
            <option value={0.25}>1/16</option>
            <option value={0.5}>1/8</option>
          </select>
        </label>
        <span className="muted step-help">Click or drag to add hits · right-click a step for accent, rolls, chance</span>
        <span className="spacer" />
        <button className="small" onClick={props.onShowPianoRoll} title="Edit these notes freely in the piano roll">
          Piano roll
        </button>
      </div>

      <div className="step-scroll">
        <div className="step-grid" style={{ gridTemplateColumns: `110px repeat(${steps}, minmax(18px, 1fr))` }}>
          {rows.map((pitch) => (
            <div className="step-row" key={pitch} role="row" aria-label={rowName(pitch)}>
              <button className="step-name" onPointerDown={() => props.onAudition(pitch)} title="Hear it">
                {rowName(pitch)}
              </button>
              {Array.from({ length: steps }, (_, step) => {
                const c = { pitch, step };
                const notes = notesIn(c);
                const painted = paint?.cells.has(key(c)) ?? false;
                const on = painted ? paint?.mode === "add" : notes.length > 0;
                const first = notes[0];
                const classes = [
                  "step",
                  on ? "on" : "",
                  step % stepsPerBeat === 0 ? "beat" : "",
                  step % (stepsPerBeat * props.beatsPerBar) === 0 ? "bar" : "",
                  step === playStep ? "now" : "",
                  first && first.chance !== undefined && first.chance < 100 ? "maybe" : "",
                ]
                  .filter(Boolean)
                  .join(" ");
                return (
                  <div
                    key={step}
                    className={classes}
                    role="gridcell"
                    aria-label={`${rowName(pitch)} step ${step + 1}`}
                    aria-pressed={on}
                    data-step={step}
                    data-pitch={pitch}
                    style={on && first && !painted ? { opacity: 0.4 + (0.6 * first.velocity) / 127 } : undefined}
                    title={
                      first
                        ? `velocity ${first.velocity}${notes.length > 1 ? ` · roll ×${notes.length}` : ""}${
                            first.chance !== undefined && first.chance < 100 ? ` · plays ${first.chance}% of the time` : ""
                          }`
                        : undefined
                    }
                    onPointerDown={(e) => startPaint(e, c)}
                    onContextMenu={(e) => {
                      e.preventDefault();
                      setMenu({ ...c, x: e.clientX, y: e.clientY });
                    }}
                  >
                    {notes.length > 1 && <span className="step-roll">{"•".repeat(notes.length)}</span>}
                    {first && first.chance !== undefined && first.chance < 100 && (
                      <span className="step-chance">{first.chance}</span>
                    )}
                  </div>
                );
              })}
            </div>
          ))}
        </div>
      </div>

      {menu && (
        <FixedMenu
          className="step-menu"
          role="menu"
          aria-label={`${rowName(menu.pitch)} step ${menu.step + 1}`}
          x={menu.x}
          y={menu.y}
          onPointerLeave={() => setMenu(null)}
        >
          <div className="step-menu-row">
            <span>Velocity</span>
            {STEP_VELOCITIES.map((v) => (
              <button
                key={v.label}
                role="menuitem"
                className={menuNotes[0]?.velocity === v.velocity ? "small toggle on" : "small"}
                onClick={() => setStep(menu, { velocity: v.velocity })}
              >
                {v.label}
              </button>
            ))}
          </div>
          <div className="step-menu-row">
            <span>Roll</span>
            {ROLLS.map((r) => (
              <button
                key={r}
                role="menuitem"
                aria-label={`Roll ${r}`}
                className={Math.max(1, menuNotes.length) === r && menuNotes.length > 0 ? "small toggle on" : "small"}
                onClick={() => setStep(menu, { roll: r })}
              >
                ×{r}
              </button>
            ))}
          </div>
          <div className="step-menu-row">
            <span>Chance</span>
            {CHANCES.map((ch) => (
              <button
                key={ch}
                role="menuitem"
                aria-label={`Chance ${ch}%`}
                className={(menuNotes[0]?.chance ?? 100) === ch && menuNotes.length > 0 ? "small toggle on" : "small"}
                onClick={() => setStep(menu, { chance: ch })}
              >
                {ch}%
              </button>
            ))}
          </div>
          {menuNotes.length > 0 && (
            <button
              role="menuitem"
              className="small"
              onClick={() => {
                setMenu(null);
                void run([{ command: "remove_notes", clip_id: clip.id, note_ids: menuNotes.map((n) => n.id) }]);
              }}
            >
              Clear step
            </button>
          )}
        </FixedMenu>
      )}
    </div>
  );
}
