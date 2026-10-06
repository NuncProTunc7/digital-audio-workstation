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

/** "Bar 3 · Beat 2" from a position in beats. */
export function formatPosition(beats: number, beatsPerBar: number): string {
  const whole = Math.floor(beats + 1e-9);
  const bar = Math.floor(whole / beatsPerBar) + 1;
  const beat = (whole % beatsPerBar) + 1;
  return `${bar}.${beat}`;
}
