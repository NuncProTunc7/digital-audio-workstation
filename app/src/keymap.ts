// Computer-keyboard-as-piano ("musical typing"), like GarageBand and Ableton.
// Uses physical key positions (KeyboardEvent.code), so it works on any
// keyboard layout.

/** Semitone offset from the base note for each key. */
const NOTE_KEYS: Record<string, number> = {
  KeyA: 0,
  KeyW: 1,
  KeyS: 2,
  KeyE: 3,
  KeyD: 4,
  KeyF: 5,
  KeyT: 6,
  KeyG: 7,
  KeyY: 8,
  KeyH: 9,
  KeyU: 10,
  KeyJ: 11,
  KeyK: 12,
  KeyO: 13,
  KeyL: 14,
  KeyP: 15,
  Semicolon: 16,
  Quote: 17,
};

/** Key labels shown on the on-screen piano and pads. */
export const KEY_LABELS: Record<string, string> = {
  KeyA: "A",
  KeyW: "W",
  KeyS: "S",
  KeyE: "E",
  KeyD: "D",
  KeyF: "F",
  KeyT: "T",
  KeyG: "G",
  KeyY: "Y",
  KeyH: "H",
  KeyU: "U",
  KeyJ: "J",
  KeyK: "K",
  KeyO: "O",
  KeyL: "L",
  KeyP: "P",
  Semicolon: ";",
  Quote: "'",
};

export const TYPING_SPAN = 18;
export const DEFAULT_BASE_NOTE = 60; // C4
export const DRUM_BASE_NOTE = 36; // Kick on A, snare on S, closed hat on T
export const MIN_BASE_NOTE = 24;
export const MAX_BASE_NOTE = 96;
export const VELOCITY_STEPS = [20, 40, 60, 80, 100, 127];
export const DEFAULT_VELOCITY = 100;

export function noteForCode(code: string, baseNote: number): number | null {
  const offset = NOTE_KEYS[code];
  if (offset === undefined) return null;
  const note = baseNote + offset;
  return note >= 0 && note <= 127 ? note : null;
}

/** The label of the key that plays `note`, if it is in typing range. */
export function labelForNote(note: number, baseNote: number): string | null {
  const offset = note - baseNote;
  for (const [code, o] of Object.entries(NOTE_KEYS)) {
    if (o === offset) return KEY_LABELS[code] ?? null;
  }
  return null;
}

export function shiftOctave(baseNote: number, direction: -1 | 1): number {
  return Math.min(MAX_BASE_NOTE, Math.max(MIN_BASE_NOTE, baseNote + 12 * direction));
}

export function stepVelocity(velocity: number, direction: -1 | 1): number {
  const index = VELOCITY_STEPS.findIndex((v) => v >= velocity);
  const current = index === -1 ? VELOCITY_STEPS.length - 1 : index;
  const next = Math.min(VELOCITY_STEPS.length - 1, Math.max(0, current + direction));
  return VELOCITY_STEPS[next];
}

const NOTE_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/** "C4" for 60. */
export function noteName(note: number): string {
  return `${NOTE_NAMES[note % 12]}${Math.floor(note / 12) - 1}`;
}

export function isBlackKey(note: number): boolean {
  return [1, 3, 6, 8, 10].includes(note % 12);
}
