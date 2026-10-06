import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { createPreviewBackend } from "./backend";
import type { Backend } from "./backend";

afterEach(cleanup);

function spyBackend(): Backend {
  const b = createPreviewBackend();
  vi.spyOn(b, "noteOn");
  vi.spyOn(b, "noteOff");
  vi.spyOn(b, "execute");
  vi.spyOn(b, "play");
  vi.spyOn(b, "selectTrack");
  return b;
}

async function renderApp(backend: Backend = createPreviewBackend()) {
  render(<App backend={backend} />);
  await screen.findByLabelText("Tempo in BPM");
  return backend;
}

describe("App", () => {
  it("shows the default project with three tracks", async () => {
    await renderApp();
    expect((screen.getByLabelText("Tempo in BPM") as HTMLInputElement).value).toBe("120");
    expect((screen.getByLabelText("Time signature") as HTMLSelectElement).value).toBe("4/4");
    const tracks = screen.getByRole("listbox", { name: "Tracks" });
    const selected = tracks.querySelector('[aria-selected="true"]');
    expect(selected?.textContent).toContain("Keys");
    expect(screen.getByText("Bass")).toBeTruthy();
    expect(screen.getByText("Drums")).toBeTruthy();
    expect((screen.getByLabelText("Preset") as HTMLSelectElement).value).toBe("Warm Keys");
  });

  it("sends a tempo Command and can undo it", async () => {
    await renderApp();
    const tempo = screen.getByLabelText("Tempo in BPM");
    fireEvent.change(tempo, { target: { value: "90" } });
    fireEvent.blur(tempo);
    expect(await screen.findByDisplayValue("90")).toBeTruthy();
    fireEvent.click(screen.getByText("Undo", { exact: false }));
    expect(await screen.findByDisplayValue("120")).toBeTruthy();
  });

  it("rejects non-numeric tempo", async () => {
    await renderApp();
    const tempo = screen.getByLabelText("Tempo in BPM");
    fireEvent.change(tempo, { target: { value: "fast" } });
    fireEvent.blur(tempo);
    expect((await screen.findByRole("alert")).textContent).toContain("not a number");
  });

  it("plays notes from the computer keyboard", async () => {
    const backend = await renderApp(spyBackend());
    fireEvent.keyDown(window, { code: "KeyA", key: "a" });
    expect(backend.noteOn).toHaveBeenCalledWith(1, 60, 100 / 127);
    // Held key repeats don't retrigger.
    fireEvent.keyDown(window, { code: "KeyA", key: "a", repeat: true });
    expect(backend.noteOn).toHaveBeenCalledTimes(1);
    fireEvent.keyUp(window, { code: "KeyA", key: "a" });
    expect(backend.noteOff).toHaveBeenCalledWith(1, 60);
  });

  it("shifts octave and velocity", async () => {
    const backend = await renderApp(spyBackend());
    fireEvent.keyDown(window, { code: "KeyZ", key: "z" });
    fireEvent.keyDown(window, { code: "KeyC", key: "c" });
    fireEvent.keyDown(window, { code: "KeyW", key: "w" });
    expect(backend.noteOn).toHaveBeenCalledWith(1, 49, 80 / 127);
  });

  it("maps keys to drum pads on the drum track", async () => {
    const backend = await renderApp(spyBackend());
    fireEvent.click(screen.getByText("Drums"));
    expect(backend.selectTrack).toHaveBeenCalledWith(3);
    expect(await screen.findByTitle("Kick (note 36)")).toBeTruthy();
    fireEvent.keyDown(window, { code: "KeyA", key: "a" });
    expect(backend.noteOn).toHaveBeenCalledWith(3, 36, 100 / 127);
    fireEvent.pointerDown(screen.getByTitle("Snare (note 38)"));
    expect(backend.noteOn).toHaveBeenCalledWith(3, 38, 100 / 127);
  });

  it("plays notes from the on-screen piano", async () => {
    const backend = await renderApp(spyBackend());
    const c4 = document.querySelector('[data-note="60"]');
    expect(c4).toBeTruthy();
    fireEvent.pointerDown(c4 as Element);
    expect(backend.noteOn).toHaveBeenCalledWith(1, 60, 100 / 127);
    fireEvent.pointerUp(window);
    expect(backend.noteOff).toHaveBeenCalledWith(1, 60);
  });

  it("sends parameter and preset Commands", async () => {
    const backend = await renderApp(spyBackend());
    fireEvent.change(screen.getAllByLabelText("Cutoff")[0], { target: { value: "500" } });
    expect(backend.execute).toHaveBeenCalledWith(
      expect.objectContaining({ command: "set_instrument_param", track_id: 1, param: "filter.cutoff_hz" }),
    );
    fireEvent.change(screen.getByLabelText("Preset"), { target: { value: "Pluck" } });
    expect(backend.execute).toHaveBeenCalledWith({ command: "load_preset", track_id: 1, preset: "Pluck" });
    expect(await screen.findByDisplayValue("Pluck")).toBeTruthy();
  });

  it("starts playback with the space bar", async () => {
    const backend = await renderApp(spyBackend());
    await act(async () => {
      fireEvent.keyDown(window, { code: "Space", key: " " });
    });
    expect(backend.play).toHaveBeenCalled();
  });
});
