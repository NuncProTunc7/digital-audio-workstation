import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { createPreviewBackend } from "./backend";
import type { Backend } from "./backend";

afterEach(() => {
  cleanup();
  // Each test starts without remembered settings (like the Godot folder).
  window.localStorage.clear();
});

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

  it("humanizes the notes in a clip", async () => {
    const backend = await renderApp(spyBackend());
    await act(async () => {
      fireEvent.doubleClick(document.querySelectorAll(".lane")[0], { clientX: 10, clientY: 10 });
    });
    const grid = await waitForElement(".roll-grid");
    await act(async () => {
      fireEvent.pointerDown(grid, { clientX: 5, clientY: 5, button: 0 });
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Humanize" }));
    });
    expect(backend.execute).toHaveBeenCalledWith(
      expect.objectContaining({ command: "humanize_notes", velocity: 10, note_ids: null }),
    );
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

  it("sets the recording delay and explains calibration results", async () => {
    const backend = createPreviewBackend();
    vi.spyOn(backend, "setRecordingOffset");
    vi.spyOn(backend, "calibrateRecording").mockResolvedValue({
      offset_ms: 185,
      claps: 8,
      spread_ms: 7,
      saved: true,
      device: "Preview microphone",
    });
    await renderApp(backend);
    await act(async () => {
      fireEvent.click(screen.getByText("+ Audio track"));
    });
    fireEvent.click(trackHeader("Audio"));
    const field = (await screen.findByLabelText("Recording delay")) as HTMLInputElement;
    await act(async () => {
      fireEvent.change(field, { target: { value: "120" } });
    });
    expect(backend.setRecordingOffset).toHaveBeenCalledWith(120);
    await waitFor(() => expect(field.value).toBe("120"));
    await act(async () => {
      fireEvent.click(screen.getByText("Calibrate…"));
    });
    expect(await screen.findByText(/Measured 185 ms from 8 claps/)).toBeTruthy();
  });

  it("offers to recover unsaved work after a crash, once", async () => {
    const backend = createPreviewBackend();
    vi.spyOn(backend, "recoveryCheck").mockResolvedValue({
      name: "Boss Theme",
      project_path: null,
      saved_at_ms: Date.now(),
    });
    vi.spyOn(backend, "confirm").mockResolvedValue(true);
    vi.spyOn(backend, "recoveryResolve");
    await renderApp(backend);
    await waitFor(() => expect(backend.recoveryResolve).toHaveBeenCalledWith(true));
    expect(backend.confirm).toHaveBeenCalledTimes(1);
    expect(vi.mocked(backend.confirm).mock.calls[0][0]).toContain("Boss Theme");
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

  it("lets a clip follow the song tempo", async () => {
    const backend = await dropRecording();
    await act(async () => {
      fireEvent.click(screen.getByLabelText("Follow song tempo"));
    });
    const clip = (await backend.getProject()).project.tracks[3].clips[0];
    expect(clip.audio?.source_bpm).toBe(120);
    expect(screen.getByText("(recorded at 120 BPM)")).toBeTruthy();
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

describe("Note clips", () => {
  it("trims a note clip's start once, when the drag ends", async () => {
    const backend = createPreviewBackend();
    await backend.execute({
      command: "create_clip",
      track_id: 1,
      start_beats: 0,
      length_beats: 8,
      name: "Riff",
      notes: [
        { pitch: 60, start_beats: 0, length_beats: 1, velocity: 100 },
        { pitch: 64, start_beats: 3, length_beats: 1, velocity: 100 },
      ],
    });
    await renderApp(backend);
    vi.spyOn(backend, "execute");
    const trim = document.querySelector(".clip:not(.audio) .clip-trim") as HTMLElement;
    const timeline = document.querySelector(".timeline") as HTMLElement;
    const ppb = parseFloat((document.querySelector(".clip:not(.audio)") as HTMLElement).style.width) / 8;
    await act(async () => {
      fireEvent.pointerDown(trim, { clientX: 100, clientY: 10 });
      // Past the first note's start and back: the note must survive.
      fireEvent.pointerMove(timeline, { clientX: 100 + 2 * ppb, clientY: 10 });
      fireEvent.pointerMove(timeline, { clientX: 100 + 1 * ppb, clientY: 10 });
    });
    expect(backend.execute).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.pointerUp(timeline);
    });
    expect(backend.execute).toHaveBeenCalledTimes(1);
    expect(backend.execute).toHaveBeenCalledWith(
      expect.objectContaining({ command: "trim_clip_start", start_beats: 1 }),
    );
    const clip = (await backend.getProject()).project.tracks[0].clips[0];
    expect(clip.start_beats).toBe(1);
    expect(clip.length_beats).toBe(7);
    expect(clip.notes.map((n) => n.start_beats)).toEqual([2]);
  });
});

describe("Drum step sequencer", () => {
  /** Makes a one-bar drum clip and opens it; returns the backend. */
  async function openDrumClip() {
    const backend = await renderApp(spyBackend());
    await act(async () => {
      fireEvent.doubleClick(document.querySelectorAll(".lane")[2], { clientX: 10, clientY: 10 });
    });
    await screen.findByRole("row", { name: "Kick" });
    return backend;
  }
  const cell = (name: string, step: number) => screen.getByRole("gridcell", { name: `${name} step ${step}` });
  const drumNotes = async (backend: Backend) => (await backend.getProject()).project.tracks[2].clips[0].notes;

  it("opens drum clips as a grid and adds a hit with a click", async () => {
    const backend = await openDrumClip();
    expect(screen.getAllByRole("gridcell", { name: /^Kick step/ }).length).toBe(16);
    await act(async () => {
      fireEvent.pointerDown(cell("Kick", 1), { button: 0 });
      fireEvent.pointerUp(cell("Kick", 1));
    });
    expect(await drumNotes(backend)).toMatchObject([{ pitch: 36, start_beats: 0, length_beats: 0.25 }]);
    expect(cell("Kick", 1).getAttribute("aria-pressed")).toBe("true");
    // Clicking it again clears it.
    await act(async () => {
      fireEvent.pointerDown(cell("Kick", 1), { button: 0 });
      fireEvent.pointerUp(cell("Kick", 1));
    });
    expect(await drumNotes(backend)).toEqual([]);
  });

  it("paints a row of hats with one drag, as one undo step", async () => {
    const backend = await openDrumClip();
    vi.mocked(backend.execute).mockClear();
    await act(async () => {
      fireEvent.pointerDown(cell("Closed Hat", 1), { button: 0 });
      for (const s of [2, 3, 4]) fireEvent.pointerMove(cell("Closed Hat", s));
      fireEvent.pointerUp(cell("Closed Hat", 4));
    });
    expect(backend.execute).toHaveBeenCalledTimes(1);
    expect((await drumNotes(backend)).map((n) => n.start_beats)).toEqual([0, 0.25, 0.5, 0.75]);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    });
    expect(await drumNotes(backend)).toEqual([]);
  });

  it("sets accents, rolls and chance from a step's menu", async () => {
    const backend = await openDrumClip();
    fireEvent.contextMenu(cell("Snare", 5), { clientX: 50, clientY: 50 });
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitem", { name: "Roll 3" }));
    });
    let notes = await drumNotes(backend);
    expect(notes.map((n) => n.start_beats)).toEqual([1, 1 + 0.25 / 3, 1 + 0.5 / 3]);
    fireEvent.contextMenu(cell("Snare", 5), { clientX: 50, clientY: 50 });
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitem", { name: "Accent" }));
    });
    fireEvent.contextMenu(cell("Snare", 5), { clientX: 50, clientY: 50 });
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitem", { name: "Chance 50%" }));
    });
    notes = await drumNotes(backend);
    // Still a roll of three, now accented and played half the time.
    expect(notes.length).toBe(3);
    expect(notes.every((n) => n.velocity === 127 && n.chance === 50)).toBe(true);
  });

  it("swings the clip and switches to the piano roll and back", async () => {
    const backend = await openDrumClip();
    await act(async () => {
      fireEvent.change(screen.getByLabelText("Swing amount"), { target: { value: "60" } });
    });
    expect((await backend.getProject()).project.tracks[2].clips[0].swing).toEqual({
      amount_percent: 60,
      grid_beats: 0.25,
    });
    fireEvent.click(screen.getByRole("button", { name: "Piano roll" }));
    expect(document.querySelector(".roll-grid")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Steps" }));
    expect(screen.getByRole("row", { name: "Kick" })).toBeTruthy();
  });
});

