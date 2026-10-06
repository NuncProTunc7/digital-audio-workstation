# The basics

## The screen

- **Top bar**: Open, Save, Import…, Export ▾, then the transport (Play, Stop, Record, position as *Bar.Beat*, Loop, Metronome, Tempo, Time signature), then Undo/Redo.
- **Timeline** (middle): one row per track, clips laid out left to right. The ruler on top shows bars.
- **Bottom panel** with tabs:
  - **Instrument** (or **Audio** for an audio track): the selected track's sound and settings.
  - **Piano roll**: the notes in the selected clip.
  - **Sheet music**: the selected track (or all tracks) as notation.
  - **Mixer**: volume, pan, mute/solo, and effects for every track.
- **Status bar** (bottom edge): sound card, sample rate and latency, CPU load, output level, MIDI keyboards, and the **Claude** button.

## Playing

| Do this | How |
|---|---|
| Play / stop | **Space**, or the ▶ and ■ buttons |
| Back to the start | **Home**, or press Stop twice |
| Jump somewhere | Click the ruler |
| Loop a section | Drag along the top strip of the ruler, then turn **Loop** on |
| Click track | **Metronome** button |
| Change speed | Type in **Tempo** (BPM) |

## Saving

- **Ctrl+S** saves; **Ctrl+Shift+S** saves under a new name. **Ctrl+O** opens, **Ctrl+N** starts a new song.
- A song is a `.nptune` file. If it has recordings, they live in a `<Song name> Audio` folder next to it. Move or back up both together.
- **Autosave:** while you have unsaved changes, the app keeps a hidden copy, updated every 30 seconds. If the app or your PC crashes, the next time you open Nunc Pro Tune it asks whether to recover them. Say **Yes**, then save.
- **Backups:** each save keeps the previous three versions beside your song as `Song.nptune.bak1` (newest) to `.bak3`. To go back to one, rename it to end in `.nptune` and open it.

## Undo

Everything, including what Claude does, can be undone: **Ctrl+Z** undo, **Ctrl+Y** (or Ctrl+Shift+Z) redo. A slider drag or a clip drag counts as one step. The History buttons in the top bar do the same.

## All keyboard shortcuts

| Key | Does |
|---|---|
| Space | Play / stop |
| R | Record (notes on an instrument track, sound on an audio track) |
| Home | Back to the start |
| Delete / Backspace | Delete the selected clip or notes |
| Ctrl+Z / Ctrl+Y | Undo / redo |
| Ctrl+S, Ctrl+Shift+S, Ctrl+O, Ctrl+N | Save, save as, open, new |
| Ctrl+D | Duplicate the selected clip |
| Ctrl+E | Split the selected clip at the playhead |
| Ctrl+A | Select all notes (in the piano roll) |
| A W S E D F T G Y H U J K O L P ; ' | Play notes, like a piano (A = C, W = C#, S = D...) |
| Z / X | Octave down / up for the keys above |
| C / V | Softer / louder for the keys above |
