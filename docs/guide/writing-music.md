# Writing music

## Tracks and sounds

- Add a track with the buttons under the track list:
  - **+ Synth track**: keys, pads, leads, basses, chiptune.
  - **+ Drum track**: beats.
  - **+ Sampler track**: a sample pack, i.e. a real recorded piano, bass, strings...
  - **+ Audio track**: recordings (see [audio](audio.md)).
- Select a track, then open the **Instrument** tab to change its sound. Pick a **Preset** first, then adjust knobs.
- **Sounds** tab: every built-in sound and every preset you saved, in one list. Search ("bass", "retro", "warm"), click a mood button to filter, and star your favorites (**★ Favorites** shows only those). **Try on …** puts the sound on the selected track and plays a short phrase so you hear it (**Ctrl+Z** puts the old sound back); sounds for a different kind of instrument offer **+ New track** instead.
- **Your own presets**: when you like a sound, click **Save preset…**, name it ("Dungeon Pad") and click **Save**. It appears under **Your presets** in the Preset list of every track with the same kind of instrument, in every song. Saving with the same name updates it; **✕** next to the list deletes it (tracks using it keep their sound). Claude can save and load them too.
- Double-click a track's name to rename it. Each track header has **M** (mute), **S** (solo), **A** (automation) and a delete button.

### Built-in sounds

| Instrument | Presets | Good for |
|---|---|---|
| Synth | Warm Keys, Soft Pad, Bright Lead, Pluck, Chip Square, Brass Stab, Sub Bass, Fat Bass, Acid Bass (Init = blank) | Chords, leads, pads, basses, chiptune |
| Drums | Classic Kit, Tight Kit, Boomy Kit | Beats: 16 pads (kick, snare, hats, toms, crash, ride...) |
| Sampler | Your SFZ pack | Realistic piano, bass, orchestral |

Synth knobs, in plain terms: **Wave** is the basic tone (Sine soft, Saw bright and buzzy, Square hollow and 8-bit, Triangle mellow). **Cutoff** is brightness. **Resonance** adds a whistle at the cutoff. **Attack** is how slowly a note fades in. **Release** is how long it rings after you let go. **LFO** makes things wobble.

### Sampler (real instruments)

1. Download a free SFZ pack. For piano: *Salamander Grand Piano* (SFZ + FLAC) from freepats.zenvoid.org. Unzip it.
2. **+ Sampler track**, then in its panel **Load sample pack…** and choose the `.sfz` file.
3. Big packs load in the background (the panel shows *loading…*). To save memory the app may keep only some of the pack's soft-to-loud layers; the panel tells you how many.

## Key and chords

- **Key** (top bar): pick the song's key, e.g. **A** and **Minor**. In the piano roll, notes in the key get lighter rows (the home note, the tonic, a bit lighter still), so you can stay "in key" without knowing theory. The key is also written into MIDI and sheet-music files. Each mode has a mood: hover over it in the list.
- **Chord track**: the strip under the section markers. **Double-click** it to add a chord at that bar (it starts as the key's home chord); a menu opens where you choose the **Root**, the **type** (major, m, 7, maj7, sus4...) and an optional **Bass** note (slash chords like C/E). Click a chord to change it again, drag it to move it, **Delete chord** to remove it. Each chord lasts until the next.
- The chord track makes no sound. It guides: in the piano roll, the notes of the chord playing at that moment are marked with an orange band, a safe choice for bass lines and long melody notes.
- Claude reads the key and chords too: "write a bass line that follows the chord track" works.

## Section markers

The strip under the bar numbers holds **section markers**: named points where a part of the song starts (Intro, Explore, Combat, Boss). A section runs from its marker to the next one.

- **Double-click** the strip to add a marker at that bar; type its name and press **Enter**.
- **Drag** a marker to move it; **double-click** it to rename; hover and click **✕** to delete.
- A dashed line shows each marker across the tracks.
- Godot export can turn the sections into music that switches between them in the game (see [Godot](godot.md)).

## Clips

- **Double-click** an empty spot on an instrument track to add a clip. Double-click a clip to open it in the piano roll.
- Drag a clip to move it; drag its right edge to change its length, or its left edge to trim the start (the end stays put). Trimming the start removes notes before the new start once you let go; **Ctrl+Z** brings them back. Clips snap to beats (zoomed in) or bars (zoomed out). Use the zoom buttons to change.
- **Ctrl+D** duplicates the selected clip right after itself: the fastest way to repeat a pattern.
- **Ctrl+E** splits it at the playhead. **Delete** removes it.

## The piano roll

- Rows are pitches (piano keys on the left), columns are beats.
- **Click** an empty spot to add a note. It takes the length of the last note you touched.
- **Drag** a note to move it, drag its right end to change its length, **double-click** it to delete it.
- **Shift+click** adds notes to the selection; **Ctrl+A** selects all.
- **Grid** sets the snap (1/4, 1/8, 1/16...). **Quantize** snaps the selected notes (or all) onto the grid, tidying up a played-in part.
- **Humanize** does the opposite: it nudges the selected notes (or all) slightly off the grid and varies their loudness, so a part you clicked in sounds played. Click again for a different take; **Ctrl+Z** undoes.
- On drum tracks, each row is a drum pad instead of a pitch. Drum clips open in the step grid (below); click **Piano roll** to edit them here, **Steps** to go back.

## The drum step grid

Drum clips open as a grid: one row per drum, one column per step (a sixteenth note unless you change **Steps**). Brighter columns mark beats; a line marks each bar.

- **Click** a step to add a hit, click it again to clear it. **Drag** along a row to paint (or erase) several at once; a drag is one undo step.
- **Right-click** a step for:
  - **Velocity**: Soft, Normal, Accent. Louder hits look brighter.
  - **Roll**: ×2, ×3, ×4 quick hits inside the step, for snare rolls and hat flutters.
  - **Chance**: 75 %, 50 %, 25 % plays the hit only sometimes, so a repeating beat varies. Striped steps have a chance. The pattern of skips is fixed, so the song sounds the same each time you play or export it, while each lap of a loop differs.
- **Swing** pushes every second step late for a shuffle (try 30–60 %). Choose whether sixteenths or eighths swing. Swing is heard when playing, exporting and in MIDI files; sheet music shows the notes straight.
- Click a drum's name to hear it.

## Playing and recording notes (no MIDI keyboard needed)

- Your computer keyboard is a piano: **A** = C, **W** = C#, **S** = D, and so on along the row. **Z/X** change octave, **C/V** change loudness.
- You can also click the on-screen piano or drum pads, or plug in a MIDI keyboard (the status bar shows it).
- To record: select the track, press **R**, wait for the count-in clicks (set **Count-in** in the top bar), play from the next beat, press **R** again. Your notes become a clip. Then quantize if the timing is loose.

## Tips

- Build 4 or 8 bars that sound good on loop, then duplicate and vary them.
- Keep bass and kick simple and locked together; let one thing at a time be busy.
- Ask Claude for parts you can't play: "add a bass line that follows these chords".
