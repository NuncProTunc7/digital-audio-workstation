# Composing playbook

How an AI (Claude, Codex, or another) writes music in Nunc Pro Tune. The user can read it too: it explains what the AI does and why.

## Workflow

1. **Brief.** Know (or ask, briefly) the use: menu, exploration, combat, boss, victory, ambience. The mood, references, tempo, loop length, and whether layers or sections are needed. If the user gives little, choose sensible defaults and say what they are.
2. **Read first.** `get_song` (and `describe_instruments` before changing sounds). To read what a clip plays, use the `notes_text` from `get_clip` (bar by bar, note names and lengths; `from_bar`/`to_bar` for long clips); the note list beside it has the ids for `edit_notes`. Don't overwrite the user's work: add tracks, or ask. Never `new_project` without asking.
3. **Build in one undo step per idea**, with `batch`: e.g. add a track, load a preset, create a clip, add notes.
4. **Order**: tempo and time signature → drums → bass → harmony → melody → texture (pads, arps, counter-melody) → mix → automation.
5. **Check.** `analyze_mix` (with `spectrogram` when judging tone: a picture plus `spectrogram_summary`, the same bar by bar in words; `per_track` for balance). Fix clipping and masking.
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
| Orchestral exploration | 80–110 | Major or Lydian | Library cello (bowed), double bass (pizzicato), flute, Soft Pad | Cello melody, flute answering it, plucked bass on roots, pad for air; Reverb bus |
| Epic boss (orchestral) | 140–170 | Harmonic minor | Library war tuba, cello (bowed), double bass (arco), Boomy Kit | Tuba and bass in octaves on driving 8ths, cello ostinato, toms; Compressor + Reverb |

Guitar tracks: pick `string.tuning` for the style (Drop D or Drop C for heavy combat riffs, DADGAD or Open G for folk and fantasy towns, Standard otherwise) and keep notes between the lowest open string and about 22 frets above the top one. Write chords as guitar voicings, one note per string (an open E chord is 40 47 52 56 59 64), so each lands on its own string; use `strum.time_s` 0.015–0.04 for strummed chords and Palm Muted for chugging rhythm.

Library instruments (sample_library) must be downloaded first: call `download_sample_pack`, tell the user how big it is, poll `sample_library` until `installed` lists its programs, then `add_track` a sampler and `load_sample_pack` with a program's path. Keep them in their natural ranges (cello C2–A5, double bass E1–G3, flute C4–C7, tuba D1–F4): once a pack has loaded, get_track's `sample_pack_status` gives `lowest_note` and `highest_note`, and notes outside them are silent. Sustained instruments (flute, strings, brass) hold a note as long as it lasts. Mention the credit line for CC-BY packs.

Progressions that work: I–V–vi–IV (C G Am F), vi–IV–I–V (Am F C G), i–VI–III–VII (Am F C G), i–iv–v (Am Dm Em), i–♭VII–♭VI–♭VII (Am G F G).

## Layers and sections for games

