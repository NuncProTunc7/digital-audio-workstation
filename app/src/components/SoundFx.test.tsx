import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi, type Mock } from "vitest";
import { applyCommand, defaultProject } from "../preview";
import type { Command, Project, SoundExportOptions, SoundExportReport } from "../types";
import SoundFx from "./SoundFx";

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

interface Spies {
  onPlay: Mock<(id: number) => void>;
  onEndGesture: Mock<() => void>;
  onExport: Mock<(o: SoundExportOptions) => Promise<SoundExportReport>>;
  commands: Command[];
}

interface Options {
  /** Commands the app refuses (resolve to undefined, like a rejected one). */
  reject?: (c: Command) => boolean;
  /** Holds each command's answer until the test lets it go. */
  hold?: { release: (() => void)[] };
}

function setup(start: Project = defaultProject(), options: Options = {}): Spies {
  const spies: Spies = {
    onPlay: vi.fn<(id: number) => void>(),
    onEndGesture: vi.fn<() => void>(),
    onExport: vi.fn(
      async (o: SoundExportOptions): Promise<SoundExportReport> => ({
        files: (o.sound_ids ?? [1]).map((id) => ({
          sound_id: id,
          res_path: "res://sfx/coin.wav",
          seconds: 0.25,
          peak_dbfs: -6,
          turned_down_db: 0,
        })),
        left_behind: ["res://sfx/old_coin.wav"],
      }),
    ),
    commands: [],
  };
  function Harness() {
    const [project, setProject] = useState(start);
    return (
      <SoundFx
        project={project}
        onCommand={async (c) => {
          spies.commands.push(c);
          if (options.hold) await new Promise<void>((r) => options.hold?.release.push(r));
          if (options.reject?.(c)) return undefined;
          const next = applyCommand(project, c);
          setProject(next);
          return next;
        }}
        onEndGesture={spies.onEndGesture}
        onPlay={spies.onPlay}
        onExport={spies.onExport}
        onPickFolder={async () => "C:\\Games\\Farm"}
        onClose={() => {}}
      />
    );
  }
  render(<Harness />);
  return spies;
}

function soundList(): string[] {
  return [...document.querySelectorAll(".sound-list .sound-name")].map((e) => e.textContent ?? "");
}

