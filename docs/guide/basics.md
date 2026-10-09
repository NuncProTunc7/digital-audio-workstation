# The basics

## The screen

- **Top bar**: Open, Save, Import…, Export ▾, then the transport (Play, Stop, Record, position as *Bar.Beat*, Loop, Metronome, Count-in, Tempo, Time signature), then Undo/Redo.
- **Timeline** (middle): one row per track, clips laid out left to right. The ruler on top shows bars.
- **Bottom panel** with tabs:
  - **Instrument** (or **Audio** for an audio track): the selected track's sound and settings.
  - **Piano roll**: the notes in the selected clip.
  - **Sheet music**: the selected track (or all tracks) as notation.
  - **Mixer**: volume, pan, mute/solo, and effects for every track.
- **Status bar** (bottom edge): sound card, sample rate, **Buffer** (see below), CPU load, output level, MIDI keyboards, and the **Claude** button.
- **Buffer** is how much sound the computer prepares at a time. Smaller (128, 256) answers faster when you play; bigger (1024, 2048) stops crackles on a busy computer. Each choice shows its delay in ms. **Default** lets Windows choose. If the computer struggles, a **Crackling? Use buffer …** button appears; click it to go one size up.

## Playing

| Do this | How |
|---|---|
| Play / stop | **Space**, or the ▶ and ■ buttons |
| Back to the start | **Home**, or press Stop twice |
| Jump somewhere | Click the ruler |
| Loop a section | Drag along the top strip of the ruler, then turn **Loop** on |
| Click track | **Metronome** button |
| Count in before recording | **Count-in**: Off, 1 bar (the default) or 2 bars of clicks before the song starts |
| Change speed | Type in **Tempo** (BPM) |

## Saving

- **Ctrl+S** saves; **Ctrl+Shift+S** saves under a new name. **Ctrl+O** opens, **Ctrl+N** starts a new song.
- A song is a `.nptune` file. If it has recordings, they live in a `<Song name> Audio` folder next to it. Move or back up both together.
- **Autosave:** while you have unsaved changes, the app keeps a hidden copy, updated every 30 seconds. If the app or your PC crashes, the next time you open Nunc Pro Tune it asks whether to recover them. Say **Yes**, then save.
- **Backups:** each save keeps the previous three versions beside your song as `Song.nptune.bak1` (newest) to `.bak3`. To go back to one, rename it to end in `.nptune` and open it.

## Versions

Versions are named copies of your song kept inside the song file: try something bold, and keep a way back.

- **Versions ▾** in the top bar: type a name ("Calm verse", "Darker mix") and click **Save version**.
- **Load** puts that version back in place of the song (tracks, mixer, tempo, loop). **Ctrl+Z** undoes a load.
- **A/B** compares a version with the song as it is now. Press play, then switch between **A: Song now** and **B: version** as often as you like; playback carries on. The louder of the two is turned down to match the other (the bar shows by how much), so you hear which one is *better*, not which is *louder*. **Keep B** loads the version; **Done** goes back to normal.
- Double-click a version's name to rename it; **✕** deletes it.
- Claude saves a version called **Before Claude's changes** by itself whenever it starts changing your song after a quiet spell (10 minutes), so your own work is always one click away.

## Undo

Everything, including what Claude does, can be undone: **Ctrl+Z** undo, **Ctrl+Y** (or Ctrl+Shift+Z) redo. A slider drag, a typed value, or a clip drag counts as one step. The History buttons in the top bar do the same.

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
| Ctrl+Shift+D | Duplicate it *linked*: both copies keep the same notes |
| Ctrl+E | Split the selected clip at the playhead |
| Ctrl+A | Select all notes (in the piano roll) |
| A W S E D F T G Y H U J K O L P ; ' | Play notes, like a piano (A = C, W = C#, S = D...) |
| Z / X | Octave down / up for the keys above |
| C / V | Softer / louder for the keys above |