describe("Section markers", () => {
  it("adds, names, moves and deletes markers, and exports their sections", async () => {
    const backend = createPreviewBackend();
    vi.spyOn(backend, "exportGodot");
    await renderApp(backend);
    const strip = screen.getByRole("group", { name: "Section markers" });
    const ppb = parseFloat((document.querySelector(".ruler") as HTMLElement).style.width) / 128; // 32 empty bars
    // Double-click on bar 3 (beat 8; the bounding box is at 0 in tests).
    await act(async () => {
      fireEvent.doubleClick(strip, { clientX: 8.5 * ppb });
    });
    const name = await screen.findByLabelText("Marker name");
    fireEvent.change(name, { target: { value: "Combat" } });
    await act(async () => {
      fireEvent.blur(name);
    });
    let markers = (await backend.getProject()).project.markers ?? [];
    expect(markers).toMatchObject([{ name: "Combat", start_beats: 8 }]);
    // Drag it one bar later.
    const marker = screen.getByTitle(/^Combat:/);
    const timeline = document.querySelector(".timeline") as HTMLElement;
    await act(async () => {
      fireEvent.pointerDown(marker, { button: 0, clientX: 100 });
      fireEvent.pointerMove(timeline, { clientX: 100 + 4 * ppb });
      fireEvent.pointerUp(timeline);
    });
    markers = (await backend.getProject()).project.markers ?? [];
    expect(markers[0].start_beats).toBe(12);
    // A second marker at the start, then export sections to Godot.
    await act(async () => {
      fireEvent.doubleClick(strip, { clientX: 0.5 * ppb });
    });
    await act(async () => {
      fireEvent.blur(await screen.findByLabelText("Marker name"));
    });
    fireEvent.click(screen.getByRole("button", { name: "Export ▾" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "To Godot (loops, stems)…" }));
    expect((screen.getByLabelText(/Sections from markers \(Section 2, Combat\)/) as HTMLInputElement).checked).toBe(true);
    fireEvent.change(screen.getByLabelText("Godot project folder"), { target: { value: "C:\\Games\\MyGame" } });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Export" }));
    });
    expect(backend.exportGodot).toHaveBeenCalledWith(expect.objectContaining({ sections_from_markers: true }));
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    // Delete one; undo brings it back.
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Delete marker Combat" }));
    });
    expect((await backend.getProject()).project.markers?.map((m) => m.name)).toEqual(["Section 2"]);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    });
    expect((await backend.getProject()).project.markers?.map((m) => m.name)).toEqual(["Section 2", "Combat"]);
  });
});

