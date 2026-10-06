import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
  vi.spyOn(b, "recordStart");
  vi.spyOn(b, "locate");
  return b;
}

function trackHeader(name: string): HTMLElement {
  const header = [...document.querySelectorAll<HTMLElement>(".track-header")].find((h) =>
    h.querySelector(".track-name")?.textContent?.includes(name),
  );
  if (!header) throw new Error(`no track header ${name}`);
  return header;
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
    const selected = document.querySelector(".track-header.selected");
    expect(selected?.textContent).toContain("Keys");
    expect(document.querySelectorAll(".lane").length).toBe(3);
    expect((screen.getByLabelText("Preset") as HTMLSelectElement).value).toBe("Warm Keys");
  });

  it("sends a tempo Command and can undo it", async () => {
    await renderApp();
    const tempo = screen.getByLabelText("Tempo in BPM");
    fireEvent.change(tempo, { target: { value: "90" } });
    fireEvent.blur(tempo);
    expect(await screen.findByDisplayValue("90")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
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
    fireEvent.click(trackHeader("Drums"));
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

describe("Arranging", () => {
  it("creates a clip by double-clicking a lane and opens the piano roll", async () => {
    const backend = await renderApp(spyBackend());
    const lane = document.querySelectorAll(".lane")[0];
    await act(async () => {
      fireEvent.doubleClick(lane, { clientX: 10, clientY: 10 });
    });
    expect(backend.execute).toHaveBeenCalledWith(
      expect.objectContaining({ command: "create_clip", track_id: 1, start_beats: 0, length_beats: 4 }),
    );
    expect(await screen.findByRole("tab", { selected: true })).toHaveProperty(
      "textContent",
      expect.stringContaining("Piano roll"),
    );
    expect(document.querySelector(".clip")).toBeTruthy();
  });

  it("adds a note when clicking the piano roll grid", async () => {
    const backend = await renderApp(spyBackend());
    await act(async () => {
      fireEvent.doubleClick(document.querySelectorAll(".lane")[0], { clientX: 10, clientY: 10 });
    });
    const grid = await waitForElement(".roll-grid");
    await act(async () => {
      fireEvent.pointerDown(grid, { clientX: 5, clientY: 5, button: 0 });
    });
    expect(backend.execute).toHaveBeenCalledWith(expect.objectContaining({ command: "add_notes" }));
    expect(document.querySelectorAll(".roll-note").length).toBe(1);
  });

  it("deletes the selected clip with the Delete key and undoes it", async () => {
    await renderApp(spyBackend());
    await act(async () => {
      fireEvent.doubleClick(document.querySelectorAll(".lane")[1], { clientX: 10, clientY: 10 });
    });
    expect(document.querySelectorAll(".clip").length).toBe(1);
    // Back to the timeline: deleting applies to the clip, not notes.
    fireEvent.click(screen.getByRole("tab", { name: /Mixer/ }));
    await act(async () => {
      fireEvent.keyDown(window, { code: "Delete", key: "Delete" });
    });
    expect(document.querySelectorAll(".clip").length).toBe(0);
    await act(async () => {
      fireEvent.keyDown(window, { code: "KeyZ", key: "z", ctrlKey: true });
    });
    expect(document.querySelectorAll(".clip").length).toBe(1);
  });

  it("adds an effect from the mixer", async () => {
    const backend = await renderApp(spyBackend());
    fireEvent.click(screen.getByRole("tab", { name: /Mixer/ }));
    await act(async () => {
      fireEvent.change(screen.getByLabelText("Add effect to Keys"), { target: { value: "reverb" } });
    });
    expect(backend.execute).toHaveBeenCalledWith({ command: "add_effect", track_id: 1, kind: "reverb", index: null });
    expect(await screen.findByRole("button", { name: "▸ Reverb" })).toBeTruthy();
  });

  it("moves a mixer fader with the arrow keys", async () => {
    const backend = await renderApp(spyBackend());
    fireEvent.click(screen.getByRole("tab", { name: /Mixer/ }));
    const fader = screen.getByRole("slider", { name: "Keys volume" });
    await act(async () => {
      fireEvent.keyDown(fader, { key: "ArrowDown" });
    });
    expect(backend.execute).toHaveBeenCalledWith(
      expect.objectContaining({ command: "set_track_mixer", track_id: 1, volume_db: -1 }),
    );
    expect(screen.getByRole("slider", { name: "Keys volume" }).getAttribute("aria-valuenow")).toBe("-1");
  });

  it("records onto the selected track with R", async () => {
    const backend = await renderApp(spyBackend());
    fireEvent.click(trackHeader("Bass"));
    await act(async () => {
      fireEvent.keyDown(window, { code: "KeyR", key: "r" });
    });
    expect(backend.recordStart).toHaveBeenCalledWith(2);
  });

  it("adds a track", async () => {
    await renderApp(spyBackend());
    await act(async () => {
      fireEvent.click(screen.getByText("+ Drum track"));
    });
    expect(document.querySelectorAll(".lane").length).toBe(4);
  });

  it("shows Claude's edits as they happen", async () => {
    const backend = createPreviewBackend();
    await renderApp(backend);
    await act(async () => {
      backend.simulateRemoteChange(
        {
          command: "batch",
          commands: [
            { command: "set_tempo", bpm: 150 },
            { command: "add_track", name: "Lead", instrument: "synth", preset: null, index: null },
          ],
        },
        "Claude: batch",
      );
    });
    expect(await screen.findByDisplayValue("150")).toBeTruthy();
    expect(document.querySelectorAll(".lane").length).toBe(4);
    expect(screen.getByRole("status").textContent).toBe("Claude: batch");
    // Claude's change is one undo step.
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(await screen.findByDisplayValue("120")).toBeTruthy();
    expect(document.querySelectorAll(".lane").length).toBe(3);
  });

  it("opens the Claude panel with setup instructions", async () => {
    await renderApp();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Claude" }));
    });
    const panel = await screen.findByRole("dialog", { name: "Claude" });
    expect(panel.textContent).toContain("claude mcp add");
    expect(panel.textContent).toContain("Nothing yet this session.");
    fireEvent.click(screen.getByRole("button", { name: "Close Claude panel" }));
    expect(screen.queryByRole("dialog", { name: "Claude" })).toBeNull();
  });
});

describe("Audio", () => {
  async function dropRecording(backend = createPreviewBackend(), file = "C:\\Phone\\Voice Memo 3.m4a") {
    await renderApp(backend);
    await act(async () => {
      backend.simulateFileDrop([file], 400, 100);
      await new Promise((r) => setTimeout(r, 20));
    });
    return backend;
  }

  it("adds an audio track with a microphone panel", async () => {
    const backend = createPreviewBackend();
    vi.spyOn(backend, "monitorInput");
    await renderApp(backend);
    await act(async () => {
      fireEvent.click(screen.getByText("+ Audio track"));
    });
    expect(document.querySelectorAll(".lane").length).toBe(4);
    fireEvent.click(trackHeader("Audio"));
    expect(await screen.findByRole("tab", { name: "Audio · Audio" })).toBeTruthy();
    expect(screen.getByLabelText("Audio input")).toBeTruthy();
    expect(backend.monitorInput).toHaveBeenCalledWith(true);
    // Leaving the audio track closes the microphone.
    fireEvent.click(trackHeader("Keys"));
    await waitFor(() => expect(backend.monitorInput).toHaveBeenCalledWith(false));
  });

  it("imports a dropped phone recording onto a new track and selects it", async () => {
    await dropRecording();
    expect(trackHeader("Voice Memo 3")).toBeTruthy();
    const clip = document.querySelector(".clip.audio");
    expect(clip?.textContent).toContain("Voice Memo 3");
    expect(screen.getByRole("tab", { name: "Audio · Voice Memo 3" })).toBeTruthy();
    expect(screen.getByLabelText("Clip gain")).toBeTruthy();
  });

  it("refuses files that aren't audio", async () => {
    await dropRecording(createPreviewBackend(), "C:\\Docs\\notes.txt");
    expect((await screen.findByRole("alert")).textContent).toContain("can be imported");
    expect(document.querySelector(".clip.audio")).toBeNull();
  });

  it("changes gain and normalizes", async () => {
    const backend = await dropRecording();
    const gain = screen.getByLabelText("Clip gain");
    fireEvent.change(gain, { target: { value: "-6" } });
    await waitFor(() => expect(screen.getByText("-6.0 dB")).toBeTruthy());
    // The preview's audio peaks at 0.5 (-6 dBFS): normalize to -1 dB adds 5 dB.
    await waitFor(() => expect((screen.getByText("Normalize") as HTMLButtonElement).disabled).toBe(false));
    await act(async () => {
      fireEvent.click(screen.getByText("Normalize"));
    });
    const clip = (await backend.getProject()).project.tracks[3].clips[0];
    expect(clip.audio?.gain_db).toBeCloseTo(5.0, 1);
  });

  it("splits the selected clip at the playhead", async () => {
    const backend = await dropRecording();
    await act(async () => {
      await backend.locate(1);
      await new Promise((r) => setTimeout(r, 120));
    });
    const split = screen.getByText("Split at playhead") as HTMLButtonElement;
    await waitFor(() => expect(split.disabled).toBe(false));
    await act(async () => {
      fireEvent.click(split);
    });
    const clips = (await backend.getProject()).project.tracks[3].clips;
    expect(clips.map((c) => c.start_beats)).toEqual([0, 1]);
    expect(clips[1].audio?.offset_seconds).toBeCloseTo(0.5);
  });

  it("trims a clip's start by dragging its left edge", async () => {
    const backend = await dropRecording();
    vi.spyOn(backend, "execute");
    const trim = document.querySelector(".clip.audio .clip-trim") as HTMLElement;
    await act(async () => {
      fireEvent.pointerDown(trim, { clientX: 100, clientY: 10 });
      fireEvent.pointerMove(document.querySelector(".timeline") as HTMLElement, { clientX: 124, clientY: 10 });
      fireEvent.pointerUp(document.querySelector(".timeline") as HTMLElement);
    });
    expect(backend.execute).toHaveBeenCalledWith(
      expect.objectContaining({ command: "trim_clip_start", start_beats: 1 }),
    );
  });
});

describe("Import and export", () => {
  it("offers WAV and MIDI export", async () => {
    const backend = createPreviewBackend();
    vi.spyOn(backend, "pickExportPath").mockResolvedValue("C:\\Music\\song.mid");
    vi.spyOn(backend, "exportMidi");
    await renderApp(backend);
    fireEvent.click(screen.getByRole("button", { name: "Export ▾" }));
    expect(screen.getByRole("menuitem", { name: "Song as WAV audio…" })).toBeTruthy();
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitem", { name: "MIDI file…" }));
    });
    expect(backend.pickExportPath).toHaveBeenCalledWith("mid", "Untitled");
    expect(backend.exportMidi).toHaveBeenCalledWith("C:\\Music\\song.mid");
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("shows sheet music for the selected track or all tracks", async () => {
    const backend = createPreviewBackend();
    vi.spyOn(backend, "sheetMusic");
    await renderApp(backend);
    fireEvent.click(screen.getByRole("tab", { name: "Sheet music" }));
    await waitFor(() => expect(backend.sheetMusic).toHaveBeenCalledWith([1]));
    fireEvent.click(screen.getByRole("button", { name: "All tracks" }));
    await waitFor(() => expect(backend.sheetMusic).toHaveBeenCalledWith(null));
    expect(screen.getByLabelText("Sheet music")).toBeTruthy();
  });

  it("routes dropped sheet music to the MusicXML importer", async () => {
    const backend = createPreviewBackend();
    vi.spyOn(backend, "importMusicXml").mockImplementation(() => backend.getProject());
    await renderApp(backend);
    await act(async () => {
      backend.simulateFileDrop(["C:\\Scores\\menu theme.mxl"], 10, 10);
      await new Promise((r) => setTimeout(r, 20));
    });
    expect(backend.importMusicXml).toHaveBeenCalledWith("C:\\Scores\\menu theme.mxl");
  });

  it("routes dropped MIDI files to the MIDI importer", async () => {
    const backend = createPreviewBackend();
    vi.spyOn(backend, "importMidi").mockImplementation(() => backend.getProject());
    await renderApp(backend);
    await act(async () => {
      backend.simulateFileDrop(["C:\\Downloads\\theme.mid"], 10, 10);
      await new Promise((r) => setTimeout(r, 20));
    });
    expect(backend.importMidi).toHaveBeenCalledWith("C:\\Downloads\\theme.mid");
  });
});

async function waitForElement(selector: string): Promise<Element> {
  for (let i = 0; i < 50; i++) {
    const el = document.querySelector(selector);
    if (el) return el;
    await act(async () => {
      await new Promise((r) => setTimeout(r, 10));
    });
  }
  throw new Error(`${selector} never appeared`);
}
