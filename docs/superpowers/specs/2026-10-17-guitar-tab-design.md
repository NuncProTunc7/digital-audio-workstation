# Guitar tab view: design

Owner request (2026-10-15): "tablature (guitar tab) view, like the sheet-music view". The owner skipped the design questions; every choice below is the builder's call.

## What it is

Guitar tracks show standard guitar notation in the **Sheet music** tab: a notes staff (treble clef with the small 8 below, as guitar music is written) with a six-line **tab** staff under it, fret numbers on the strings, chord names above. A switch in the toolbar (shown when a guitar track is on screen) picks **Notes + tab**, **Tab**, or **Notes**. Exported MusicXML carries the tab too (as `<string>`/`<fret>` on a TAB staff with the tuning and capo), so MuseScore and Guitar Pro open it as guitar tab.

It's a view: editing frets in the tab, text tab export, slides/bends and choosing the string per note are later items.

## Which string each note is on

The tab must show what the guitar actually plays. The guitar's rule (each note on the highest free string that reaches it, a repeated note on the string already playing it, else the highest that reaches it, cutting it) moves into one pure function in `daw-model` that the engine's guitar and the tab both call, so they can't drift apart. The tab replays the track's notes in the engine's order (as the sequencer sorts them: by time, offs before ons) to know which strings are busy. Fret = note − open string (with the capo, frets count from the capo, as in tab). Notes the guitar can't reach in its tuning stay on the notes staff and are left off the tab.

## Parts

- `daw-model`: `guitar_string_for(note, open, uncapoed_low, busy)`; the engine's `pick_string` calls it.
- `daw-notation`: `guitar_frets(track)` (the replay); the MusicXML writer gives guitar tracks two staves (or one, per the switch), writes `<harmony>` chord names on the top part, and the reader skips notes on a TAB staff (no doubled notes when a score comes back in).
- `daw-control` / Tauri `sheet_music`: an optional `guitar` view choice (`both`, `tab`, `notes`; default both).
- UI: the switch in the Sheet music toolbar; the preview backend draws a small stand-in.

## Not now

Editing in the tab, string choice per note, slides, bends, hammer-ons, text (ASCII) tab, tab for imported Guitar Pro files.
