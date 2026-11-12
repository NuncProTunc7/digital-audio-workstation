# Working with your AI

Nunc Pro Tune can be driven by an AI assistant: it writes parts, picks sounds, mixes, listens to the result through its measurements, and exports to Godot, while you watch every change happen. It works with any AI app that supports **MCP** (the open standard for giving AIs tools): Claude, OpenAI's Codex, or a model running on your own computer.

## Connect

Keep Nunc Pro Tune open while your AI works: it talks to the running app. Click **AI** (bottom-right), choose your app under **Which AI app?**, and follow the steps shown:

- **Claude Desktop**: click **Set up Claude Desktop**. Quit Claude Desktop completely (tray icon → Quit) and reopen it. `nunc-pro-tune` appears in its tools.
- **Claude Code**: copy the command shown and run it once in a terminal.
- **Codex (OpenAI)**: click **Set up Codex** (or copy the `codex mcp add` command and run it once in a terminal), then start a new Codex session. Type `/mcp` in Codex to check that `nunc-pro-tune` is listed.
- **Another AI app** (LM Studio, Cursor, Cline, Windsurf, goose…): in the app's MCP or tools settings, add a server that runs the program shown (the *bridge*, `npt-mcp`). Apps that take a settings file want the **MCP settings** text shown; paste it in, keeping any servers already there.
- **A small model on this computer**: use the **MCP settings for small models**. They add `--core`, which offers only the essential tools (write notes, pick sounds, mix, play, save) and sends text instead of pictures (when it checks the tone of your song, it gets a description in words, bar by bar, instead of a spectrogram picture), so a small model doesn't lose its way. Pick a model that supports *tool use* (also called *function calling*). Bigger models write better music; with a small one, ask for one change at a time.
- **An app that takes a web address** (Open WebUI, some local-model chat apps): tick **Let AI apps connect with a web address** (and **Short tool list for small models** first, for a small model). The panel shows an **address** and a **secret key**: in the app's MCP or tools settings, add a server of type *Streamable HTTP* with that address, and put the key where it asks for a *Bearer token* or *API key*. Apps with no key field take the **Address with key** instead. Only apps on this computer can use it (websites can't), and only with the key; turning the switch off and on makes a new key. It stays on until you turn it off or close Nunc Pro Tune.

ChatGPT can't connect: its tools run on OpenAI's servers, which can't reach an app on your own computer.

Once an AI has done something, the button shows its name (**AI · Codex**), and the panel lists what it did, each change labelled with the AI that made it. **Ctrl+Z** undoes any of it.

The app writes a small control file when it starts so your AI can find it. If the connection fails, open the **AI** panel and ask your AI to run `bridge_status`. It checks the file, the local connection, and whether the app and bridge speak the same protocol. If it connects, it reports the app version, protocol version, and supported features. Neither check shows the control file's secret.

- A missing file means the bridge couldn't find the control file. Open Nunc Pro Tune, or check the AI panel if it's already open.
- A stale file points at an address where no app is listening. Reopen Nunc Pro Tune and try again. Some AI apps keep their own copy of the file. An ordinary tool call can clear a stale copy and look again.
- If access is denied, use the failed step to tell whether it happened while reading the file, connecting, or signing in to the app. Check the AI panel and open both apps under the same Windows account.
- If the protocol versions are incompatible, use the bridge that came with your app and restart your AI app.

The guide your AI reads is built into the bridge (`npt-mcp`). If your AI describes buttons or shortcuts that don't match the app, its bridge is older than the app: install the latest version (or, if you build the app yourself, build the bridge again too).

## Things to ask

- "Teach me lesson 1." / "Explain what a compressor does, using my drums."
- "Make a 16-bar chiptune battle loop at 150 BPM with lead, bass and drums."
- "Add a bass line that follows the chords on the Keys track."
- "Listen to my mix and tell me what to fix."
- "Turn this photo of sheet music into tracks." (needs an AI that can read pictures)
- "Export the Boss marker as an Ogg loop into my game’s audio folder."
- "Give me a playable preview of the loop seam while the song keeps playing."
- "Save an independent copy of this song, including my unsaved changes."
- "Export the explore and combat sections to my Godot project as adaptive music."
- "Make a coin, a jump and a hit sound for my platformer, retro and a bit crunchy, then export them to my Godot project."
- "Make the jump sound softer and a little lower."
- "Make this clip louder." / "Fix these notes, they sound wrong." / "Make this sound punchier." (select it first: see below)

