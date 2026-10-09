import type { ParamSpec } from "./types";

/** Human-readable value for a parameter, such as "2.1 kHz" or "120 ms". */
export function formatParam(spec: ParamSpec, value: number): string {
  if (spec.choices.length > 0) return spec.choices[Math.round(value)] ?? String(value);
  switch (spec.unit) {
    case "hertz":
      return value >= 1000 ? `${(value / 1000).toFixed(1)} kHz` : `${value.toFixed(value < 10 ? 2 : 0)} Hz`;
    case "seconds":
      return value < 1 ? `${Math.round(value * 1000)} ms` : `${value.toFixed(2)} s`;
    case "decibels":
      return `${value > 0 ? "+" : ""}${value.toFixed(1)} dB`;
    case "semitones":
      return `${value > 0 ? "+" : ""}${Math.round(value)} st`;
    case "cents":
      return `${value > 0 ? "+" : ""}${Math.round(value)} ct`;
    case "percent":
      return `${Math.round(value * 100)}%`;
    case "ratio":
      return `${value.toFixed(1)}:1`;
    case "none":
      return `${value.toFixed(2)}×`;
  }
}

/** Snaps `beats` to the nearest multiple of `grid`. */
export function snap(beats: number, grid: number): number {
  return Math.round(beats / grid) * grid;
}

/** Snaps down to a multiple of `grid`. */
export function snapDown(beats: number, grid: number): number {
  return Math.floor(beats / grid + 1e-9) * grid;
}

/** "-6.0 dB", or "−∞" at the bottom of a fader. */
export function formatDb(db: number): string {
  return db <= -60 ? "−∞ dB" : `${db > 0 ? "+" : ""}${db.toFixed(1)} dB`;
}

/** Level meter width (0–100%) from a linear peak, over a 60 dB range. */
export function meterPercent(peak: number): number {
  const db = peak > 0 ? 20 * Math.log10(peak) : -100;
  return Math.max(0, Math.min(100, ((db + 60) / 60) * 100));
}

const SLIDER_STEPS = 1000;

/** Slider position (0–1000) for a value, honoring log scaling. */
export function toSlider(spec: ParamSpec, value: number): number {
  const t =
    spec.log_scale && spec.min > 0
      ? Math.log(value / spec.min) / Math.log(spec.max / spec.min)
      : (value - spec.min) / (spec.max - spec.min);
  return Math.round(Math.min(1, Math.max(0, t)) * SLIDER_STEPS);
}

/** Value for a slider position, snapped to whole steps for semitones. */
export function fromSlider(spec: ParamSpec, position: number): number {
  const t = position / SLIDER_STEPS;
  let value =
    spec.log_scale && spec.min > 0
      ? spec.min * Math.pow(spec.max / spec.min, t)
      : spec.min + t * (spec.max - spec.min);
  if (spec.unit === "semitones" || spec.unit === "cents") value = Math.round(value);
  return Math.min(spec.max, Math.max(spec.min, value));
}

export { SLIDER_STEPS };

/** A number with an optional unit after it: "2.5 kHz" → [2.5, "khz"]. */
function splitNumber(text: string): [number, string] | null {
  const m = /^([+-]?(?:\d+\.?\d*|\.\d+))\s*([a-z%:×]*1?)$/.exec(text.trim().toLowerCase().replace("−", "-"));
  if (!m) return null;
  const n = Number(m[1]);
  return Number.isFinite(n) ? [n, m[2]] : null;
}

/**
 * A typed parameter value, in the units its readout uses ("2.5k", "120 ms",
 * "35%"). A bare number is read in the unit the readout shows for `current`.
 * Null when the text isn't a number in a unit this parameter understands.
 */
export function parseParam(spec: ParamSpec, text: string, current: number): number | null {
  const parsed = splitNumber(text);
  if (!parsed) return null;
  const [n, unit] = parsed;
  const one = (...units: string[]) => (units.includes(unit) ? n : null);
  switch (spec.unit) {
    case "hertz":
      return unit === "k" || unit === "khz" ? n * 1000 : one("", "hz");
    case "seconds":
      if (unit === "ms") return n / 1000;
      if (unit === "s") return n;
      return unit === "" ? (current < 1 ? n / 1000 : n) : null;
    case "decibels":
      return one("", "db");
    case "semitones":
      return unit === "" || unit === "st" ? Math.round(n) : null;
    case "cents":
      return unit === "" || unit === "ct" ? Math.round(n) : null;
    case "percent":
      return unit === "" || unit === "%" ? n / 100 : null;
    case "ratio":
      return one("", ":1");
    case "none":
      return one("", "x", "×");
  }
}

/** A typed fader level in dB; "-inf" or "−∞" is the bottom (-60). */
export function parseDb(text: string): number | null {
  const t = text.trim().toLowerCase().replace(/\s*db$/, "");
  if (/^[-−]?(inf|∞)$/.test(t)) return -60;
  const parsed = splitNumber(t);
  return parsed && parsed[1] === "" ? parsed[0] : null;
}

/** A typed pan position (-1 left to 1 right): "C", "30L", "45R", or -20. */
export function parsePan(text: string): number | null {
  const t = text.trim().toLowerCase();
  if (t === "c" || t === "center" || t === "centre") return 0;
  const parsed = splitNumber(t);
  if (!parsed) return null;
  const [n, side] = parsed;
  if (side === "l") return -Math.abs(n) / 100;
  if (side === "r") return Math.abs(n) / 100;
  return side === "" ? n / 100 : null;
}

/** "Bar 3 · Beat 2" from a position in beats. */
export function formatPosition(beats: number, beatsPerBar: number): string {
  const whole = Math.floor(beats + 1e-9);
  const bar = Math.floor(whole / beatsPerBar) + 1;
  const beat = (whole % beatsPerBar) + 1;
  return `${bar}.${beat}`;
}
