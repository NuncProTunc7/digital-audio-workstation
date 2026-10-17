// Beats ↔ seconds through the song's tempo changes. Mirrors
// `daw_model::TempoMap` (crates/daw-model/src/tempo.rs): a change takes over
// on its beat; with `ramp` the tempo glides (linearly in BPM over beats) to
// the next change's tempo.

import type { Project, TempoChange } from "./types";

interface Segment {
  beats: number;
  seconds: number;
  bpm: number;
  /** BPM gained per beat (0 = steady). */
  slope: number;
}

function segmentSeconds(s: Segment, beats: number): number {
  const db = beats - s.beats;
  if (s.slope === 0) return s.seconds + (db * 60) / s.bpm;
  return s.seconds + (60 / s.slope) * Math.log((s.bpm + s.slope * db) / s.bpm);
}

function segmentBeats(s: Segment, seconds: number): number {
  const dt = seconds - s.seconds;
  if (s.slope === 0) return s.beats + (dt * s.bpm) / 60;
  return s.beats + (s.bpm * (Math.exp((s.slope * dt) / 60) - 1)) / s.slope;
}

const saneBpm = (bpm: number) => (Number.isFinite(bpm) ? Math.min(999, Math.max(20, bpm)) : 120);

export class TempoMap {
  private readonly segments: Segment[];

  constructor(startBpm: number, changes: readonly TempoChange[] = []) {
    const sorted = changes
      .filter((c) => Number.isFinite(c.beats) && c.beats > 0)
      .sort((a, b) => a.beats - b.beats)
      .filter((c, i, all) => i === 0 || Math.abs(c.beats - all[i - 1].beats) >= 1e-9);
    const points = [
      { beats: 0, bpm: saneBpm(startBpm), ramp: false },
      ...sorted.map((c) => ({ beats: c.beats, bpm: saneBpm(c.bpm), ramp: !!c.ramp })),
    ];
    this.segments = [];
    points.forEach((p, i) => {
      const next = points[i + 1];
      const slope = next && p.ramp ? (next.bpm - p.bpm) / (next.beats - p.beats) : 0;
      const last = this.segments[this.segments.length - 1];
      const seconds = last ? segmentSeconds(last, p.beats) : 0;
      this.segments.push({ beats: p.beats, seconds, bpm: p.bpm, slope });
    });
  }

  private atBeats(beats: number): Segment {
    let found = this.segments[0];
    for (const s of this.segments) {
      if (s.beats <= beats) found = s;
      else break;
    }
    return found;
  }

  private atSeconds(seconds: number): Segment {
    let found = this.segments[0];
    for (const s of this.segments) {
      if (s.seconds <= seconds) found = s;
      else break;
    }
    return found;
  }

  /** Seconds from the song start to `beats` (negative before it). */
  secondsAt(beats: number): number {
    return segmentSeconds(this.atBeats(beats), beats);
  }

  /** The beat heard `seconds` after the song start. */
  beatsAt(seconds: number): number {
    return segmentBeats(this.atSeconds(seconds), seconds);
  }

  secondsBetween(fromBeats: number, toBeats: number): number {
    return this.secondsAt(toBeats) - this.secondsAt(fromBeats);
  }

  /** The tempo at `beats` (on a change's beat, the new tempo). */
  bpmAt(beats: number): number {
    const s = this.atBeats(beats);
    return s.bpm + s.slope * (beats - s.beats);
  }

  get steady(): boolean {
    return this.segments.length === 1;
  }
}

/** The project's tempo map. */
export function tempoMap(project: Pick<Project, "tempo_bpm" | "tempo_changes">): TempoMap {
  return new TempoMap(project.tempo_bpm, project.tempo_changes ?? []);
}
