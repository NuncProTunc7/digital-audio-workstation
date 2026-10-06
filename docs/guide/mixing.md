# Mixing

Mixing makes every part audible and the whole song sit at the right loudness.

## The Mixer tab

- **Fader**: track volume. **Pan**: left/right (double-click to center). **M** / **S**: mute / solo.
- **+ Add effect…** on a track: EQ, Compressor, Reverb, Delay, Chorus, Distortion, Limiter. Effects run top to bottom; each has an on/off switch and its own settings.
- The master channel affects everything. A **Limiter** there stops the song clipping.

| Effect | Use it to |
|---|---|
| EQ | Cut mud (lows on non-bass tracks), add sparkle (highs) |
| Compressor | Even out loud and soft moments; makes drums and bass punchy |
| Reverb | Put a sound in a room or hall. Use less than you think |
| Delay | Echoes. Great on leads at 1/8 or dotted 1/8 |
| Chorus | Widen and thicken pads and keys |
| Distortion | Grit for bass, leads, drums |
| Limiter | Last on the master: catches peaks |

## Automation (settings that change over time)

1. Click **A** on a track to show its automation row.
2. **Automate…**: pick a setting (Volume, Pan, a filter cutoff, a reverb amount...).
3. Click in the row to add points; the setting follows the line. Drag points to move them; double-click one to delete it.

Use it for fade-ins, filter sweeps into a chorus, and reverb swells at the end of a section.

## A simple mixing order

1. Set every fader so nothing is louder than the drums and bass together.
2. Pan supporting parts slightly apart (keys a little left, pad a little right). Keep kick, bass and lead centered.
3. EQ: cut lows from everything except kick and bass.
4. Add reverb and delay to taste.
5. Limiter on the master.
6. Ask Claude to "listen to the mix". It measures loudness and balance and reports what's off.

Game music usually sits around **-16 to -20 LUFS** (a loudness measure), with peaks below **-1 dB**. The Godot export levels this for you.