describe("Buses", () => {
  it("groups a track into a bus and sends another to it", async () => {
    const backend = createPreviewBackend();
    await renderApp(backend);
    fireEvent.click(screen.getByRole("tab", { name: /Mixer/ }));
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "+ Bus" }));
    });
    const bus = (await backend.getProject()).project.buses?.[0];
    expect(bus?.name).toBe("Reverb");
    expect(screen.getByRole("region", { name: "Reverb channel" })).toBeTruthy();
    // Drums play into the bus; Keys send some of their sound to it.
    await act(async () => {
      fireEvent.change(screen.getByLabelText("Drums output"), { target: { value: String(bus?.id) } });
    });
    await act(async () => {
      fireEvent.click(screen.getByLabelText("Send Keys to Reverb"));
    });
    await act(async () => {
      fireEvent.change(screen.getByLabelText("Keys send to Reverb"), { target: { value: "-12" } });
    });
    let p = (await backend.getProject()).project;
    expect(p.tracks[2].output).toBe(bus?.id);
    expect(p.tracks[0].sends).toEqual([{ bus_id: bus?.id, level_db: -12, pre_fader: false }]);
    expect(screen.getByRole("region", { name: "Reverb channel" }).textContent).toContain("Drums");
    // Deleting the bus sends the drums back to the master.
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Delete bus Reverb" }));
    });
    p = (await backend.getProject()).project;
    expect(p.buses).toEqual([]);
    expect(p.tracks[2].output).toBeNull();
  });
});

