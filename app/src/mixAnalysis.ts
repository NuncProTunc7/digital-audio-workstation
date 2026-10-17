// The Mix analysis tab's judgements: game-music targets as green / amber /
// red, plain names for the frequency bands, bar positions under the
// spectrogram, and a stand-in analysis for the browser preview.

import type { MixAnalysis, Project } from "./types";
import { tempoMap } from "./tempo";

export type Verdict = "good" | "check" | "bad";

/** Integrated loudness against game music's -20 to -16 LUFS. */
export function loudnessVerdict(lufs: number | null): Verdict {
  if (lufs === null) return "bad";
  if (lufs >= -20 && lufs <= -16) return "good";
  if (lufs >= -23 && lufs <= -13) return "check";
  return "bad";
}

/** True peak: at most -1 dBTP leaves room so exports don't distort. */
export function peakVerdict(dbtp: number): Verdict {
  if (dbtp <= -1) return "good";
  if (dbtp <= 0) return "check";
  return "bad";
}

/** Stereo correlation: near +1 is almost mono, below 0 cancels on phones. */
export function stereoVerdict(correlation: number): Verdict {
  if (correlation < 0) return "bad";
  if (correlation < 0.2 || correlation > 0.95) return "check";
  return "good";
}

/** One line under each verdict, in plain words. */
export function loudnessNote(lufs: number | null): string {
  if (lufs === null) return "Silent";
  if (lufs > -16) return "Louder than most game music (aim for -20 to -16)";
  if (lufs < -20) return "Quieter than most game music (aim for -20 to -16)";
  return "Right for game music";
}

export function peakNote(dbtp: number): string {
  if (dbtp > 0) return "Over 0 dB: will distort when exported";
  if (dbtp > -1) return "Very close to 0 dB: may distort when exported";
  return "Room to spare";
}

export function stereoNote(correlation: number): string {
  if (correlation < 0) return "Parts may cancel out on phones";
  if (correlation > 0.95) return "Almost mono: pan or add reverb to widen";
  if (correlation < 0.2) return "Very wide";
  return "Nicely wide";
}

const BAND_LABELS: Record<string, string> = {
  sub: "Sub",
  bass: "Bass",
  low_mids: "Low mids",
  mids: "Mids",
  high_mids: "High mids",
  highs: "Highs",
};

/** A band's plain name, e.g. "low_mids" → "Low mids". */
export function bandLabel(name: string): string {
  return BAND_LABELS[name] ?? name;
}

/** "60–250 Hz", "2–6 kHz". */
export function bandRange(fromHz: number, toHz: number): string {
  if (toHz < 1000) return `${fromHz}–${toHz} Hz`;
  if (fromHz < 1000) return `${fromHz} Hz–${toHz / 1000} kHz`;
  return `${fromHz / 1000}–${toHz / 1000} kHz`;
}

/**
 * Where each bar starts across the spectrogram, as a 0–1 fraction of its
 * width (it covers `mix.duration_s`, which can include a reverb tail).
 * Thinned to at most `maxLabels` so the numbers don't crowd.
 */
export function barTicks(
  project: Pick<Project, "tempo_bpm" | "tempo_changes" | "time_signature">,
  analysis: Pick<MixAnalysis, "start_beats" | "end_beats" | "mix">,
  maxLabels = 16,
): { bar: number; at: number }[] {
  const duration = analysis.mix.duration_s;
  if (!(duration > 0)) return [];
  const map = tempoMap(project);
  const perBar = Math.max(1, project.time_signature.numerator);
  const firstBar = Math.ceil(analysis.start_beats / perBar - 1e-9);
  const lastBar = Math.floor(analysis.end_beats / perBar + 1e-9);
  const count = lastBar - firstBar + 1;
  const step = Math.max(1, Math.ceil(count / maxLabels));
  const ticks: { bar: number; at: number }[] = [];
  for (let b = firstBar; b <= lastBar; b += step) {
    const at = map.secondsBetween(analysis.start_beats, b * perBar) / duration;
    if (at >= 0 && at <= 1) ticks.push({ bar: b + 1, at });
  }
  return ticks;
}

/** A believable analysis for the browser preview (no audio there). */
export function previewAnalysis(project: Project, startBeats: number | null, endBeats: number | null): MixAnalysis {
  const songEnd = Math.max(
    project.time_signature.numerator,
    ...project.tracks.flatMap((t) => t.clips.map((c) => c.start_beats + c.length_beats)),
  );
  const start = startBeats ?? 0;
  const end = endBeats ?? songEnd;
  const playing = project.tracks.filter((t) => !t.mixer.mute && t.clips.length > 0);
  const weights = project.tracks.map((t) => (playing.includes(t) ? 1 + t.clips.length : 0));
  const total = weights.reduce((a, b) => a + b, 0);
  const silent = total === 0;
  const tail = endBeats === null ? 1.5 : 0;
  return {
    start_beats: start,
    end_beats: end,
    mix: {
      duration_s: tempoMap(project).secondsBetween(start, end) + tail,
      integrated_lufs: silent ? null : -17.5,
      max_short_term_lufs: silent ? null : -14.2,
      true_peak_dbtp: silent ? -120 : -2.4,
      sample_peak_dbfs: silent ? -120 : -2.6,
      rms_dbfs: silent ? -120 : -20.1,
      peak_to_loudness_db: silent ? null : 15.1,
      stereo_correlation: silent ? 1 : 0.62,
      bands: [
        { name: "sub", from_hz: 20, to_hz: 60, share_percent: 18 },
        { name: "bass", from_hz: 60, to_hz: 250, share_percent: 41 },
        { name: "low_mids", from_hz: 250, to_hz: 500, share_percent: 14 },
        { name: "mids", from_hz: 500, to_hz: 2000, share_percent: 17 },
        { name: "high_mids", from_hz: 2000, to_hz: 6000, share_percent: 7 },
        { name: "highs", from_hz: 6000, to_hz: 20000, share_percent: 3 },
      ],
    },
    tracks:
      project.tracks.length > 1
        ? project.tracks.map((t, i) => ({
            id: t.id,
            name: t.name,
            lufs: weights[i] > 0 ? -22 + weights[i] : null,
            peak_dbfs: weights[i] > 0 ? -6 : -120,
            energy_share_percent: total > 0 ? (100 * weights[i]) / total : 0,
          }))
        : [],
    hints: silent
      ? ["The mix is silent in this range. Check that clips have notes and tracks aren't muted."]
      : ["Levels look healthy: -17.5 LUFS with peaks at -2.4 dBTP."],
    spectrogram_summary: silent ? [] : ["(Browser preview: no real audio, so no spectrogram.)"],
  };
}
