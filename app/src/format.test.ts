import { describe, expect, it } from "vitest";
import { parseDb, parsePan, parseParam } from "./format";
import type { ParamSpec } from "./types";

const spec = (unit: ParamSpec["unit"], min: number, max: number): ParamSpec => ({
  id: "p",
  name: "P",
  group: "G",
  min,
  max,
  default: min,
  unit,
  log_scale: false,
  choices: [],
});

describe("typed values", () => {
  it("reads frequencies in Hz or kHz", () => {
    const cutoff = spec("hertz", 20, 20000);
    expect(parseParam(cutoff, "2500", 1000)).toBe(2500);
    expect(parseParam(cutoff, "2.5k", 1000)).toBe(2500);
    expect(parseParam(cutoff, "2.5 kHz", 1000)).toBe(2500);
    expect(parseParam(cutoff, "440hz", 1000)).toBe(440);
  });

  it("reads times in the unit the readout shows unless one is typed", () => {
    const attack = spec("seconds", 0.001, 5);
    // The readout shows "120 ms" for 0.12 s, so a bare number is ms...
    expect(parseParam(attack, "250", 0.12)).toBeCloseTo(0.25);
    // ...and "1.50 s" for 1.5 s, so a bare number is seconds.
    expect(parseParam(attack, "2", 1.5)).toBe(2);
    expect(parseParam(attack, "80 ms", 1.5)).toBeCloseTo(0.08);
    expect(parseParam(attack, "0.5s", 0.12)).toBe(0.5);
  });

  it("reads percent, decibels, semitones and ratios", () => {
    expect(parseParam(spec("percent", 0, 1), "35%", 0)).toBeCloseTo(0.35);
    expect(parseParam(spec("percent", 0, 1), "35", 0)).toBeCloseTo(0.35);
    expect(parseParam(spec("decibels", -24, 24), "+3.5 dB", 0)).toBe(3.5);
    expect(parseParam(spec("semitones", -24, 24), "-7", 0)).toBe(-7);
    expect(parseParam(spec("semitones", -24, 24), "4.6 st", 0)).toBe(5);
    expect(parseParam(spec("ratio", 1, 20), "4:1", 2)).toBe(4);
  });

  it("refuses text that isn't a number in the right unit", () => {
    const cutoff = spec("hertz", 20, 20000);
    expect(parseParam(cutoff, "loud", 1000)).toBeNull();
    expect(parseParam(cutoff, "", 1000)).toBeNull();
    expect(parseParam(cutoff, "3 dB", 1000)).toBeNull();
  });

  it("reads fader levels and pan positions", () => {
    expect(parseDb("-3.5")).toBe(-3.5);
    expect(parseDb("+2 dB")).toBe(2);
    expect(parseDb("-inf")).toBe(-60);
    expect(parseDb("−∞ dB")).toBe(-60);
    expect(parseDb("abc")).toBeNull();
    expect(parsePan("C")).toBe(0);
    expect(parsePan("30L")).toBeCloseTo(-0.3);
    expect(parsePan("45 r")).toBeCloseTo(0.45);
    expect(parsePan("-20")).toBeCloseTo(-0.2);
    expect(parsePan("left")).toBeNull();
  });
});
