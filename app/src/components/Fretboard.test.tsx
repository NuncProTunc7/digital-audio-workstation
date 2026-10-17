import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import Fretboard from "./Fretboard";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const STANDARD = { name: "Standard (E A D G B E)", strings: [40, 45, 50, 55, 59, 64] };

describe("Fretboard chord buttons", () => {
  it("strums a chord shape low to high and lets it go on release", () => {
    vi.useFakeTimers();
    const played: number[] = [];
    const released: number[] = [];
    render(
      <Fretboard
        tuning={STANDARD}
        capo={0}
        frets={22}
        activeNotes={new Set()}
        chords={[
          { root: 0, quality: "major" },
          { root: 9, quality: "minor" },
        ]}
        onPlay={(n) => played.push(n)}
        onRelease={(n) => released.push(n)}
      />,
    );
    const c = screen.getByRole("button", { name: "Play C" });
    expect(c.title).toContain("x32010");
    fireEvent.pointerDown(c);
    act(() => {
      vi.advanceTimersByTime(200);
    });
    expect(played).toEqual([48, 52, 55, 60, 64]);
    fireEvent.pointerUp(c);
    act(() => {
      vi.advanceTimersByTime(10);
    });
    expect(released.sort()).toEqual([48, 52, 55, 60, 64]);
    expect(screen.getByRole("button", { name: "Play Am" })).toBeTruthy();
  });

  it("follows the capo", () => {
    render(
      <Fretboard
        tuning={STANDARD}
        capo={2}
        frets={22}
        activeNotes={new Set()}
        chords={[{ root: 2, quality: "major" }]}
        onPlay={() => {}}
        onRelease={() => {}}
      />,
    );
    // D with a capo on 2 is the C shape, written from the capo.
    expect(screen.getByRole("button", { name: "Play D" }).title).toContain("x32010");
  });
});
