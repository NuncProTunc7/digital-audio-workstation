import { describe, expect, it } from "vitest";
import { formatParam, formatPosition, fromSlider, toSlider } from "./format";
import { labelForNote, noteForCode, noteName, shiftOctave, stepVelocity } from "./keymap";
import type { ParamSpec } from "./types";

const cutoff: ParamSpec = {
  id: "filter.cutoff_hz",
  name: "Cutoff",
  group: "Filter",
  min: 20,
  max: 20000,
  default: 2000,
  unit: "hertz",
  log_scale: true,
  choices: [],
};

describe("musical typing", () => {
  it("maps keys to notes from the base note", () => {
    expect(noteForCode("KeyA", 60)).toBe(60);
    expect(noteForCode("KeyW", 60)).toBe(61);
    expect(noteForCode("KeyK", 60)).toBe(72);
    expect(noteForCode("Quote", 60)).toBe(77);
    expect(noteForCode("KeyQ", 60)).toBeNull();
  });

  it("labels keys and names notes", () => {
    expect(labelForNote(62, 60)).toBe("S");
    expect(labelForNote(59, 60)).toBeNull();
    expect(noteName(60)).toBe("C4");
    expect(noteName(69)).toBe("A4");
  });

  it("clamps octave and velocity", () => {
    expect(shiftOctave(24, -1)).toBe(24);
    expect(shiftOctave(60, 1)).toBe(72);
    expect(stepVelocity(127, 1)).toBe(127);
    expect(stepVelocity(20, -1)).toBe(20);
    expect(stepVelocity(100, -1)).toBe(80);
  });
});

describe("formatting", () => {
  it("formats parameter values", () => {
    expect(formatParam(cutoff, 440)).toBe("440 Hz");
    expect(formatParam(cutoff, 2100)).toBe("2.1 kHz");
    expect(formatParam({ ...cutoff, unit: "seconds" }, 0.12)).toBe("120 ms");
    expect(formatParam({ ...cutoff, unit: "percent" }, 0.7)).toBe("70%");
    expect(formatParam({ ...cutoff, choices: ["Sine", "Saw"] }, 1)).toBe("Saw");
  });

  it("round-trips log sliders", () => {
    for (const v of [20, 200, 2000, 20000]) {
      expect(Math.abs(fromSlider(cutoff, toSlider(cutoff, v)) - v) / v).toBeLessThan(0.01);
    }
    expect(toSlider(cutoff, 632.5)).toBe(500);
  });

  it("formats bar.beat positions", () => {
    expect(formatPosition(0, 4)).toBe("1.1");
    expect(formatPosition(5.5, 4)).toBe("2.2");
    expect(formatPosition(3, 3)).toBe("2.1");
  });
});
