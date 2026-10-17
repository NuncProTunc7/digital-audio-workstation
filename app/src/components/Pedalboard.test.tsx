import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";
import catalogJson from "../generated/instruments.json";
import { applyCommand, defaultProject } from "../preview";
import type { Catalog, Command, Project } from "../types";
import Pedalboard from "./Pedalboard";

const catalog = catalogJson as unknown as Catalog;

afterEach(cleanup);

function setup() {
  const commands: Command[] = [];
  let ends = 0;
  function Harness() {
    const [project, setProject] = useState<Project>(defaultProject());
    const track = project.tracks[0];
    return (
      <Pedalboard
        track={track}
        catalog={catalog}
        onCommand={async (c) => {
          commands.push(c);
          setProject((p) => applyCommand(p, c));
        }}
        onEndGesture={() => ends++}
      />
    );
  }
  render(<Harness />);
  const pedals = () => [...document.querySelectorAll(".pedal .pedal-name")].map((e) => e.textContent);
  return { commands, pedals, ends: () => ends };
}

describe("Pedalboard", () => {
  it("loads a rig as one step and shows its pedals", async () => {
    const s = setup();
    expect(s.pedals()).toEqual([]);
    await act(async () => fireEvent.change(screen.getByLabelText("Rig"), { target: { value: "Metal" } }));
    expect(s.commands).toEqual([{ command: "load_rig", track_id: 1, rig: "Metal" }]);
    expect(s.pedals()).toEqual(["Noise gate", "Overdrive", "Amp", "Reverb"]);
    expect(s.ends()).toBe(1);
  });

  it("adds a pedal, stomps it off, and moves it", async () => {
    const s = setup();
    await act(async () => fireEvent.change(screen.getByLabelText("Add a pedal"), { target: { value: "fuzz" } }));
    await act(async () => fireEvent.change(screen.getByLabelText("Add a pedal"), { target: { value: "amp" } }));
    expect(s.pedals()).toEqual(["Fuzz", "Amp"]);
    const fuzz = screen.getAllByRole("group", { name: /pedal/ })[0];
    await act(async () => fireEvent.click(within(fuzz).getByRole("button", { name: "Fuzz on" })));
    expect(s.commands.at(-1)).toMatchObject({ command: "set_effect_enabled", enabled: false });
    expect(within(fuzz).getByRole("button", { name: "Fuzz off" })).toBeTruthy();
    // Amp to the front: one step that keeps its id.
    const amp = screen.getAllByRole("group", { name: /pedal/ })[1];
    await act(async () => fireEvent.click(within(amp).getByRole("button", { name: "Move Amp earlier" })));
    expect(s.pedals()).toEqual(["Amp", "Fuzz"]);
    expect(s.commands.at(-1)).toMatchObject({ command: "batch" });
  });
});
