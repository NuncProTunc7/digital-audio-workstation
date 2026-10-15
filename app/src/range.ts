import { MAX_BASE_NOTE, MIN_BASE_NOTE, noteName } from "./keymap";
import type { Catalog, SamplePackStatus, Track } from "./types";

/** The notes an instrument can play, as MIDI notes. */
export interface NoteRange {
  low: number;
  high: number;
}

/** A guitar's range: its lowest open string to the last fret of the highest. */
export function guitarRange(track: Track, catalog: Catalog): NoteRange | null {
  const tunings = catalog.guitar_tunings ?? [];
  const tuning = tunings[track.instrument.params["string.tuning"] ?? 0] ?? tunings[0];
  if (!tuning || tuning.strings.length === 0) return null;
  const capo = track.instrument.params["string.capo"] ?? 0;
  // The capo shortens the neck; it still ends at the last fret.
  return {
    low: tuning.strings[0] + capo,
    high: tuning.strings[tuning.strings.length - 1] + (catalog.guitar_frets ?? 22),
  };
}

/** A loaded sample pack's range (C4 to C7 for a flute), once known. */
export function packRange(status: SamplePackStatus | null | undefined): NoteRange | null {
  if (status?.state !== "ready" || status.lowest_note === undefined || status.highest_note === undefined) {
    return null;
  }
  return { low: status.lowest_note, high: status.highest_note };
}

/** "C4 to C7". */
export function rangeText(range: NoteRange): string {
  return `${noteName(range.low)} to ${noteName(range.high)}`;
}

/** Whether the range leaves out part of the keyboard worth mentioning. */
export function isLimited(range: NoteRange): boolean {
  return range.low > 21 || range.high < 108;
}

/**
 * Where the typing keys should start for this range. Keeps `current` when
 * the octave it plays (current to current + 12) lies inside the range;
 * otherwise the first C at or above the lowest note (or the C below it,
 * for an instrument narrower than an octave).
 */
export function baseNoteForRange(range: NoteRange, current: number): number {
  if (current >= range.low && current + 12 <= range.high) return current;
  let base = Math.ceil(range.low / 12) * 12;
  if (base + 12 > range.high) base = Math.floor(range.low / 12) * 12;
  return Math.min(MAX_BASE_NOTE, Math.max(MIN_BASE_NOTE, base));
}
