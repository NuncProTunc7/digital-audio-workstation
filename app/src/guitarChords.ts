// Guitar chord shapes for any tuning: which fret each string plays so a
// chord sounds with its root (or slash note) lowest, the way a guitarist
// would hold it. Open shapes win where they exist; otherwise a shape higher
// up the neck (often a barre).

import { CHORD_QUALITIES } from "./music";
import type { ChordQuality } from "./types";

export interface ChordShape {
  /** Fret per string, lowest string first (null = not played). Counted from the capo. */
  frets: (number | null)[];
  /** The notes that sound, lowest first. */
  notes: number[];
}

/** How many frets a hand covers. */
const SPAN = 3;

/**
 * The best shape for a chord on a guitar whose open strings (with the capo)
 * are `open`, lowest first, reaching `reach` frets.
 */
export function chordShape(
  chord: { root: number; quality: ChordQuality; bass?: number | null },
  open: number[],
  reach: number,
): ChordShape {
  const intervals = CHORD_QUALITIES.find((q) => q.quality === chord.quality)?.intervals ?? [0, 4, 7];
  const tones = new Set(intervals.map((i) => (chord.root + i) % 12));
  const bass = (chord.bass ?? chord.root) % 12;
  if (chord.bass !== undefined && chord.bass !== null) tones.add(bass);
  // Four-note chords may leave out the fifth, as guitarists do.
  const optional = intervals.length > 3 ? new Set([(chord.root + 7) % 12]) : new Set<number>();
  const minStrings = intervals.length === 2 ? 2 : 3;

  let best: { frets: (number | null)[]; score: number } | null = null;
  for (let start = 0; start + SPAN <= reach; start++) {
    // Each string: not played, open, or a fret within the hand's reach.
    const options = open.map((o) => {
      const opts: (number | null)[] = [null];
      if (tones.has(o % 12)) opts.push(0);
      for (let f = Math.max(1, start); f <= start + SPAN; f++) if (tones.has((o + f) % 12)) opts.push(f);
      return opts;
    });
    const frets: (number | null)[] = new Array(open.length).fill(null);
    const visit = (s: number) => {
      if (s === open.length) {
        const score = scoreShape(frets, open, tones, optional, bass, minStrings, start);
        if (score !== null && (best === null || score > best.score)) best = { frets: [...frets], score };
        return;
      }
      for (const f of options[s]) {
        // Once a string sounds, every higher one must too (no gaps).
        if (f === null && s > 0 && frets[s - 1] !== null) continue;
        frets[s] = f;
        visit(s + 1);
      }
    };
    visit(0);
  }
  const frets: (number | null)[] = (best as { frets: (number | null)[] } | null)?.frets ?? new Array(open.length).fill(null);
  return {
    frets,
    notes: frets.flatMap((f, i) => (f === null ? [] : [open[i] + f])),
  };
}

/** A shape's score (higher is better), or null if it doesn't work. */
function scoreShape(
  frets: (number | null)[],
  open: number[],
  tones: Set<number>,
  optional: Set<number>,
  bass: number,
  minStrings: number,
  start: number,
): number | null {
  const sounding = frets.flatMap((f, i) => (f === null ? [] : [{ f, note: open[i] + f }]));
  if (sounding.length < minStrings) return null;
  if (sounding[0].note % 12 !== bass) return null;
  const heard = new Set(sounding.map((s) => s.note % 12));
  for (const t of tones) if (!heard.has(t) && !optional.has(t)) return null;
  const fretted = sounding.filter((s) => s.f > 0).map((s) => s.f);
  const lowest = Math.min(...fretted);
  // Up the neck, one finger can bar the lowest fret across the strings.
  const fingers = start > 0 && fretted.length > 0 ? fretted.filter((f) => f > lowest).length + 1 : fretted.length;
  if (fingers > 4) return null;
  const opens = sounding.length - fretted.length;
  const highest = fretted.length > 0 ? Math.max(...fretted) : 0;
  return sounding.length * 10 + opens * 3 - highest - start * 4;
}

/** "x32010": the shape as guitarists write it, lowest string first. */
export function shapeText(shape: ChordShape): string {
  return shape.frets.map((f) => (f === null ? "x" : f > 9 ? `(${f})` : String(f))).join("");
}
