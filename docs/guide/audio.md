# Recording and audio

## Bring in a phone recording

1. Send the file to your PC (email, OneDrive, USB...).
2. Drag it onto the timeline, or **Import…** and pick it. Voice memos (m4a), mp3, wav, flac and ogg all work. If an empty audio track is selected, the file goes onto it; otherwise it gets a new track named after the file.
3. It lands on an audio track as a clip with its waveform.

## Record with your mic or headset

1. **+ Audio track**, and select it.
2. In the **Audio** tab, choose the **Input** device and check the **Microphone** meter moves when you speak. Aim for the loud parts reaching about three quarters of the meter, never pinned at the top.
   - Switched a headset on while the app was open? With **System default** chosen, the app moves to the new default microphone by itself within a few seconds (also when no microphone was there at all before). If the meter still doesn't move, click **⟳** beside the Input menu to look for microphones again.
3. Wear headphones, so the mic doesn't record the song.
4. **Bluetooth headset? Calibrate once** (see below), or your takes land late.
5. Press **R**. First you hear a bar of clicks (the **count-in**; the position box counts down 4, 3, 2, 1 in red). Then the song plays from the playhead: come in right after the last click. Press **R** again to stop. The take lines up with the beat you heard, and anything the mic picked up during the count-in is left out.
6. Want more time, or none? Set **Count-in** in the top bar to 2 bars or Off.

## Several takes: keep the best parts (comping)

Record the same part again over your first take: the new take plays and the old one is kept, muted (faded out on the timeline). Do it as often as you like.

1. Click **T2** (or T3...) on the audio track's header to show the takes, one per lane.
2. Move the playhead to where you want to switch takes and click **✂**: every take is cut there.
3. On each part, click **▶ Use** on the take you like best. That part plays; the others are kept, muted. Joins get short fades so they don't click.
4. Click **T…** again to fold the lanes away. **Ctrl+Z** undoes any step.

## Calibrate the recording delay (Bluetooth headsets)

Bluetooth delays both what you hear and what the mic records, typically by 100–250 ms. The app can't see that delay, so it measures it:

1. Select an audio track; in the **Audio** tab, check the right **Input** is chosen.
2. Put the headset on and click **Calibrate…**.
3. You'll hear clicks. Let **4 clicks** go by, then **clap sharply on each of the next 8**.
4. The app says what it measured (e.g. *Measured 185 ms from 8 claps*) and uses it for that microphone from now on. If your claps were uneven, it asks you to try again.

The number shows in **Recording delay**; you can also type one. It's remembered per microphone, so a wired mic keeps 0. Recalibrate if you change headsets.

Bluetooth also switches to a lower-quality "hands-free" sound while its mic is on: that's Bluetooth, not the app. Wired earbuds give the best sound and timing.

## Editing recordings

| Do this | How |
|---|---|
| Trim | Drag a clip's left or right edge |
| Cut in two | Put the playhead where you want the cut, select the clip, **Ctrl+E** |
| Louder / quieter | **Gain** in the Audio tab |
| Make it as loud as safely possible | **Normalize** (loudest moment reaches -1 dB) |
| Smooth start / end | **Fade in** / **Fade out** |
| Follow tempo changes | Tick **Follow song tempo**. Changing the tempo then speeds the clip up or slows it down without changing its pitch |

Recordings are saved in the `<Song name> Audio` folder next to your song. If you move the song without that folder, the app tells you which files are missing.

## Getting good recordings

- Quiet room, mic a hand's width from your mouth, slightly to the side (fewer pops).
- Record a few takes; keep the best, or cut the best bits together with Ctrl+E.
- Turn the metronome on while recording so you stay in time.