- **Layers (stems)**: write so each track still sounds complete as layers are removed: base (pad + bass), + drums, + melody. Export with stems; the game fades layers in.
- **Harmony first**: `set_key` (e.g. tonic 9 + minor for A minor; dorian for adventure, lydian for magic, harmonic minor for villains, mixolydian for heroic), then lay out the progression with `add_chord` (one per bar or two) before writing parts. get_song lists the scale notes and each chord's notes and span: bass on roots and fifths, pads on chord tones, melody mostly in the scale with chord tones on strong beats. If the user already set chords, follow them.
- **Sidechain**: `set_effect_sidechain` on a Compressor (on bass or pads) with the drum track as `source` makes them duck on each kick: threshold -30 dB, ratio 4-8, attack 0.005 s, release 0.1-0.2 s. Use it for energetic combat and electronic tracks, not calm ones.
- **Buses**: for a cohesive space, `add_bus` "Reverb" with one Reverb effect (mix 1.0, add via add_effect with the bus id as track_id) and `set_send` each melodic track to it (-12 to -6 dB) instead of a reverb per track. Group stems for games with `set_track_output` (Drums, Music, Ambience buses) and export with `bus_stems`.
- **Intensity versions** (quiet / normal / boss, explore / combat): build them as sections in one song (add_marker), with shared material as linked clips (`duplicate_clip` with `linked: true`, or `link_clips`): editing one linked clip edits them all, so the theme stays consistent. Vary arrangement per section (layers, drums, register, tempo feel via swing) rather than rewriting the melody.
- **Offering options** (the user asks for "a few versions", "options", "variations", or you're unsure what they want): first `take_snapshot` "Before options"; then for each option make the changes, `take_snapshot` "Option N – short description", and `load_snapshot` "Before options" to reset before the next. End with the song as it was, and tell the user to compare them with Versions → A/B (the B menu switches between options) and Keep B on their favorite. Keep options focused on the passage asked about, and make them genuinely different.
- **Before exporting**: run `inspect_export` with the same options as `export_godot` and fix any "problem" findings first (usually: a Limiter on the master for clipping, or shortening a note that rings across the loop end).
- **Sections**: mark song parts with `add_marker` ("Explore" at 0, "Combat" at 32...) so the user sees them on the timeline; `export_godot` with `sections_from_markers` makes one loop per section and an `AudioStreamInteractive` that switches on the bar. Keep sections in the same key and tempo, and make each loop on its own.
- **Sections**: write "explore" and "combat" in the same key and tempo, each a whole number of bars, then `export_godot` with named sections for an `AudioStreamInteractive` that switches on the next bar.

## Mix targets

- Game music: about **-16 LUFS** integrated (-14 to -20 is fine), true peak under **-1 dBTP**. The Godot export normalizes to -16 by default.
- Kick and bass carry the low end; cut lows elsewhere (EQ). Lead and vocal sit on top; pads sit under.
- Reverb on pads and keys; less (or none) on bass and kick. A limiter on the master.
- Use automation for movement: filter sweeps, swells, fade-ins.

## Sound effects

Game sound effects are recipes in the song (`sounds` in `get_song`); see [Sound effects](sound-effects.md) for every setting.

- **Start from a category** with `add_sound` (coin, jump, laser, explosion, hit, power_up, blip, random). Make two or three takes (different `seed`s), `play_sound` each, and let the user pick; remove the rest.
- **Read before changing**: `get_sound` gives every setting plus `start_hz`, `seconds`, `peak_dbfs` and `silent`. `set_sound` changes only the settings you send.
- **Words to settings**:
  - lower or higher: `base_freq` (about 0.1 is low, 0.3 middle, 0.6 high);
  - shorter or longer: `sustain` and `decay`;
  - softer or gentler: lower `punch` and `lpf_freq`, or the `triangle` or `sine` wave;
  - brighter or harsher: raise `lpf_freq`, use the `saw` wave;
  - retro or crunchy: `crush` 0.3 to 0.6 with the `square` wave;
  - bigger or heavier: lower `base_freq`, longer `decay`, a little `repeat_speed` for explosions;
  - rises or falls: positive or negative `freq_ramp`;
  - "ding-ding" (coin): `arp_mod` 0.2 to 0.5 with `arp_speed` about 0.6.
- **Keep a family consistent**: sounds for one game share a wave and similar `crush` and `lpf_freq`, so they feel like one set.
- **Levels**: most sounds should peak around -6 to -3 dBFS (`peak_dbfs`); make UI blips quieter than impacts. If `silent` is true or `seconds` is tiny, a slide fell below `freq_limit`: lower `freq_limit` or the slide.
- **Export**: `export_sounds` into the user's Godot project (`res://sfx/` by default). Name sounds the way the game uses them (`coin`, `player_jump`) before exporting.

## Teaching

When the user asks to be taught: follow [lessons](lessons.md), one step at a time. Let them do the clicking; use `get_song` to see what they did, and explain in plain language (no code, no jargon without a one-line meaning). Praise what works, fix one thing at a time.
