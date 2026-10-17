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

Your AI finds the app through a small connection note the app writes when it starts. Some AI apps keep their own out-of-date copy of that note; when the copy points at nothing, the bridge clears it and finds the open app by itself. If your AI still says the app isn't running while it is open, quit and reopen Nunc Pro Tune.

The guide your AI reads is built into the bridge (`npt-mcp`). If your AI describes buttons or shortcuts that don't match the app, its bridge is older than the app: install the latest version (or, if you build the app yourself, build the bridge again too).

## Things to ask

- "Teach me lesson 1." / "Explain what a compressor does, using my drums."
- "Make a 16-bar chiptune battle loop at 150 BPM with lead, bass and drums."
- "Add a bass line that follows the chords on the Keys track."
- "Listen to my mix and tell me what to fix."
- "Turn this photo of sheet music into tracks." (needs an AI that can read pictures)
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
