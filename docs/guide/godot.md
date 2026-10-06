# Godot

**Export ▾ → To Godot (loops, stems)…**

1. **Godot project**: browse to the folder with `project.godot` (once; it's remembered).
2. **Folder in the project**: `music` by default (`res://music/`).
3. **What**: **Loop region** (when Loop is on) or **Whole song**. Tick **stems** for one file per track as well.
4. **Format**: OGG (small, recommended) or WAV.
5. **Loudness**: leave it on game level (-16 LUFS), or **Keep the mix level**.
6. **Export**.

What you get:

- Seamless loops: the reverb tail at the end is folded back onto the start, so the loop point has no click or gap. Godot loops them without any setup.
- With stems, a `*_layers.tres` (`AudioStreamSynchronized`): all stems play in sync, and your game fades layers in and out (e.g. add drums when combat starts) by changing each stream's volume.
- Claude can also export named sections (say *explore* and *combat*) as an `AudioStreamInteractive` that switches to the other section on the next bar.

Using it in Godot 4.6: drag the `.ogg` (or `.tres`) onto an `AudioStreamPlayer`'s **Stream** and enable **Autoplay**, or set it from code. Re-exporting replaces the files and keeps Godot's references working.