describe("Sidechain", () => {
  it("makes the bass compressor listen to the drums", async () => {
    const backend = createPreviewBackend();
    await renderApp(backend);
    fireEvent.click(screen.getByRole("tab", { name: /Mixer/ }));
    await act(async () => {
      fireEvent.change(screen.getByLabelText("Add effect to Bass"), { target: { value: "compressor" } });
    });
    const bass = screen.getByRole("region", { name: "Bass channel" });
    fireEvent.click(within(bass).getByRole("button", { name: /▸ Compressor/ }));
    await act(async () => {
      fireEvent.change(screen.getByLabelText("Compressor listens to"), { target: { value: "3" } });
    });
    const comp = (await backend.getProject()).project.tracks[1].mixer.effects[0];
    expect(comp.sidechain).toBe(3);
    expect((screen.getByLabelText("Compressor listens to") as HTMLSelectElement).value).toBe("3");
  });
});

describe("Your presets", () => {
  it("saves a sound and loads it on another track", async () => {
    const backend = await renderApp();
    // Keys is selected; save its sound.
    fireEvent.click(screen.getByRole("button", { name: "Save preset…" }));
    fireEvent.change(screen.getByLabelText("New preset name"), { target: { value: "Glass Keys" } });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save this preset" }));
    });
    expect(await screen.findByText('Saved preset "Glass Keys"')).toBeTruthy();
    // Select the Bass track and pick it from "Your presets".
    await act(async () => {
      fireEvent.click(trackHeader("Bass"));
    });
    const select = screen.getByLabelText("Preset") as HTMLSelectElement;
    expect([...select.querySelectorAll("optgroup[label='Your presets'] option")].map((o) => o.textContent)).toEqual([
      "Glass Keys",
    ]);
    await act(async () => {
      fireEvent.change(select, { target: { value: "user:Glass Keys" } });
    });
    const bass = (await backend.getProject()).project.tracks[1].instrument;
    expect(bass.preset).toBe("Glass Keys");
    await waitFor(() => expect((screen.getByLabelText("Preset") as HTMLSelectElement).value).toBe("user:Glass Keys"));
    // Delete it: the list empties, the track keeps its sound.
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Delete preset Glass Keys" }));
    });
    expect(await backend.userPresets()).toEqual([]);
  });
});

describe("Game preview", () => {
  it("switches sections and fades layers like the game will", async () => {
    const backend = createPreviewBackend();
    await backend.execute({ command: "add_marker", name: "Explore", start_beats: 0 });
    await backend.execute({ command: "add_marker", name: "Combat", start_beats: 8 });
    vi.spyOn(backend, "previewStart");
    vi.spyOn(backend, "previewSwitch");
    vi.spyOn(backend, "previewLayer");
    vi.spyOn(backend, "previewStop");
    await renderApp(backend);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Game preview/ }));
    });
    expect(backend.previewStart).toHaveBeenCalledWith(0);
    const panel = await screen.findByRole("dialog", { name: "Game preview" });
    expect(panel.textContent).toContain("Explore");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Combat" }));
    });
    expect(backend.previewSwitch).toHaveBeenCalledWith(1);
    fireEvent.change(screen.getByLabelText("Layer fade length"), { target: { value: "4" } });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Drums" }));
    });
    expect(backend.previewLayer).toHaveBeenCalledWith(3, false, 4);
    expect(screen.getByRole("button", { name: "Drums" }).getAttribute("aria-pressed")).toBe("false");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Close game preview" }));
    });
    expect(backend.previewStop).toHaveBeenCalled();
    expect(screen.queryByRole("dialog", { name: "Game preview" })).toBeNull();
  });
});

