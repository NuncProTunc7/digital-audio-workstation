import { describe, expect, it } from "vitest";
import { applyCommand, defaultProject } from "./preview";

describe("preview reducer", () => {
  it("adds, names, and removes sound effects", () => {
    let p = defaultProject();
    p = applyCommand(p, { command: "add_sound", category: "coin", seed: 3 });
    p = applyCommand(p, { command: "add_sound", category: "coin", seed: 4 });
    expect(p.sounds?.map((s) => s.name)).toEqual(["coin", "coin_2"]);
    const id = p.sounds![0].id;
    p = applyCommand(p, { command: "set_sound", sound_id: id, recipe: { ...p.sounds![0].recipe, base_freq: 5 } });
    expect(p.sounds![0].recipe.base_freq).toBe(1);
    p = applyCommand(p, { command: "remove_sound", sound_id: id });
    p = applyCommand(p, { command: "remove_sound", sound_id: p.sounds![0].id });
    expect(p.sounds).toEqual([]);
  });

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

  it("inserts, repeats, and deletes whole bars on every track", () => {
    let p = defaultProject();
    p = applyCommand(p, {
      command: "create_clip",
      track_id: 1,
      start_beats: 0,
      length_beats: 8,
      name: null,
      notes: [
        { pitch: 60, start_beats: 0, length_beats: 1 },
        { pitch: 67, start_beats: 4, length_beats: 1 },
      ],
    });
    p = applyCommand(p, { command: "add_marker", name: "Chorus", start_beats: 4 });
    const starts = () => p.tracks[0].clips.map((c) => [c.start_beats, c.length_beats, c.notes.length]);
    p = applyCommand(p, { command: "repeat_bars", from_bar: 2, bars: 1 });
    expect(starts()).toEqual([
      [0, 8, 2],
      [8, 4, 1],
    ]);
    expect(p.markers?.map((m) => [m.name, m.start_beats])).toEqual([
      ["Chorus", 4],
      ["Chorus 2", 8],
    ]);
    p = applyCommand(p, { command: "insert_bars", at_bar: 2, bars: 1 });
    expect(starts()).toEqual([
      [0, 4, 1],
      [8, 4, 1],
      [12, 4, 1],
    ]);
    p = applyCommand(p, { command: "delete_bars", from_bar: 2, bars: 2 });
    expect(starts()).toEqual([
      [0, 4, 1],
      [4, 4, 1],
    ]);
    expect(p.markers?.map((m) => m.start_beats)).toEqual([4]);
    expect(() => applyCommand(p, { command: "delete_bars", from_bar: 0, bars: 1 })).toThrow();
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
