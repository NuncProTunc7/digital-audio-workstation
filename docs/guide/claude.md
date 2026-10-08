# Working with Claude

## Connect

Keep Nunc Pro Tune open while Claude works: Claude talks to the running app, and you see every change as it happens.

- **Claude Desktop**: click **Claude** (bottom-right), then **Set up Claude Desktop**. Quit Claude Desktop completely (tray icon → Quit) and reopen it. `nunc-pro-tune` appears in its tools.
- **Claude Code**: click **Claude**, copy the command under *Claude Code*, run it once in a terminal.

The **Claude** panel lists what Claude did this session. **Ctrl+Z** undoes any of it.

## Things to ask

- "Teach me lesson 1." / "Explain what a compressor does, using my drums."
- "Make a 16-bar chiptune battle loop at 150 BPM with lead, bass and drums."
- "Add a bass line that follows the chords on the Keys track."
- "Listen to my mix and tell me what to fix."
- "Turn this photo of sheet music into tracks."
- "Export the explore and combat sections to my Godot project as adaptive music."

## Getting what you want

- **Give a reference**: a game, composer or mood ("Celeste-style, melancholy, piano and soft synths").
- **Say how it's used**: menu, town, boss fight; how long before it loops; does it need layers?
- **React in your own words**: "too busy", "the lead is annoying", "needs more energy at bar 9". Claude turns that into specific changes.
- **One change at a time** when fine-tuning, and listen after each.
- Claude can't hear the way you do. It measures loudness, balance and frequency content, but your ears decide. Say what you hear.
- **Before Claude's changes**: when Claude starts editing your song, the app first saves a version with that name (see [Versions](basics.md)). Use **A/B** to compare Claude's take with yours, and **Load** to go back.
- Ask Claude to save versions too: "save this as *Calm verse*, then try a darker version".
- **Ask for options, then choose by ear**: "give me three different melodies for bars 9–16". Claude builds each one as a version (*Option 1 – rising*, *Option 2 – calmer*…) and puts your song back as it was. Open **Versions**, click **A/B** on one option, press play, and switch between **A** (your song) and **B**; the menu next to **B** switches to the other options. **Keep B** takes the one you like (Ctrl+Z undoes).
