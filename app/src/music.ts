// Keys, scales, and chords. Mirrors daw-model's `music` module.
import type { Chord, ChordQuality, Key, Mode } from "./types";

export const NOTE_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

export const MODES: { mode: Mode; name: string; intervals: number[]; hint: string }[] = [
  { mode: "major", name: "Major", intervals: [0, 2, 4, 5, 7, 9, 11], hint: "bright, happy" },
  { mode: "minor", name: "Minor", intervals: [0, 2, 3, 5, 7, 8, 10], hint: "sad, serious" },
  { mode: "dorian", name: "Dorian", intervals: [0, 2, 3, 5, 7, 9, 10], hint: "mysterious, adventurous" },
  { mode: "phrygian", name: "Phrygian", intervals: [0, 1, 3, 5, 7, 8, 10], hint: "dark, exotic" },
  { mode: "lydian", name: "Lydian", intervals: [0, 2, 4, 6, 7, 9, 11], hint: "dreamy, magical" },
  { mode: "mixolydian", name: "Mixolydian", intervals: [0, 2, 4, 5, 7, 9, 10], hint: "heroic, open" },
  { mode: "harmonic_minor", name: "Harmonic minor", intervals: [0, 2, 3, 5, 7, 8, 11], hint: "dramatic, villainous" },
  { mode: "major_pentatonic", name: "Major pentatonic", intervals: [0, 2, 4, 7, 9], hint: "simple, folk" },
  { mode: "minor_pentatonic", name: "Minor pentatonic", intervals: [0, 3, 5, 7, 10], hint: "bluesy, rock" },
];

export const CHORD_QUALITIES: { quality: ChordQuality; symbol: string; intervals: number[] }[] = [
  { quality: "major", symbol: "", intervals: [0, 4, 7] },
  { quality: "minor", symbol: "m", intervals: [0, 3, 7] },
  { quality: "diminished", symbol: "dim", intervals: [0, 3, 6] },
  { quality: "augmented", symbol: "aug", intervals: [0, 4, 8] },
  { quality: "sus2", symbol: "sus2", intervals: [0, 2, 7] },
  { quality: "sus4", symbol: "sus4", intervals: [0, 5, 7] },
  { quality: "major7", symbol: "maj7", intervals: [0, 4, 7, 11] },
  { quality: "minor7", symbol: "m7", intervals: [0, 3, 7, 10] },
  { quality: "dominant7", symbol: "7", intervals: [0, 4, 7, 10] },
  { quality: "half_diminished7", symbol: "m7b5", intervals: [0, 3, 6, 10] },
  { quality: "power", symbol: "5", intervals: [0, 7] },
];

const isMinor = (mode: Mode) =>
  mode === "minor" || mode === "dorian" || mode === "phrygian" || mode === "harmonic_minor" || mode === "minor_pentatonic";

/** Pitch classes of the key's scale. */
export function scaleOf(key: Key): Set<number> {
  const m = MODES.find((x) => x.mode === key.mode) ?? MODES[0];
  return new Set(m.intervals.map((i) => (key.tonic + i) % 12));
}

export function keyName(key: Key): string {
  return `${NOTE_NAMES[key.tonic % 12]} ${(MODES.find((m) => m.mode === key.mode)?.name ?? key.mode).toLowerCase()}`;
}

/** "Am7", "C/E". */
export function chordName(c: Pick<Chord, "root" | "quality" | "bass">): string {
  const q = CHORD_QUALITIES.find((x) => x.quality === c.quality);
  const bass = c.bass !== null && c.bass !== undefined ? `/${NOTE_NAMES[c.bass % 12]}` : "";
  return `${NOTE_NAMES[c.root % 12]}${q?.symbol ?? ""}${bass}`;
}

/** Pitch classes of a chord's tones. */
export function chordTones(c: Pick<Chord, "root" | "quality" | "bass">): Set<number> {
  const q = CHORD_QUALITIES.find((x) => x.quality === c.quality) ?? CHORD_QUALITIES[0];
  const tones = new Set(q.intervals.map((i) => (c.root + i) % 12));
  if (c.bass !== null && c.bass !== undefined) tones.add(c.bass % 12);
  return tones;
}

/** The chord a new chord starts as: the key's home chord, else C major. */
export function homeChord(key: Key | null | undefined): { root: number; quality: ChordQuality } {
  if (!key) return { root: 0, quality: "major" };
  return { root: key.tonic, quality: isMinor(key.mode) ? "minor" : "major" };
}
