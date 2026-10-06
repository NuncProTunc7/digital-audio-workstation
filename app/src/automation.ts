// What can be automated, and how a target's values map onto a lane.

import type { AutomationTarget, Catalog, ParamSpec, Track } from "./types";

export const AUTOMATION_HEIGHT = 64;
/** How a target's values map onto the lane: range, log or linear, name. */
export interface TargetScale {
  min: number;
  max: number;
  log: boolean;
  label: string;
  format: (v: number) => string;
}

function paramScale(spec: ParamSpec, prefix = ""): TargetScale {
  const log = spec.log_scale && spec.min > 0;
  const unit = spec.unit === "hertz" ? " Hz" : spec.unit === "decibels" ? " dB" : spec.unit === "seconds" ? " s" : "";
  return {
    min: spec.min,
    max: spec.max,
    log,
    label: `${prefix}${spec.name}`,
    format: (v) =>
      spec.unit === "percent" ? `${Math.round(v * 100)}%` : `${Math.abs(v) >= 100 ? Math.round(v) : v.toFixed(2)}${unit}`,
  };
}

/** The scale for a lane's target on `track`, or null if it no longer exists. */
export function targetScale(target: AutomationTarget, track: Track, catalog: Catalog): TargetScale | null {
  switch (target.kind) {
    case "volume":
      return { min: -60, max: 6, log: false, label: "Volume", format: (v) => `${v.toFixed(1)} dB` };
    case "pan":
      return {
        min: -1,
        max: 1,
        log: false,
        label: "Pan",
        format: (v) => (Math.abs(v) < 0.01 ? "C" : v < 0 ? `L${Math.round(-v * 100)}` : `R${Math.round(v * 100)}`),
      };
    case "instrument_param": {
      const spec = catalog.instruments
        .find((i) => i.kind === track.instrument.kind)
        ?.params.find((p) => p.id === target.param);
      return spec ? paramScale(spec) : null;
    }
    case "effect_param": {
      const fx = track.mixer.effects.find((e) => e.id === target.effect_id);
      const desc = catalog.effects.find((d) => d.kind === fx?.kind);
      const spec = desc?.params.find((p) => p.id === target.param);
      return spec && desc ? paramScale(spec, `${desc.name}: `) : null;
    }
  }
}

/** Every setting on `track` that can be automated, for the picker. */
export function automatableTargets(track: Track, catalog: Catalog): { target: AutomationTarget; label: string }[] {
  const out: { target: AutomationTarget; label: string }[] = [
    { target: { kind: "volume" }, label: "Volume" },
    { target: { kind: "pan" }, label: "Pan" },
  ];
  const inst = catalog.instruments.find((i) => i.kind === track.instrument.kind);
  for (const p of inst?.params ?? []) {
    if (p.choices.length === 0) out.push({ target: { kind: "instrument_param", param: p.id }, label: `${p.group}: ${p.name}` });
  }
  for (const e of track.mixer.effects) {
    const desc = catalog.effects.find((d) => d.kind === e.kind);
    for (const p of desc?.params ?? []) {
      if (p.choices.length === 0) {
        out.push({ target: { kind: "effect_param", effect_id: e.id, param: p.id }, label: `${desc?.name}: ${p.name}` });
      }
    }
  }
  return out;
}

