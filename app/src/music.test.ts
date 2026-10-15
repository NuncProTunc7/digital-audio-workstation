import { describe, expect, it } from "vitest";
import { guitarStringsFor } from "./music";

const STANDARD = [40, 45, 50, 55, 59, 64];

describe("guitarStringsFor", () => {
  it("puts an open E chord one note per string", () => {
    const m = guitarStringsFor([64, 40, 56, 47, 59, 52], STANDARD, 22);
    expect([40, 47, 52, 56, 59, 64].map((n) => m.get(n))).toEqual([0, 1, 2, 3, 4, 5]);
  });

  it("uses the highest string that reaches a note", () => {
    // A3 is above the G string's open note but below the B string.
    expect(guitarStringsFor([57], STANDARD, 22).get(57)).toBe(3);
    // In DADGAD the 2nd string is tuned to A3: it plays it open.
    expect(guitarStringsFor([57], [38, 45, 50, 55, 57, 62], 22).get(57)).toBe(4);
  });

  it("skips notes no string can reach", () => {
    expect(guitarStringsFor([20], STANDARD, 22).size).toBe(0);
  });
});
