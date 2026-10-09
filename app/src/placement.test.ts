import { describe, expect, it } from "vitest";
import { clampToWindow } from "./placement";

describe("menus stay on screen", () => {
  it("leaves a menu that fits where it was opened", () => {
    expect(clampToWindow(100, 100, 200, 150, 1024, 768)).toEqual({ left: 100, top: 100 });
  });

  it("pulls a menu opened near the right or bottom edge back on screen", () => {
    // Found 2026-10-09: the step-grid menu on the last columns was cut off.
    expect(clampToWindow(1000, 700, 200, 150, 1024, 768)).toEqual({ left: 816, top: 610 });
  });

  it("keeps the top-left corner visible when the menu is bigger than the window", () => {
    expect(clampToWindow(50, 50, 2000, 2000, 1024, 768)).toEqual({ left: 8, top: 8 });
  });
});
