import { describe, expect, it } from "vitest";
import { TempoMap } from "./tempo";

const change = (beats: number, bpm: number, ramp = false) => ({ id: 1, beats, bpm, ramp });

describe("TempoMap", () => {
  it("is sixty over the BPM at one tempo", () => {
    const m = new TempoMap(120);
    expect(m.secondsAt(8)).toBeCloseTo(4);
    expect(m.beatsAt(4)).toBeCloseTo(8);
    expect(m.secondsAt(-2)).toBeCloseTo(-1);
    expect(m.steady).toBe(true);
  });

  it("switches tempo on a change's beat", () => {
    const m = new TempoMap(120, [change(4, 60)]);
    expect(m.secondsAt(6)).toBeCloseTo(4);
    expect(m.beatsAt(4)).toBeCloseTo(6);
    expect(m.bpmAt(3.9)).toBe(120);
    expect(m.bpmAt(4)).toBe(60);
  });

  it("glides like the Rust map", () => {
    const m = new TempoMap(60, [change(4, 60, true), change(8, 120)]);
    expect(m.bpmAt(6)).toBeCloseTo(90);
    // Same numbers as daw-model's a_ramp_glides_to_the_next_tempo.
    expect(m.secondsAt(8)).toBeCloseTo(4 + 4 * Math.log(2), 9);
    for (const b of [0.5, 5.3, 7.9, 12]) expect(m.beatsAt(m.secondsAt(b))).toBeCloseTo(b, 9);
  });
});