describe("Versions", () => {
  async function saveVersion(name: string) {
    fireEvent.click(screen.getByRole("button", { name: /^Versions/ }));
    fireEvent.change(screen.getByLabelText("New version name"), { target: { value: name } });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save version" }));
    });
  }

  it("saves a version and loads it back after changes", async () => {
    const backend = await renderApp();
    await saveVersion("Slow");
    expect(screen.getByRole("button", { name: /^Versions \(1\)/ })).toBeTruthy();
    const tempo = screen.getByLabelText("Tempo in BPM");
    fireEvent.change(tempo, { target: { value: "150" } });
    fireEvent.blur(tempo);
    await screen.findByDisplayValue("150");
    // The menu is still open; load the version.
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Load" }));
    });
    expect(await screen.findByDisplayValue("120")).toBeTruthy();
    expect((await backend.getProject()).project.snapshots?.map((s) => s.name)).toEqual(["Slow"]);
    // Loading is undoable.
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(await screen.findByDisplayValue("150")).toBeTruthy();
  });

  it("compares a version A/B and keeps it", async () => {
    const backend = createPreviewBackend();
    vi.spyOn(backend, "compareListen");
    vi.spyOn(backend, "compareStop");
    await renderApp(backend);
    await saveVersion("Darker mix");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "A/B" }));
    });
    const bar = await screen.findByRole("group", { name: "Compare versions" });
    expect(bar.textContent).toContain("turned down 2.0 dB");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /^B: Darker mix/ }));
    });
    expect(backend.compareListen).toHaveBeenCalledWith("version");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Done" }));
    });
    expect(backend.compareStop).toHaveBeenCalled();
    await waitFor(() => expect(screen.queryByRole("group", { name: "Compare versions" })).toBeNull());
  });
});

describe("Sound card buffer", () => {
  it("changes the buffer size from the status bar", async () => {
    const backend = createPreviewBackend();
    vi.spyOn(backend, "setBufferSize");
    await renderApp(backend);
    const select = (await screen.findByLabelText("Sound card buffer")) as HTMLSelectElement;
    expect(select.value).toBe("default");
    expect(screen.getByRole("option", { name: "256 (5.3 ms)" })).toBeTruthy();
    await act(async () => {
      fireEvent.change(select, { target: { value: "1024" } });
    });
    expect(backend.setBufferSize).toHaveBeenCalledWith(1024);
    await waitFor(() => expect(select.value).toBe("1024"));
  });

  it("suggests a bigger buffer when the computer struggles", async () => {
    const backend = createPreviewBackend();
    const status = backend.transportStatus.bind(backend);
    vi.spyOn(backend, "transportStatus").mockImplementation(async () => ({
      ...(await status()),
      buffer_frames: 256,
      struggling: true,
    }));
    vi.spyOn(backend, "setBufferSize");
    await renderApp(backend);
    const hint = await screen.findByRole("button", { name: "Crackling? Use buffer 512" });
    await act(async () => {
      fireEvent.click(hint);
    });
    expect(backend.setBufferSize).toHaveBeenCalledWith(512);
  });
});

describe("Diagnostic report", () => {
  it("copies a report that includes errors the user saw", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const backend = createPreviewBackend();
    vi.spyOn(backend, "exportMidi").mockRejectedValue(new Error("the disk is full"));
    vi.spyOn(backend, "pickExportPath").mockResolvedValue("C:\\Music\\song.mid");
    await renderApp(backend);
    fireEvent.click(screen.getByRole("button", { name: "Export ▾" }));
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitem", { name: "MIDI file…" }));
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy diagnostic report" }));
    });
    expect(writeText).toHaveBeenCalledTimes(1);
    const report = writeText.mock.calls[0][0] as string;
    expect(report).toContain("Nunc Pro Tune diagnostic report");
    expect(report).toContain("the disk is full");
    expect(await screen.findByText(/Diagnostic report copied/)).toBeTruthy();
  });
});

