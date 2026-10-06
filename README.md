# Nunc Pro Tune

A free, open-source DAW for making game music — record real instruments, compose with built-in synths, drums, bass, and keys, read and write sheet music, and export straight into Godot. Claude Desktop and Claude Code can control every part of it through MCP.

**Status:** Phase 4 (record). Record your voice or an instrument, or bring in recordings from your phone, and mix them with the built-in instruments. Claude can build, edit, play, listen to, and export your songs through MCP. Before that: write songs on a timeline with clips, record what you play, edit notes in a piano roll, mix with volume, pan, mute/solo and seven effects, loop a region, and save/open `.nptune` project files. Built-in Keys, Bass, and Drums play from the on-screen piano, your computer keyboard, drum pads, or a MIDI keyboard. See [`docs/PLAN.md`](docs/PLAN.md) for the roadmap.

## Try it on Windows

1. Open the repository's **Actions** tab on GitHub and pick the latest green **CI** run.
2. Download the **nunc-pro-tune-windows-installer** artifact and unzip it.
3. Run the `Nunc Pro Tune_…_x64-setup.exe` installer. Windows SmartScreen may warn about an unsigned app: choose **More info → Run anyway**.

## Try the demo song

In the app, click **Open** and choose `examples/Demo Groove.nptune` from this repository (download it from GitHub first). It has chords, a bass line, and a beat on a 4-bar loop.

## Record and import audio

- **From your phone:** send the recording to your PC (email, OneDrive, USB...), then drag the file onto the timeline, or click **Import audio…**. Voice memos (m4a), mp3, wav, flac and ogg all work.
- **With your mic:** click **+ Audio track**, select it, check the **Microphone** meter moves when you speak, then press **R** (or ● Record). The song plays and you record along; press **R** again to stop. Wear headphones so the mic doesn't hear the song.
- **Editing:** drag a clip's left or right edge to trim it, **Ctrl+E** splits the selected clip at the playhead, and the Audio panel has gain, fade in/out and **Normalize**.
- Audio is saved in a `<Song name> Audio` folder next to your `.nptune` file. Keep them together when you move or back up a song.

## Connect Claude

Keep Nunc Pro Tune open while Claude works: Claude talks to the running app, and you see every change as it happens. Everything Claude does can be undone with **Ctrl+Z**.

**Claude Desktop:** click **Claude** in the app's bottom-right corner, then **Set up Claude Desktop**. Quit Claude Desktop completely (right-click its tray icon → Quit) and reopen it. `nunc-pro-tune` appears in its tools menu.

**Claude Code:** click **Claude** in the app, copy the command shown under *Claude Code*, and run it once in a terminal. It looks like:

```
claude mcp add --scope user nunc-pro-tune -- "C:\Users\you\AppData\Local\Nunc Pro Tune\npt-mcp.exe"
```

Things to ask:

- "Make a 16-bar chiptune battle loop at 150 BPM with lead, bass and drums."
- "Listen to my mix and tell me what to fix."
- "Make the bass punchier and add some reverb to the keys."
- "Export the song as a WAV to my Godot project's music folder."

## Build from source

See the Commands section of [`AGENTS.md`](AGENTS.md).

## License

GPL-3.0-or-later. See [`LICENSE`](LICENSE) and [`THIRD_PARTY.md`](THIRD_PARTY.md).