## Getting what you want

- **Select, then say "this"**: click a clip on the timeline, select notes in the piano roll, or pick a sound in the **Sound FX** panel, then talk about "this clip", "these notes" or "this sound". Your AI can see what you have selected (and where the playhead is), so it doesn't have to guess which one you mean. It can't see the rest of the screen, so if it picks the wrong thing, select it again and say "this one".
- **Give a reference**: a game, composer or mood ("Celeste-style, melancholy, piano and soft synths").
- **Say how it's used**: menu, town, boss fight; how long before it loops; does it need layers?
- **React in your own words**: "too busy", "the lead is annoying", "needs more energy at bar 9". Your AI turns that into specific changes.
- **One change at a time** when fine-tuning, and listen after each.
- Your AI can't hear the way you do. It measures loudness, balance and frequency content, but your ears decide. Say what you hear.
- **Before an AI's changes**: when an AI starts editing your song, the app first saves a version named after it (*Before Codex's changes*, *Before Claude's changes*; see [Versions](basics.md)). Use **A/B** to compare its take with yours, and **Load** to go back.
- Ask your AI to save versions too: "save this as *Calm verse*, then try a darker version".
- **Ask for options, then choose by ear**: "give me three different melodies for bars 9–16". Your AI builds each one as a version (*Option 1 – rising*, *Option 2 – calmer*…) and puts your song back as it was. Open **Versions**, click **A/B** on one option, press play, and switch between **A** (your song) and **B**; the menu next to **B** switches to the other options. **Keep B** takes the one you like (Ctrl+Z undoes).


## Exporting to an ordinary folder

Use `inspect_audio_export` to check the render and destination, then `export_audio` to write the file. Give a path in an existing folder with an `.ogg` or `.wav` extension. Choose a marker name, a start and end beat, or the full arrangement. When several songs share one timeline, choose the marker or beat range for the song you want. A marker runs to the next marker or the end of the last bar.

- **Loop** renders the exact selected length, folds the effect tail into the start, and checks the join. The report gives sample positions per channel. Start is inclusive and end is exclusive. Use these values or whole-file repeat in your game.
- **One shot** plays once. Set `tail_seconds` if the release, reverb, or delay should ring after the selected passage.
- **Playlist** renders a standalone track and measures its first and last 50 milliseconds to help you judge transitions between songs.

Leave `target_lufs` out to keep your mix level, or give a loudness target such as −18 LUFS. Exports are stereo at 44.1 kHz. The report gives the path, duration, loudness, and peak measured from the decoded export. An existing file is replaced only with `overwrite: true`, after the new file has encoded and verified successfully.

Use `preview_audio` for a short playable WAV or Ogg excerpt while playback continues. Give it a destination and choose the first eight bars, a region (up to 30 seconds), or the loop seam. For a seam preview it uses the enabled loop or a region you supply and places the last two seconds before the first two seconds. The report gives the measured loudness and peak.

## Working on projects and making batches

`get_song` includes the open project's ID, name, saved path (if any), unsaved state, and revision. The ID belongs to this opening of the project. Opening a file or starting a new song gives it a new ID. Use the ID with project edits, saves, and direct audio exports to protect against a switch to another song. Send `expected_revision` with an edit or save to check that the song still matches the version you read.

Use `create_project` with an unused file path to make a blank song beside the song you are working on. Use `save_project_copy` to duplicate the open song and its recordings to a new file, including unsaved edits. The app keeps one project open at a time. Use `open_project` when you want to switch to the new file. `save_project` needs a destination for an unsaved project and explicit permission to overwrite an existing file.

In `batch`, name a new track or clip with `ref` and use that name later in the batch with `track_ref` or `clip_ref`. For example, add a guitar track with `ref: "guitar"`, then load a preset, set its mixer level, and add a clip using `track_ref: "guitar"`. The batch is one undo step. If a step fails, the reply points to the failing command and the edits are discarded. A successful reply gives the created, changed, and removed IDs and the new revision. Set `include_song: true` to get the song overview as well.
