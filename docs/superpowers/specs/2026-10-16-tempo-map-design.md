# Tempo changes mid-song (tempo map): design

Owner request (Oct 2026): let a song change speed partway through, so an AI
(or the owner) can write a boss theme that speeds up, a calm intro before a
faster loop, or a slow-down at the end. PLAN.md item 9. The owner skipped the
design questions ("just start building"), so the calls below are the
builder's, marked as such where they matter.

## Goals

- A song has a starting tempo (today's `tempo_bpm`, still set by `set_tempo`)
  plus any number of **tempo changes**: "from beat X the tempo is Y BPM".
- A change can be **sudden** (default) or a **ramp**: from that change the
  tempo glides smoothly to the next change's tempo, arriving exactly on it.
  (Builder's call: ramps are included because "speed up as the timer runs
  out" is a staple of game music.)
- Everything that turns beats into seconds uses one shared conversion:
  playback, loop, count-in, metronome, recording, tempo-following audio
  clips, render, Godot export, analysis, MIDI and MusicXML.
- AIs add, move, change and remove changes through ordinary Commands (MCP
  tools generated from them); `get_song` shows the map.
- UI: a tempo strip on the timeline ruler, like the section-marker strip.

## Non-goals

- Tempo changes before beat 0, or at beat 0 (that is the starting tempo).
- Curved (non-linear) ramps; ramps are linear in BPM over beats.
- Meter changes (time signature stays one per song).
- Warping recorded audio to follow a tempo map automatically (see "audio").

## Anti-goals

- Old projects must load and sound exactly as before (no map = one tempo).
- No allocation, locking or I/O on the audio thread.
- Undo must stay exact and ids stable.

## Data shape (kept simple on purpose; other sessions will shift it)

```rust
// daw-model Project
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub tempo_changes: Vec<TempoChange>,   // sorted by beats, all > 0, no two on the same beat

pub struct TempoChange {
    pub id: Id,          // from Project::next_id, like markers
    pub beats: f64,      // where it takes effect, beats from song start (> 0)
    pub bpm: f64,        // 20..=999
    #[serde(default, skip_serializing_if = "is_false")]
    pub ramp: bool,      // glide from here to the next change's bpm
}
```

The tempo of beat `b` is the last change at or before `b` (or `tempo_bpm`).
A ramp with no later change is steady. Saved versions (`SongState`) keep the
changes too. Commands that shift time (bar insert/delete from the
arrangement work) should move `tempo_changes[i].beats` like marker starts.

`daw_model::TempoMap` (built from the project, `Project::tempo_map()`) is the
one conversion: `seconds_at(beats)`, `beats_at(seconds)`, `bpm_at(beats)`,
`next_change_after(beats)`, `is_steady()`. Before beat 0 the starting tempo
extends backwards (count-ins). It holds precomputed segments, so queries are
a binary search and never allocate: the engine uses the same type on the
audio thread.

## Commands

- `AddTempoChange { beats, bpm, ramp }`, inverse `RemoveTempoChange`.
- `MoveTempoChange { tempo_change_id, beats }` (coalesces while dragging).
- `SetTempoChange { tempo_change_id, bpm, ramp }` (coalesces while dragging).
- `RemoveTempoChange { tempo_change_id }`, inverse
  `RestoreTempoChange { change }` (keeps the id).
- `set_tempo` keeps setting only the starting tempo.

Errors: beats not > 0 or past `MAX_BEATS`; another change already on that
beat; bpm out of range; unknown id. Each message says what to do instead
(e.g. "use set_tempo to change the starting tempo").

## Engine

The processor keeps its beat-domain design and gets a `Box<TempoMap>`
(message `SetTempoMap`, replacing `SetTempo`; the old map goes back as
garbage). Each render segment has one beats-per-sample rate:

- segments stop at the next tempo change, like they stop at the loop end;
- inside a ramp, segments are at most 64 frames and the segment's end beat
  is computed exactly from the map, so ramps never drift;
- a count-in clicks at the tempo where the song will start;
- tempo-synced effects, instruments and plugins get `set_tempo` when the
  tempo they hear changes; the status clock publishes the current tempo.

Audio clips are placed in **seconds** on the audio thread (start/end and
fades in song seconds), so audio that keeps its own speed plays at its own
speed across tempo changes. A clip that follows the tempo (`source_bpm`) is
cut into one piece per tempo stretch, each stretched to fill its beats
exactly (a ramp stretch uses its average speed; pieces stay locked to the
beat at every change). Frozen tracks play from second 0.

## Elsewhere

- **Recording**: takes are placed with the map; audio captured during the
  count-in is measured at the count-in tempo.
- **Offline render / analysis**: lengths and bar positions come from the map.
- **Godot export** (builder's call, documented in `docs/godot-formats.md`): a
  seamless loop is still the region in beats; the file is exactly that long
  in seconds, and when the tempo changes inside it the loop restarts at the
  region's starting tempo. Godot's `bpm` is written as the region's average
  tempo (beats × 60 / seconds), so `beat_count × 60 / bpm` still equals the
  file length.
- **MIDI**: export writes a tempo event per change (ramps as small steps,
  every sixteenth); import reads all tempo events into changes (an empty
  song takes them, like the starting tempo today).
- **MusicXML**: export writes a metronome mark and `<sound tempo>` at each
  change, with "accel." / "rit." words at ramps; import reads every
  `<sound tempo>` with its position.
- **get_song / summary**: `tempo_changes` with id, beats, bar, bpm, ramp.
- **Freeze fingerprint** includes the changes, so a frozen track plays live
  after the map changes.

## UI

- `app/src/tempo.ts`: the same conversion in TypeScript.
- A **Tempo** strip under the section markers: double-click to add a change
  (starts at the tempo already playing there), click the label to type a
  BPM, drag to move, a ramp toggle, and ×  to delete. Each edit is one
  Command; drags end the gesture.
- Audio clip drawing and the preview backend use the TS conversion.

## Docs

`docs/guide/writing-music.md` (how), `docs/guide/composing.md` (when, for game
music), a lesson tip, PLAN.md item 9 marked built.

## Left to the builder

Exact styling, test names, the strip's height, how many MIDI ramp steps.
