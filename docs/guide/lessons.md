# Lessons

A short course. Each lesson takes 15–30 minutes and ends with something you can hear. Ask Claude "teach me lesson N": it walks you through the steps, watches what you do in the app, and answers questions as you go.

## Lesson 1: Find your way around

Goal: play the demo, change it, and undo.

1. Open `examples/Demo Groove.nptune`. Press **Space** to play, **Space** to stop, **Home** to go back.
2. Click a track, open the **Instrument** tab, and try three presets. Listen to each.
3. Change the **Tempo** to 140, then 80. Notice everything follows.
4. Mute (**M**) and solo (**S**) tracks to hear each part alone.
5. Press **Ctrl+Z** until it's back how it started.

You've learned: the screen, the transport, presets, undo.

## Lesson 2: Your first loop

Goal: a 4-bar beat with chords and bass, made by you.

1. **Ctrl+N** for a new song. Tempo 100.
2. **+ Drum track**. Double-click its row at bar 1 to add a clip, then double-click the clip to open the piano roll. Put a **Kick** on beats 1 and 3, a **Snare** on 2 and 4, and **Closed Hat** on every half beat.
3. **Ctrl+D** three times to fill 4 bars. Set a 4-bar loop on the ruler and turn on **Loop**.
4. **+ Synth track**, preset **Warm Keys**. Add a 4-bar clip and draw four chords, one per bar: C–E–G, A–C–E, F–A–C, G–B–D (the chords C, Am, F, G).
5. **+ Synth track**, preset **Sub Bass**. Draw the lowest note of each chord, an octave or two down: C, A, F, G.
6. Save it (**Ctrl+S**).

You've learned: tracks, clips, the piano roll, duplicating, looping.

## Lesson 3: Play it in

Goal: record a melody with your computer keyboard.

1. Add a **+ Synth track** with **Bright Lead** or **Chip Square**.
2. Try the keyboard: **A S D F G H J K** is C major. **Z/X** change octave.
3. Turn on the **Metronome**, press **R**, play along with your loop, press **R** to stop.
4. Open the clip, select all (**Ctrl+A**), **Quantize**. Fix or delete stray notes.
5. Not happy? **Ctrl+Z** and record again.

## Lesson 4: Record yourself

Goal: put your voice (or an instrument) on the loop.

1. **+ Audio track**, select it, check the **Microphone** meter in the Audio tab.
2. Headphones on. Bluetooth headset? Click **Calibrate…** first and clap along with the clicks.
3. **R**, perform, **R**.
4. Trim the edges, add a short **Fade in** and **Fade out**, **Normalize**.
5. Or: record on your phone, send the file to your PC, drag it onto the timeline.

## Lesson 5: Mix it

Goal: everything clear, nothing too loud.

1. **Mixer** tab. Balance faders: drums and bass first, then everything else under them.
2. Pan the keys slightly left, the lead slightly right.
3. **+ Add effect…**: Reverb on the keys, Delay on the lead, Compressor on the drums.
4. Add a **Limiter** to the master.
5. Click **A** on the keys track, automate **Volume** to fade in over the first 2 bars.
6. Ask Claude: "listen to my mix and tell me what to fix."

## Lesson 6: Into your game

Goal: your loop playing in Godot.

1. Loop region on your best 4 or 8 bars.
2. **Export ▾ → To Godot**, pick your project, tick **stems**, **Export**.
3. In Godot, put `music/<song>.ogg` on an `AudioStreamPlayer` with **Autoplay**. Run the game: it loops with no gap.
4. Bonus: use the `_layers.tres` and fade the drums stem in from code when something exciting happens.

## Lesson 7: Compose with Claude

Goal: direct Claude like a composer directs a band.

1. Describe a scene: "a calm forest village at dusk, 90 BPM, 8 bars, loops".
2. Listen, then react in plain words: "warmer", "less busy", "the melody needs to resolve at the end".
3. Ask for variations: "make a combat version with the same melody" and export both as adaptive music.
4. Ask Claude to explain what it changed and why, and try one of the changes yourself.
