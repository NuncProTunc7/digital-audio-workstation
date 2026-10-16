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

function setup(start: Project = defaultProject()): Spies {
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
    expect(within(document.querySelector(".sound-report") as HTMLElement).getByText(/res:\/\/sfx\/coin.wav/)).toBeTruthy();
  });

  it("space plays the selected sound", async () => {
    const s = setup();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Coin" })));
    s.onPlay.mockClear();
    fireEvent.keyDown(screen.getByRole("dialog"), { code: "Space", key: " " });
    expect(s.onPlay).toHaveBeenCalledTimes(1);
  });
});
