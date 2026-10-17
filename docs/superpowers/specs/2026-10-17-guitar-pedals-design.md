# Guitar pedals and guitar playing: design

Owner request (2026-10-15 and 2026-10-17): guitar effects pedals, and the other guitar follow-ups. The owner skipped the design questions; every choice below is the builder's call. Two projects, built in this order.

## Project 1: pedals

### What it is

Nine new built-in effects, usable on any track, bus or the master like the existing seven (EQ ... Limiter), so a recorded guitar on an audio track can use them too:

| Effect | Controls | Sound |
|---|---|---|
| **Overdrive** | Drive, Tone, Level | A warm, mid-boosted tube-style push (Tube Screamer idea): bass trimmed before clipping so chords stay clear. |
| **Fuzz** | Fuzz, Tone, Level | Hard, buzzy, square-ish clipping (Big Muff / Fuzz Face idea). |
| **Amp** | Gain, Bass, Mid, Treble, Master, Cabinet, Channel | A guitar amp and speaker: preamp gain stages, a tone stack, and a speaker cabinet (Off, 1x12 open back, 2x12, 4x12 closed). Channel: Clean, Crunch, High gain. |
| **Phaser** | Rate, Depth, Feedback, Mix | Swirling all-pass sweep (4 stages). |
| **Flanger** | Rate, Depth, Feedback, Mix | Jet-plane comb sweep (short modulated delay). |
| **Tremolo** | Rate, Depth, Shape | Volume pulsing (sine or square), for surf and western. |
| **Wah** | Mode, Position, Sensitivity, Rate, Resonance | A sweeping band-pass "wow". Mode: Auto (follows how hard you play), Sweep (moves by itself), Fixed (set Position, or automate it). |
| **Noise gate** | Threshold, Release | Silences hiss and hum between phrases (for recorded guitars; also tightens high-gain chugs). |
| **Octaver** | Octave down, Octave up, Dry | Adds a note an octave below (classic analog "flip-flop" octaver) and a fuzzy octave above. |

The existing Distortion, Chorus, Delay, Reverb, Compressor and EQ stay as they are and double as pedals.

Clipping stages (Overdrive, Fuzz, Amp) run at 4x the sample rate so high notes don't produce harsh false tones (aliasing).

### One-click rigs

**Rigs** are ready-made pedal chains, data in `daw-model` like instrument presets: Clean, Clean shimmer, Crunch, Blues, Classic rock, Metal, Fuzz lead, Funk, Surf, Ambient. Loading a rig replaces the track's effect chain in one undo step (new Command `LoadRig { track_id, rig }`; undo puts the old chain back exactly, plugin effects included, through `RestoreEffectChain`). The AI sees the rig list in `describe_effects` and can load one by name.

### Pedalboard

Guitar tracks (and audio tracks, for recorded guitar) show a **Pedalboard** in their instrument panel: the track's effects as a row of stomp boxes, each with its colour, name, a footswitch (on/bypass) and its main knobs; **Rig ▾** to load a rig; **+ Pedal** to add one. Knobs and footswitches are the same Commands as the Mixer's effect cards, so the two stay in step.

## Project 2: guitar playing

1. **Chord buttons on the fretboard**: common chords (C, D, E, G, A, Am, Em, Dm, plus the song's chord-track chords) as buttons; a click strums the chord on the guitar and lights the shape on the fretboard. Voicings are worked out for the current tuning and capo (standard open shapes where they fit), with the chord's root as the lowest note.
2. **String per note**: a note on a guitar track can name its string (1-6) instead of "auto". The piano roll gets a **String** choice for selected notes on guitar tracks; the guitar plays the note on that string; the tab shows it there. Auto (the default) keeps today's rule.
3. **Slides, hammer-ons and bends**: per-note techniques on guitar tracks: **Slide** (the pitch glides in from the previous note on the same string), **Hammer-on/pull-off** (no new pick: the string changes pitch and keeps ringing), **Bend** (the pitch rises by a half step, whole step or more and holds). Shown in the tab (MusicXML `<slide>`, `<hammer-on>`/`<pull-off>`, `<bend>`) and set from the piano roll.

A recorded (sampled) guitar for the free instrument library needs a pack with a clear CC0/CC-BY licence and a stable download; it's added only if one is found and checked.

## Constraints

- Real-time rules (AGENTS.md §1): no allocation in processing; oversampling buffers and filters are allocated when the effect is made. Each new effect goes into `no_alloc.rs`.
- Parameters are append-only data in `daw-model/src/effect.rs`; `app/src/generated/instruments.json` is regenerated.
- Every effect gets a render test (finite, bounded, does what it says) and the existing extreme-settings test covers it.
- Note fields added for project 2 are optional and default to today's behaviour, so old songs load unchanged.
