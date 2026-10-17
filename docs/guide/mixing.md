# Mixing

Mixing makes every part audible and the whole song sit at the right loudness.

## The Mixer tab

- **Fader**: track volume. **Pan**: left/right (double-click to center). **M** / **S**: mute / solo.
- **Type an exact value**: double-click the number under a fader (or a pan, send or effect setting), type, and press **Enter**; **Esc** cancels. Pan takes `C`, `30L` or `45R`; levels take `-3.5` (or `-inf` for silence).
- **+ Add effect…** on a track: EQ, Compressor, Reverb, Delay, Chorus, Distortion, Limiter, and under **Plugins** any VST3 effect you installed (see [Plugins](plugins.md)). Effects run top to bottom; each has an on/off switch and its own settings (a plugin effect's are in its window: click its name, then **Open plugin window**).
- The master channel affects everything. A **Limiter** there stops the song clipping.
- **Master** and **+ Bus** always stay at the right of the Mixer. With many tracks, scroll the other strips sideways with the mouse wheel or the scrollbar under them.

| Effect | Use it to |
|---|---|
| EQ | Cut mud (lows on non-bass tracks), add sparkle (highs) |
| Compressor | Even out loud and soft moments; makes drums and bass punchy |
| Reverb | Put a sound in a room or hall. Use less than you think |
| Delay | Echoes. Great on leads at 1/8 or dotted 1/8 |
| Chorus | Widen and thicken pads and keys |
| Distortion | Grit for bass, leads, drums |
| Limiter | Last on the master: catches peaks |

## Buses: groups and a shared reverb

A **bus** is an extra channel that other tracks play into. Two common uses:

- **A shared reverb**: click **+ Bus** (the first one is called *Reverb*), add a **Reverb** effect to it, and turn the reverb's mix all the way up. Then on each track tick **→ Reverb** and set how much it sends with the slider under it. A new send starts at -12 dB (a usual reverb level), and the number beside **→ Reverb** shows the level: try -18 dB for a hint of room, -6 dB for a big wash. All tracks sit in the same room, and one reverb uses less CPU than one per track.
- **A group**: add a bus called *Drums*, then set each drum track's **Out** to *Drums*. Now one fader (and one compressor, if you add one) controls all of them together.

Details:

- **Out** chooses where a track plays: the **Master**, or one bus.
- **→ Bus** sends are taken after the track's fader, so turning the track down also turns down its reverb.
- With a shared Reverb bus, don't also put a **Reverb** on each track: that stacks two rooms and can wash the mix out, and the tracks no longer share one space. The Mixer says so under a track when it happens; remove the track's own Reverb and use its **→ Reverb** send.
- Buses have a fader, pan, mute (**M**), effects, and a meter; double-click a bus's name to rename it; **✕** deletes it (its tracks go back to the master).
- Godot export can make one stem per bus (see [Godot](godot.md)).

## Sidechain: make room for the kick

A compressor can listen to a *different* track. Put one on the bass (or a pad), open it, and set **Listens to** to your drum track: every kick now pushes the bass down for a moment, so the kick punches through and the music "pumps" in time.

- Good starting point: **Threshold** -30 dB, **Ratio** 6, **Attack** 0.005 s, **Release** 0.15 s. Longer release = slower, more obvious pumping.
- It listens to the drum track's own sound, before its fader. To duck to a kick you don't want to hear, make a kick-only drum track, mute it, and point the compressor at it.

## Automation (settings that change over time)

1. Click **A** on a track to show its automation row.
2. **Automate…**: pick a setting (Volume, Pan, a filter cutoff, a reverb amount...).
3. Click in the row to add points; the setting follows the line. Drag points to move them; double-click one to delete it.

Use it for fade-ins, filter sweeps into a chorus, and reverb swells at the end of a section.

## Mix analysis: see how your mix measures up

The **Mix analysis** tab (bottom panel, after **Sounds**) shows you what your AI sees when it "listens" to the song. Choose **Whole song** or **Loop region** (when Loop is on) and click **Analyze**. It plays the song through silently (and each track on its own), so it takes a few seconds on a long song.

- **Three cards**, each with ✓ (fine), ⚠ (worth a look) or ✗ (fix it), judged against the usual targets for game music:
  - **Loudness** (LUFS): aim for **-20 to -16**. It also shows the loudest moment.
  - **Peaks**: keep them at **-1 dB or below**, or exported files may distort. A Limiter last on the master fixes this.
  - **Stereo width**: 1.00 means mono (everything in the middle); around 0.3 to 0.9 sounds wide; below 0 means parts can cancel out on a phone speaker.
- **What to fix**: the same plain-language advice your AI gets.
- **Tone**: how the sound is shared from deep **Sub** to bright **Highs**. Most music has the most in **Bass**; a tiny **Highs** bar means a dull mix.
- **Tracks**: each track's share of the mix. One bar far longer than the rest may be drowning the others out. Click a track to select it.
- **Spectrogram**: a picture of the sound. Time runs left to right (bar numbers underneath), low sounds at the bottom, high at the top; brighter means louder. Kicks are tall stripes, a bass line glows along the bottom. **In words** describes it bar by bar.

When you change the song, the results fade and say so: click **Analyze again** to update them.

## A simple mixing order

1. Set every fader so nothing is louder than the drums and bass together.
2. Pan supporting parts slightly apart (keys a little left, pad a little right). Keep kick, bass and lead centered.
3. EQ: cut lows from everything except kick and bass.
4. Add reverb and delay to taste.
5. Limiter on the master.
6. Open **Mix analysis** and click **Analyze** (or ask your AI to "listen to the mix"). Fix anything marked ⚠ or ✗.

Game music usually sits around **-16 to -20 LUFS** (a loudness measure), with peaks below **-1 dB**. The Godot export levels this for you.
