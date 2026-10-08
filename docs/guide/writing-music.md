# Writing music

## Tracks and sounds

- Add a track with the buttons under the track list:
  - **+ Synth track**: keys, pads, leads, basses, chiptune.
  - **+ Drum track**: beats.
  - **+ Sampler track**: a sample pack, i.e. a real recorded piano, bass, strings...
  - **+ Audio track**: recordings (see [audio](audio.md)).
- Select a track, then open the **Instrument** tab to change its sound. Pick a **Preset** first, then adjust knobs.
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
- On drum tracks, each row is a drum pad instead of a pitch.

## Playing and recording notes (no MIDI keyboard needed)

- Your computer keyboard is a piano: **A** = C, **W** = C#, **S** = D, and so on along the row. **Z/X** change octave, **C/V** change loudness.
- You can also click the on-screen piano or drum pads, or plug in a MIDI keyboard (the status bar shows it).
- To record: select the track, press **R**, wait for the count-in clicks (set **Count-in** in the top bar), play from the next beat, press **R** again. Your notes become a clip. Then quantize if the timing is loose.

## Tips

- Build 4 or 8 bars that sound good on loop, then duplicate and vary them.
- Keep bass and kick simple and locked together; let one thing at a time be busy.
- Ask Claude for parts you can't play: "add a bass line that follows these chords".
