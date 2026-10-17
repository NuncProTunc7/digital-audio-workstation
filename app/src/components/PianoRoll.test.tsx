import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { applyCommand, defaultProject } from "../preview";
import type { Command, Project } from "../types";
import PianoRoll from "./PianoRoll";

afterEach(cleanup);

/** A guitar track with a clip holding two notes (first one bent). */
function guitarSong(): Project {
  let p = defaultProject();
  p = applyCommand(p, { command: "add_track", name: "Guitar", instrument: "guitar", preset: null, index: null });
  const track = p.tracks[p.tracks.length - 1];
  p = applyCommand(p, {
    command: "create_clip",
    track_id: track.id,
    start_beats: 0,
    length_beats: 4,
    name: null,
    notes: [
      { pitch: 64, start_beats: 0, length_beats: 1, velocity: 100, guitar: { bend_semitones: 1 } },
      { pitch: 67, start_beats: 1, length_beats: 1, velocity: 100 },
    ],
  });
  return p;
}

function renderRoll(p: Project, kind: "guitar" | "keys", onCommand: (c: Command) => Promise<unknown>) {
  const track = kind === "guitar" ? p.tracks[p.tracks.length - 1] : p.tracks[0];
  const clip = track.clips[0] ?? { id: 0, name: "", start_beats: 0, length_beats: 4, notes: [] };
  render(
    <PianoRoll
      clip={clip}
      track={track}
      drumPads={[]}
      playheadBeats={0}
      beatsPerBar={4}
      selected={new Set(clip.notes.map((n) => n.id))}
      onSelect={() => {}}
      onCommand={onCommand}
      onEndGesture={() => {}}
      onAudition={() => {}}
    />,
  );
  return clip;
}

describe("PianoRoll guitar controls", () => {
  it("sets the string and the technique of the selected notes", () => {
    const sent: Command[] = [];
    const clip = renderRoll(guitarSong(), "guitar", async (c) => sent.push(c));
    // Mixed techniques show as such; the bend is marked on its note.
    expect((screen.getByLabelText("Technique") as HTMLSelectElement).value).toBe("");
    expect(document.querySelector(".roll-note .roll-note-mark")?.textContent).toBe("↑1");
    fireEvent.change(screen.getByLabelText("String"), { target: { value: "2" } });
    const [first, second] = clip.notes;
    expect(sent.at(-1)).toEqual({
      command: "edit_notes",
      clip_id: clip.id,
      edits: [
        { id: first.id, guitar: { bend_semitones: 1, string: 2 } },
        { id: second.id, guitar: { string: 2 } },
      ],
    });
    // A technique replaces the old one and keeps the string.
    fireEvent.change(screen.getByLabelText("Technique"), { target: { value: "legato" } });
    expect(sent.at(-1)).toMatchObject({
      edits: [{ guitar: { legato: true } }, { guitar: { legato: true } }],
    });
  });

  it("only guitars get them", () => {
    renderRoll(defaultProject(), "keys", vi.fn(async () => undefined));
    expect(screen.queryByLabelText("String")).toBeNull();
  });
});
