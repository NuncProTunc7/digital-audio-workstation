// Sound effect recipes on the UI side: the default recipe, field ranges,
// and rough stand-ins for the Rust generators (used only by the browser
// preview backend; the real generators are in daw-model/src/sound.rs).

import type { SoundCategory, SoundRecipe } from "./types";

/** A short square blip (matches `SoundRecipe::default()` in Rust). */
export const DEFAULT_RECIPE: SoundRecipe = {
  wave: "square",
  attack: 0,
  sustain: 0.1,
  punch: 0,
  decay: 0.2,
  base_freq: 0.5,
  freq_limit: 0,
  freq_ramp: 0,
  freq_dramp: 0,
  vib_strength: 0,
  vib_speed: 0,
  arp_mod: 0,
  arp_speed: 0,
  duty: 0,
  duty_ramp: 0,
  repeat_speed: 0,
  phaser_offset: 0,
  phaser_ramp: 0,
  lpf_freq: 1,
  lpf_ramp: 0,
  lpf_resonance: 0,
  hpf_freq: 0,
  hpf_ramp: 0,
  crush: 0,
  volume_db: 0,
};

export type RecipeField = Exclude<keyof SoundRecipe, "wave">;

/** Every number field: range and a plain label and hint. */
export const FIELDS: { key: RecipeField; label: string; min: number; max: number; hint: string }[] = [
  { key: "attack", label: "Attack", min: 0, max: 1, hint: "Fade-in time (0 = instant)" },
  { key: "sustain", label: "Hold", min: 0, max: 1, hint: "How long it stays at full volume" },
  { key: "punch", label: "Punch", min: 0, max: 1, hint: "Extra loudness at the start, for a ping" },
  { key: "decay", label: "Fade out", min: 0, max: 1, hint: "Fade-out time" },
  { key: "base_freq", label: "Pitch", min: 0, max: 1, hint: "Starting pitch" },
  { key: "freq_limit", label: "Lowest pitch", min: 0, max: 1, hint: "The sound stops if it slides below this (0 = no limit)" },
  { key: "freq_ramp", label: "Slide", min: -1, max: 1, hint: "Pitch slide: down for lasers, up for jumps" },
  { key: "freq_dramp", label: "Slide curve", min: -1, max: 1, hint: "Bends the slide as it goes" },
  { key: "vib_strength", label: "Vibrato", min: 0, max: 1, hint: "Pitch wobble depth" },
  { key: "vib_speed", label: "Vibrato speed", min: 0, max: 1, hint: "Pitch wobble speed" },
  { key: "arp_mod", label: "Pitch jump", min: -1, max: 1, hint: "Jump up (coin ding-ding) or down partway through" },
  { key: "arp_speed", label: "Jump timing", min: 0, max: 1, hint: "How soon the pitch jump happens" },
  { key: "duty", label: "Square width", min: 0, max: 1, hint: "Square wave only: hollow (0) to thin and nasal (1)" },
  { key: "duty_ramp", label: "Width sweep", min: -1, max: 1, hint: "Square wave only: changes the width over time" },
  { key: "repeat_speed", label: "Repeat", min: 0, max: 1, hint: "Restarts the slide and jump this often (0 = off)" },
  { key: "phaser_offset", label: "Phaser", min: -1, max: 1, hint: "A swirling, jet-like colour (0 = off)" },
  { key: "phaser_ramp", label: "Phaser sweep", min: -1, max: 1, hint: "Moves the phaser over time" },
  { key: "lpf_freq", label: "Tone", min: 0, max: 1, hint: "Low-pass filter: lower is duller (1 = off)" },
  { key: "lpf_ramp", label: "Tone sweep", min: -1, max: 1, hint: "Opens or closes the tone over time" },
  { key: "lpf_resonance", label: "Resonance", min: 0, max: 1, hint: "A whistling peak at the tone setting" },
  { key: "hpf_freq", label: "Thin", min: 0, max: 1, hint: "High-pass filter: higher is thinner (0 = off)" },
  { key: "hpf_ramp", label: "Thin sweep", min: -1, max: 1, hint: "Moves the high-pass over time" },
  { key: "crush", label: "Crunch", min: 0, max: 1, hint: "Bit crush: lo-fi grit (0 = off)" },
  { key: "volume_db", label: "Volume", min: -24, max: 6, hint: "Overall level in dB" },
];

/** Keeps every field in range, like `SoundRecipe::clamped`. */
export function clampRecipe(r: SoundRecipe): SoundRecipe {
  const out = { ...r };
  for (const f of FIELDS) out[f.key] = Math.min(f.max, Math.max(f.min, r[f.key]));
  return out;
}

/** Rough per-category starting points for the preview backend. */
const PREVIEW_RECIPES: Record<SoundCategory, Partial<SoundRecipe>> = {
  coin: { base_freq: 0.6, sustain: 0.05, decay: 0.3, punch: 0.45, arp_speed: 0.6, arp_mod: 0.4 },
  jump: { base_freq: 0.4, freq_ramp: 0.2, sustain: 0.25, decay: 0.15, duty: 0.3 },
  laser: { wave: "saw", base_freq: 0.75, freq_limit: 0.2, freq_ramp: -0.25, sustain: 0.2, decay: 0.2 },
  explosion: { wave: "noise", base_freq: 0.1, freq_ramp: -0.1, sustain: 0.25, decay: 0.4, punch: 0.5 },
  hit: { wave: "noise", base_freq: 0.5, freq_ramp: -0.5, sustain: 0.05, decay: 0.2 },
  power_up: { base_freq: 0.35, freq_ramp: 0.3, repeat_speed: 0.6, sustain: 0.2, decay: 0.3 },
  blip: { base_freq: 0.4, sustain: 0.15, decay: 0.1, hpf_freq: 0.1 },
  random: { wave: "triangle", base_freq: 0.5, freq_ramp: 0.1, sustain: 0.3, decay: 0.3 },
};

/** Preview stand-in for `generate`: the category's start, moved a little by the seed. */
export function previewGenerate(category: SoundCategory, seed: number): SoundRecipe {
  const r = { ...DEFAULT_RECIPE, sustain: 0.3, decay: 0.4, base_freq: 0.3, ...PREVIEW_RECIPES[category] };
  r.base_freq += ((seed % 11) - 5) * 0.01;
  return clampRecipe(r);
}

/** Preview stand-in for `vary`. */
export function previewVary(r: SoundRecipe, seed: number): SoundRecipe {
  return clampRecipe({ ...r, base_freq: r.base_freq + ((seed % 11) - 5) * 0.005, decay: r.decay + ((seed % 7) - 3) * 0.005 });
}

/** Length in seconds, like `SoundRecipe::seconds`. */
export function recipeSeconds(r: SoundRecipe): number {
  return Math.min(10, ((r.attack ** 2 + r.sustain ** 2 + r.decay ** 2) * 100000) / 44100);
}