describe("SoundFx", () => {
  it("a category button adds and selects a sound", async () => {
    const s = setup();
    expect(screen.getByText(/No sounds yet/)).toBeTruthy();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Coin" })));
    expect(soundList()).toEqual(["coin"]);
    expect(document.querySelector(".sound-list .selected")?.textContent).toContain("coin");
    expect(s.commands[0]).toMatchObject({ command: "add_sound", category: "coin" });
    expect(s.onPlay).toHaveBeenCalledTimes(1);
  });

  it("+ New starts a plain sound to shape from scratch", async () => {
    const s = setup();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "+ New" })));
    expect(soundList()).toEqual(["sound"]);
    expect(document.querySelector(".sound-list .selected")?.textContent).toContain("sound");
    // No category: the plain starting recipe, not a generated one.
    expect(s.commands[0]).toEqual({ command: "add_sound" });
    expect(s.onPlay).toHaveBeenCalledTimes(1);
  });

  it("a slider drag is one undo step", async () => {
    const s = setup();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Jump" })));
    s.onPlay.mockClear();
    const pitch = screen.getByLabelText("Pitch");
    await act(async () => {
      for (const v of ["0.2", "0.3", "0.4"]) fireEvent.change(pitch, { target: { value: v } });
    });
    expect(s.onPlay).not.toHaveBeenCalled();
    await act(async () => fireEvent.pointerUp(pitch));
    expect(s.commands.filter((c) => c.command === "set_sound")).toHaveLength(3);
    expect(s.onEndGesture).toHaveBeenCalledTimes(2); // the add, then the drag
    expect(s.onPlay).toHaveBeenCalledTimes(1);
    const last = s.commands.at(-1);
    expect(last?.command === "set_sound" && last.recipe.base_freq).toBeCloseTo(0.4);
    // Only the slider's own setting is sent, so other changes are kept.
    expect(last?.command === "set_sound" && Object.keys(last.recipe)).toEqual(["base_freq"]);
  });

  it("vary changes the recipe", async () => {
    const s = setup();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Laser" })));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: /Vary/ })));
    expect(s.commands.at(-1)).toMatchObject({ command: "vary_sound" });
    expect(s.onPlay).toHaveBeenCalledTimes(2);
  });

  it("more shows every setting", async () => {
    setup();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Blip" })));
    expect(screen.queryByLabelText("Vibrato")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /More/ }));
    for (const label of ["Wave", "Vibrato", "Pitch jump", "Square width", "Phaser", "Resonance", "Thin", "Repeat"]) {
      expect(screen.getByLabelText(label)).toBeTruthy();
    }
  });

  it("rename and delete work", async () => {
    const s = setup();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Hit" })));
    fireEvent.doubleClick(screen.getByText("hit"));
    const field = screen.getByLabelText("Sound name");
    fireEvent.change(field, { target: { value: "punch" } });
    await act(async () => fireEvent.keyDown(field, { key: "Enter" }));
    expect(soundList()).toEqual(["punch"]);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Delete sound" })));
    expect(soundList()).toEqual([]);
    expect(s.commands.map((c) => c.command)).toEqual(["add_sound", "rename_sound", "remove_sound"]);
  });

  it("export lists the files written", async () => {
    const s = setup();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Coin" })));
    fireEvent.click(screen.getByRole("button", { name: "Browse…" }));
    await screen.findByDisplayValue("C:\\Games\\Farm");
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Export this sound" })));
    expect(s.onExport).toHaveBeenCalledWith(
      expect.objectContaining({ project_dir: "C:\\Games\\Farm", folder: "sfx", sound_ids: [expect.any(Number)] }),
    );
    const report = within(document.querySelector(".sound-report") as HTMLElement);
    expect(report.getByText(/res:\/\/sfx\/coin.wav/)).toBeTruthy();
    expect(report.getByText(/old_coin.wav is still there from before a rename/)).toBeTruthy();
  });

  it("keys in the panel don't reach the song's shortcuts", async () => {
    setup();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Coin" })));
    const heard: string[] = [];
    const listener = (e: KeyboardEvent) => heard.push(e.key);
    window.addEventListener("keydown", listener);
    try {
      const slider = screen.getByLabelText("Pitch");
      fireEvent.keyDown(slider, { key: "Delete" });
      fireEvent.keyDown(slider, { key: "r" });
      fireEvent.keyDown(slider, { key: "z", ctrlKey: true });
    } finally {
      window.removeEventListener("keydown", listener);
    }
    // Undo still works; Delete and the record key stay in the panel.
    expect(heard).toEqual(["z"]);
  });

  it("space plays the selected sound", async () => {
    const s = setup();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Coin" })));
    s.onPlay.mockClear();
    fireEvent.keyDown(screen.getByRole("dialog"), { code: "Space", key: " " });
    expect(s.onPlay).toHaveBeenCalledTimes(1);
  });

  it("a rejected wave change doesn't play", async () => {
    const s = setup(defaultProject(), {
      reject: (c) => c.command === "set_sound" && c.recipe.wave !== undefined,
    });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Coin" })));
    fireEvent.click(screen.getByRole("button", { name: /More/ }));
    s.onPlay.mockClear();
    await act(async () => fireEvent.change(screen.getByLabelText("Wave"), { target: { value: "saw" } }));
    expect(s.onPlay).not.toHaveBeenCalled();
    // An accepted change plays.
    s.onPlay.mockClear();
    await act(async () => fireEvent.change(screen.getByLabelText("Vibrato"), { target: { value: "0.5" } }));
    await act(async () => fireEvent.pointerUp(screen.getByLabelText("Vibrato")));
    expect(s.onPlay).toHaveBeenCalledTimes(1);
  });

  it("length down to nothing and back keeps the hold/fade balance", async () => {
    const s = setup();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Coin" })));
    const sets = () => s.commands.filter((c) => c.command === "set_sound");
    fireEvent.click(screen.getByRole("button", { name: /More/ }));
    await act(async () => fireEvent.change(screen.getByLabelText("Hold"), { target: { value: "0.1" } }));
    await act(async () => fireEvent.change(screen.getByLabelText("Fade out"), { target: { value: "0.3" } }));
    const length = screen.getByLabelText("Length");
    await act(async () => fireEvent.change(length, { target: { value: "0" } }));
    await act(async () => fireEvent.change(length, { target: { value: "0.8" } }));
    const last = sets().at(-1);
    if (last?.command !== "set_sound") throw new Error("no set_sound");
    expect(last.recipe.sustain).toBeCloseTo(0.2, 2);
    expect(last.recipe.decay).toBeCloseTo(0.6, 2);
  });

  it("letting go of a slider plays only after its last change is in", async () => {
    const hold = { release: [] as (() => void)[] };
    const s = setup(defaultProject(), { hold });
    act(() => fireEvent.click(screen.getByRole("button", { name: "Coin" })));
    await act(async () => hold.release.splice(0).forEach((r) => r()));
    s.onPlay.mockClear();
    const pitch = screen.getByLabelText("Pitch");
    act(() => fireEvent.change(pitch, { target: { value: "0.7" } }));
    act(() => fireEvent.pointerUp(pitch));
    await act(async () => {});
    expect(s.onPlay).not.toHaveBeenCalled();
    await act(async () => hold.release.splice(0).forEach((r) => r()));
    expect(s.onPlay).toHaveBeenCalledTimes(1);
  });

  it("warns when a setting makes the sound silent", async () => {
    setup();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Blip" })));
    expect(screen.queryByText(/below Lowest pitch/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /More/ }));
    await act(async () => fireEvent.change(screen.getByLabelText("Pitch"), { target: { value: "0.2" } }));
    await act(async () => fireEvent.change(screen.getByLabelText("Lowest pitch"), { target: { value: "0.4" } }));
    expect(screen.getByText(/below Lowest pitch/)).toBeTruthy();
  });
});
