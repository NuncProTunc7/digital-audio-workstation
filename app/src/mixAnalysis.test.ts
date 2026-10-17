import { describe, expect, it } from "vitest";
import {
  bandLabel,
  bandRange,
  barTicks,
  loudnessVerdict,
  peakVerdict,
  previewAnalysis,
  stereoVerdict,
} from "./mixAnalysis";
import { defaultProject } from "./preview";

describe("mix analysis verdicts", () => {
  it("judges loudness against game music's -20 to -16 LUFS", () => {
    expect(loudnessVerdict(-18)).toBe("good");
    expect(loudnessVerdict(-16)).toBe("good");
    expect(loudnessVerdict(-14)).toBe("check");
    expect(loudnessVerdict(-22)).toBe("check");
    expect(loudnessVerdict(-9)).toBe("bad");
    expect(loudnessVerdict(-30)).toBe("bad");
    expect(loudnessVerdict(null)).toBe("bad");
  });

  it("wants peaks at -1 dBTP or below", () => {
    expect(peakVerdict(-3)).toBe("good");
    expect(peakVerdict(-1)).toBe("good");
    expect(peakVerdict(-0.5)).toBe("check");
    expect(peakVerdict(0.4)).toBe("bad");
  });

  it("flags mono and out-of-phase mixes", () => {
    expect(stereoVerdict(0.6)).toBe("good");
    expect(stereoVerdict(0.99)).toBe("check");
    expect(stereoVerdict(0.1)).toBe("check");
    expect(stereoVerdict(-0.2)).toBe("bad");
  });

  it("names bands plainly", () => {
    expect(bandLabel("low_mids")).toBe("Low mids");
    expect(bandRange(60, 250)).toBe("60–250 Hz");
    expect(bandRange(500, 2000)).toBe("500 Hz–2 kHz");
    expect(bandRange(2000, 6000)).toBe("2–6 kHz");
  });
});

describe("bar ticks under the spectrogram", () => {
  it("places bars by time, with the tail after the last bar", () => {
    const p = defaultProject(); // 120 BPM, 4/4: a bar is 2 s
    const ticks = barTicks(p, { start_beats: 0, end_beats: 16, mix: { duration_s: 10 } as never });
    expect(ticks.map((t) => t.bar)).toEqual([1, 2, 3, 4, 5]);
    expect(ticks[1].at).toBeCloseTo(0.2); // bar 2 starts at 2 s of 10
    expect(ticks[4].at).toBeCloseTo(0.8); // the song's end, before the tail
  });

  it("follows tempo changes", () => {
    const p = { ...defaultProject(), tempo_changes: [{ id: 99, beats: 4, bpm: 60 }] };
    const ticks = barTicks(p, { start_beats: 0, end_beats: 8, mix: { duration_s: 6 } as never });
    // Bar 1 takes 2 s at 120 BPM; bar 2 takes 4 s at 60 BPM.
    expect(ticks.map((t) => [t.bar, Number(t.at.toFixed(3))])).toEqual([
      [1, 0],
      [2, 0.333],
      [3, 1],
    ]);
  });

  it("thins labels on long songs", () => {
    const p = defaultProject();
    const ticks = barTicks(p, { start_beats: 0, end_beats: 400, mix: { duration_s: 201.5 } as never }, 10);
    expect(ticks.length).toBeLessThanOrEqual(11);
    expect(ticks[0].bar).toBe(1);
  });
});

describe("preview analysis", () => {
  it("shares out the energy between tracks that play", () => {
    const a = previewAnalysis(defaultProject(), null, null);
    expect(a.mix.integrated_lufs).toBeNull(); // the default song is empty
    expect(a.hints[0]).toContain("silent");
  });
});
