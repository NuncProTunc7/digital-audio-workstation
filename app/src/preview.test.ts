import { describe, expect, it } from "vitest";
import { applyCommand, defaultProject } from "./preview";

describe("preview reducer", () => {
  it("applies a batch all-or-nothing", () => {
    const p = defaultProject();
    const ok = applyCommand(p, {
      command: "batch",
      commands: [
        { command: "set_tempo", bpm: 90 },
        { command: "rename_track", track_id: 1, name: "Lead" },
      ],
    });
    expect(ok.tempo_bpm).toBe(90);
    expect(ok.tracks[0].name).toBe("Lead");
    expect(() =>
      applyCommand(p, {
        command: "batch",
        commands: [
          { command: "set_tempo", bpm: 90 },
          { command: "rename_track", track_id: 999, name: "x" },
        ],
      }),
    ).toThrow();
    expect(p.tempo_bpm).toBe(120);
  });

  it("creates, edits, and deletes clips and notes", () => {
    let p = defaultProject();
    p = applyCommand(p, {
      command: "create_clip",
      track_id: 1,
      start_beats: 0,
      length_beats: 4,
      name: null,
      notes: [{ pitch: 60, start_beats: 0.1, length_beats: 1 }],
    });
    const clip = p.tracks[0].clips[0];
    expect(clip.name).toBe("Keys");
    p = applyCommand(p, {
      command: "quantize_notes",
      clip_id: clip.id,
      grid_beats: 0.5,
      strength: null,
      lengths: false,
      note_ids: null,
    });
    expect(p.tracks[0].clips[0].notes[0].start_beats).toBe(0);
    p = applyCommand(p, { command: "move_clip", clip_id: clip.id, start_beats: 8, track_id: 2 });
    expect(p.tracks[0].clips).toHaveLength(0);
    expect(p.tracks[1].clips[0].start_beats).toBe(8);
    p = applyCommand(p, { command: "delete_clip", clip_id: clip.id });
    expect(p.tracks[1].clips).toHaveLength(0);
  });

  it("does not mutate the input project", () => {
    const p = defaultProject();
    applyCommand(p, { command: "set_tempo", bpm: 90 });
    expect(p.tempo_bpm).toBe(120);
  });

  it("rejects unknown targets", () => {
    expect(() => applyCommand(defaultProject(), { command: "delete_clip", clip_id: 99 })).toThrow();
  });
});
