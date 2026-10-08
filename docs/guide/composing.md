# Composing playbook

How Claude writes music in Nunc Pro Tune. The user can read it too: it explains what Claude does and why.

## Workflow

1. **Brief.** Know (or ask, briefly) the use: menu, exploration, combat, boss, victory, ambience. The mood, references, tempo, loop length, and whether layers or sections are needed. If the user gives little, choose sensible defaults and say what they are.
2. **Read first.** `get_song` (and `describe_instruments` before changing sounds). Don't overwrite the user's work: add tracks, or ask. Never `new_project` without asking.
3. **Build in one undo step per idea**, with `batch`: e.g. add a track, load a preset, create a clip, add notes.
4. **Order**: tempo and time signature → drums → bass → harmony → melody → texture (pads, arps, counter-melody) → mix → automation.
5. **Check.** `analyze_mix` (with `spectrogram` when judging tone, `per_track` for balance). Fix clipping and masking.
6. **Play it for the user** (`play`, with the loop set) and ask how it sounds. Change one thing at a time from their feedback.
7. **Save** (`save_project`), and export when asked (`export_godot` for games).

## Building blocks

- Time is in beats from 0. In 4/4, bar *n* starts at beat 4(*n*−1).
- Write 4- or 8-bar loops; game loops are usually 8–32 bars.
- Make a loop seamless: the last bar should lead back into the first (end on the V chord or a pickup), and don't let the melody stop dead on the last beat.
- Drums (General MIDI): 36 kick, 38 snare, 39 clap, 42 closed hat, 46 open hat, 49 crash, 51 ride, toms 41–50. Velocity variety (hats 60–90, accents 110+) makes them feel human.
- Groove: `set_clip_swing` (amount_percent 30–60, grid_beats 0.25) gives drums a shuffle; town and adventure themes often want it, combat usually doesn't. Notes with `chance` 25–75 (ghost snares, extra hats, an occasional crash) keep a looping beat from sounding copied; playback and exports stay identical run to run. Rolls are just several short notes inside one step.
- Bass: roots on strong beats, approach notes into chord changes, an octave below the chord (MIDI 28–48).
- Chords: voice them around middle C (MIDI 55–72), moving each note as little as possible between chords.
- Melody: above the chords (MIDI 67–84), a repeated motif with variation, rests to breathe.

## Recipes

| Use | Tempo | Key / mode | Sounds | Notes |
|---|---|---|---|---|
| Chiptune battle | 140–170 | Minor | Chip Square lead, Pluck arp, Acid/Fat Bass, Tight Kit | Fast 16th arps, driving 8th bass, snare on 2 and 4 |
| Exploration / overworld | 90–120 | Major or Lydian | Warm Keys, Soft Pad, Pluck, Classic Kit (light) | Open voicings, gentle hats, singable melody |
| Town / cozy | 80–100 | Major | Sampler piano or Warm Keys, Soft Pad, Sub Bass | Swingy or waltz (3/4), little or no drums |
| Dungeon / tension | 60–90 | Minor, Phrygian | Soft Pad, Sub Bass, Boomy Kit toms | Drones, low pulses, sparse hits, slow filter automation |
| Boss | 150–180 | Harmonic minor | Brass Stab, Bright Lead, Fat Bass, Boomy Kit | Stabs on off-beats, toms, Distortion on bass |
| Menu / title | 70–100 | Major or Dorian | Soft Pad, Pluck, Sampler piano | Calm, loops cleanly, little low end |
| Victory jingle | 120–140 | Major | Bright Lead, Brass Stab | 2–4 bars, ends on the tonic, not looped |

Progressions that work: I–V–vi–IV (C G Am F), vi–IV–I–V (Am F C G), i–VI–III–VII (Am F C G), i–iv–v (Am Dm Em), i–♭VII–♭VI–♭VII (Am G F G).

## Layers and sections for games

- **Layers (stems)**: write so each track still sounds complete as layers are removed: base (pad + bass), + drums, + melody. Export with stems; the game fades layers in.
- **Sections**: mark song parts with `add_marker` ("Explore" at 0, "Combat" at 32...) so the user sees them on the timeline; `export_godot` with `sections_from_markers` makes one loop per section and an `AudioStreamInteractive` that switches on the bar. Keep sections in the same key and tempo, and make each loop on its own.
- **Sections**: write "explore" and "combat" in the same key and tempo, each a whole number of bars, then `export_godot` with named sections for an `AudioStreamInteractive` that switches on the next bar.

## Mix targets

- Game music: about **-16 LUFS** integrated (-14 to -20 is fine), true peak under **-1 dBTP**. The Godot export normalizes to -16 by default.
- Kick and bass carry the low end; cut lows elsewhere (EQ). Lead and vocal sit on top; pads sit under.
- Reverb on pads and keys; less (or none) on bass and kick. A limiter on the master.
- Use automation for movement: filter sweeps, swells, fade-ins.

## Teaching

When the user asks to be taught: follow [lessons](lessons.md), one step at a time. Let them do the clicking; use `get_song` to see what they did, and explain in plain language (no code, no jargon without a one-line meaning). Praise what works, fix one thing at a time.