describe("Count-in", () => {
  it("chooses how many bars to count in before recording", async () => {
    const backend = createPreviewBackend();
    vi.spyOn(backend, "setCountIn");
    await renderApp(backend);
    const select = screen.getByLabelText("Count-in before recording") as HTMLSelectElement;
    await waitFor(() => expect(select.value).toBe("1"));
    await act(async () => {
      fireEvent.change(select, { target: { value: "2" } });
    });
    expect(backend.setCountIn).toHaveBeenCalledWith(2);
    await waitFor(() => expect(select.value).toBe("2"));
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

  it("exports loops and stems into a Godot project", async () => {
    const backend = createPreviewBackend();
    vi.spyOn(backend, "exportGodot");
    await renderApp(backend);
    fireEvent.click(screen.getByRole("button", { name: "Export ▾" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "To Godot (loops, stems)…" }));
    const dialog = screen.getByRole("dialog", { name: "Export to Godot" });
    expect((screen.getByRole("button", { name: "Export" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.change(screen.getByLabelText("Godot project folder"), { target: { value: "C:\\Games\\MyGame" } });
    fireEvent.click(screen.getByLabelText(/each track as a stem/));
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Export" }));
    });
    expect(backend.exportGodot).toHaveBeenCalledWith(
      expect.objectContaining({ project_dir: "C:\\Games\\MyGame", folder: "music", name: "untitled", stems: true, looped: true, target_lufs: -16 }),
    );
    expect(dialog.textContent).toContain("res://music/untitled.ogg");
    expect(dialog.textContent).toContain("res://music/untitled_layers.tres");
  });

  it("exports an intro that plays once before the loop", async () => {
    const backend = createPreviewBackend();
    vi.spyOn(backend, "exportGodot");
    await backend.execute({ command: "set_loop", enabled: true, start_beats: 4, end_beats: 12 });
    await renderApp(backend);
    fireEvent.click(screen.getByRole("button", { name: "Export ▾" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "To Godot (loops, stems)…" }));
    fireEvent.change(screen.getByLabelText("Godot project folder"), { target: { value: "C:\\Games\\MyGame" } });
    const intro = screen.getByLabelText(/Play from the song start/) as HTMLInputElement;
    expect(intro.disabled).toBe(false);
    fireEvent.click(intro);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Export" }));
    });
    expect(backend.exportGodot).toHaveBeenCalledWith(
      expect.objectContaining({ start_beats: 4, end_beats: 12, looped: true, intro: true }),
    );
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

describe("Automation", () => {
  it("adds a volume lane and draws points on it", async () => {
    const backend = createPreviewBackend();
    vi.spyOn(backend, "execute");
    await renderApp(backend);
    fireEvent.click(screen.getByRole("button", { name: "Automation for Keys" }));
    const picker = screen.getByLabelText("Automation lane for Keys") as HTMLSelectElement;
    expect([...picker.options].map((o) => o.textContent)).toContain("+ Filter: Cutoff");
    await act(async () => {
      fireEvent.change(picker, { target: { value: 'new:{"kind":"volume"}' } });
    });
    expect(backend.execute).toHaveBeenCalledWith(
      expect.objectContaining({ command: "add_automation_lane", track_id: 1, target: { kind: "volume" } }),
    );
    const lane = await screen.findByLabelText("Volume automation");
    // jsdom has no layout: x is beats * 24 px, y = 0 is the top (+6 dB).
    await act(async () => {
      fireEvent.pointerDown(lane, { clientX: 96, clientY: 0, button: 0 });
      fireEvent.pointerUp(lane);
    });
    const keys = (await backend.getProject()).project.tracks[0];
    expect(keys.automation?.[0].points).toEqual([{ beats: 4, value: 6 }]);
    // Double-click removes it.
    const point = document.querySelector(".automation-point") as Element;
    await act(async () => {
      fireEvent.doubleClick(point);
    });
    expect((await backend.getProject()).project.tracks[0].automation?.[0].points).toEqual([]);
  });
});

describe("Sampler", () => {
  it("adds a sampler track and loads a sample pack", async () => {
    const backend = createPreviewBackend();
    vi.spyOn(backend, "pickSamplePack").mockResolvedValue("D:\\Samples\\Salamander\\SalamanderGrandPiano.sfz");
    await renderApp(backend);
    await act(async () => {
      fireEvent.click(screen.getByText("+ Sampler track"));
    });
    fireEvent.click(trackHeader("Piano"));
    expect(await screen.findByText("No sample pack loaded yet: this track is silent.")).toBeTruthy();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Load sample pack…" }));
    });
    const piano = (await backend.getProject()).project.tracks[3];
    expect(piano.instrument.sample_pack).toBe("D:\\Samples\\Salamander\\SalamanderGrandPiano.sfz");
    expect(await screen.findByText("SalamanderGrandPiano.sfz")).toBeTruthy();
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
