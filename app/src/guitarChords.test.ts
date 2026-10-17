import { describe, expect, it } from "vitest";
import { chordShape, shapeText } from "./guitarChords";

const STANDARD = [40, 45, 50, 55, 59, 64];
const DROP_D = [38, 45, 50, 55, 59, 64];
const shape = (root: number, quality: Parameters<typeof chordShape>[0]["quality"], open = STANDARD, bass?: number) =>
  shapeText(chordShape({ root, quality, bass }, open, 22));

describe("guitar chord shapes", () => {
  it("finds the open shapes guitarists know in standard tuning", () => {
    expect(shape(0, "major")).toBe("x32010"); // C
    expect(shape(7, "major")).toBe("320003"); // G
    expect(shape(2, "major")).toBe("xx0232"); // D
    expect(shape(9, "minor")).toBe("x02210"); // Am
    expect(shape(4, "major")).toBe("022100"); // E
    expect(shape(4, "minor")).toBe("022000"); // Em
    expect(shape(2, "minor")).toBe("xx0231"); // Dm
    expect(shape(9, "major")).toBe("x02220"); // A
  });

  it("puts the root (or the slash note) lowest", () => {
    // C/E: the open low E under a C shape.
    expect(shape(0, "major", STANDARD, 4)).toBe("032010");
  });

  it("uses the tuning: drop D's low string rings under a D chord", () => {
    expect(shape(2, "major", DROP_D)).toBe("000232");
  });

  it("moves up the neck when there is no open shape", () => {
    const fsharp = chordShape({ root: 6, quality: "major" }, STANDARD, 22);
    const notes = fsharp.notes.map((n) => n % 12);
    expect(new Set(notes)).toEqual(new Set([6, 10, 1]));
    expect(notes[0]).toBe(6);
    expect(fsharp.frets.filter((f) => f !== null).every((f) => (f ?? 0) <= 22)).toBe(true);
  });
});
