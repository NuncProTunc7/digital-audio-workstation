# Godot

## Try it like the game first: Game preview

Click **🎮 Game preview** in the top bar. The song plays the way your game will play the export:

- **Sections**: one button per section marker (or the loop region, or the whole song if there are no markers). The playing section loops. Click another one and it starts at the **next bar line**, while the old section's echoes ring out, just like the game switching from *Explore* to *Combat*. A blinking button is waiting for its bar.
- **Layers**: one button per track. Click to fade it out or back in over the **Fade** time (at once, 1–4 beats, 2 bars): try starting with pads and bass only, then bringing the drums in for combat.
- Nothing you do here changes the song or goes into undo. Close the panel to stop.

## Export

**Export ▾ → To Godot (loops, stems)…**

1. **Godot project**: browse to the folder with `project.godot` (once; it's remembered).
2. **Folder in the project**: `music` by default (`res://music/`).
3. **What**: **Loop region** (when Loop is on) or **Whole song**. Tick **stems** for one file per track as well.
   - **Intro, then loop**: set the loop region to the part that should repeat (say bars 5–12), then tick **Play from the song start, then loop the loop region**. Bars 1–4 play once as an intro; after that Godot repeats bars 5–12 forever, with no gap at the join.
4. **Format**: OGG (small, recommended) or WAV.
5. **Loudness**: leave it on game level (-16 LUFS), or **Keep the mix level**.
6. **Check** (optional, recommended): renders exactly what would be exported and lists anything wrong, in plain words: a click where the loop repeats, clipping, silence, recordings that are missing, a loop that isn't a whole number of bars (Godot can't keep it in time), stems of different lengths. ✓ is fine, ⚠ is worth a listen, ✗ will sound wrong in the game.
7. **Listen to the loop point** plays the last bar into the first bar, over and over, so you can hear the join. Press **Stop** (Space) to end.
8. **Export**.

What you get:

- Seamless loops: the reverb tail at the end is folded back onto the start, so the loop point has no click or gap. Godot loops them without any setup.
- With stems, a `*_layers.tres` (`AudioStreamSynchronized`): all stems play in sync, and your game fades layers in and out (e.g. add drums when combat starts) by changing each stream's volume.
- If the song has [buses](mixing.md), tick **One stem per bus** to get one stem per group (say *drums*, *music*, *ambience*) plus *other* for tracks that play straight into the master. Fewer, bigger layers are easier to handle in the game.
- **Sections from markers**: mark where each part of the song starts (see [section markers](writing-music.md)), then tick **Sections from markers**. Each section becomes its own seamless loop, plus a `*_sections.tres` (`AudioStreamInteractive`) that switches to another section on the next bar with a short crossfade. In Godot, call `switch_to_clip_by_name("Combat")` on the playback to change section.
- Claude can export sections too, from your markers or ones it names itself.

Using it in Godot 4.6: drag the `.ogg` (or `.tres`) onto an `AudioStreamPlayer`'s **Stream** and enable **Autoplay**, or set it from code. Re-exporting replaces the files and keeps Godot's references working.
