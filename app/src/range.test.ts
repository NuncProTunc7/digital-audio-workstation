import { describe, expect, it } from "vitest";
import { baseNoteForRange, isLimited, packRange, rangeText } from "./range";

describe("instrument ranges", () => {
  const flute = { low: 60, high: 96 };

  it("moves the typing keys up to a flute", () => {
    expect(baseNoteForRange(flute, 48)).toBe(60);
    expect(baseNoteForRange(flute, 36)).toBe(60);
  });

  it("leaves the keys alone when they already play inside the range", () => {
    expect(baseNoteForRange(flute, 72)).toBe(72);
    expect(baseNoteForRange({ low: 21, high: 108 }, 48)).toBe(48);
  });

  it("moves down to a bass and copes with ranges narrower than an octave", () => {
    expect(baseNoteForRange({ low: 28, high: 55 }, 72)).toBe(36);
    expect(baseNoteForRange({ low: 62, high: 70 }, 36)).toBe(60);
  });

  it("reads a loaded pack's range and names it", () => {
    expect(packRange({ state: "ready", lowest_note: 60, highest_note: 96 })).toEqual(flute);
    expect(packRange({ state: "loading" })).toBeNull();
    expect(rangeText(flute)).toBe("C4 to C7");
    expect(isLimited(flute)).toBe(true);
    expect(isLimited({ low: 21, high: 108 })).toBe(false);
  });
});
