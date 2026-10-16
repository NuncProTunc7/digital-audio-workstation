# Sound effects generator: design

Date: 2026-10-16. Owner request (backlog in `docs/tutorial-notes.md`), design agreed in chat.

## Goal

Make game sound effects (coin, jump, laser, explosion, hit, power-up, blip) in the app, in the spirit of sfxr/jsfxr, and drop them into a Godot project as files. The owner shapes them with category buttons, Randomize, Vary, six plain sliders and a **More** section; their AI can do everything the owner can ("a softer coin, lower").

Agreed with the owner:
- Main use: **files for the game** (one file per sound). Placing sounds on song tracks is out of scope for now.
- Controls: **category buttons + Randomize + Vary + 6 simple sliders**, with **More** opening the full sfxr-style set (~20).
- The list of sounds is **saved in the project file** (`.nptune`), undoable like any edit.

## What the owner sees

A **Sound FX** button in the toolbar opens a panel:

- **List of sounds** (left): name, select, rename (double-click), delete, **+ New** (a Blip to start).
- **Make a new one**: Coin, Jump, Laser, Explosion, Hit, Power-up, Blip, Randomize. Each adds a new sound named after its category (`coin`, `coin_2`...) and plays it.
- **Vary**: nudges the selected sound a little (a new take on the same idea).
- **Simple sliders**: Pitch, Length, Slide, Punch, Tone, Crunch, plus Volume.
- **More**: waveform (square, saw, sine, triangle, noise), every recipe field below.
- **Play** (and the Space key while the panel is focused). Any change plays the sound again after the drag ends.
- **Export to Godot**: one sound or all; asks for the Godot project folder (same chooser as the music export, remembered between uses) and a folder inside it (default `sfx`).

Every change is a Command, so Ctrl+Z works and the AI's changes show as "Your AI: change sound".

## The recipe (data)

New module `daw-model/src/sound.rs`:

```rust
pub struct SoundEffect { pub id: Id, pub name: String, pub recipe: SoundRecipe }
pub struct SoundRecipe {   // sfxr fields; all 0..1 unless noted
    pub wave: Wave,        // Square | Saw | Sine | Triangle | Noise
    pub attack, sustain, punch, decay,        // envelope
    pub base_freq, freq_limit,                // pitch, lowest pitch before it cuts off
    pub freq_ramp, freq_dramp,                // slide, change of slide (-1..1)
    pub vib_strength, vib_speed,
    pub arp_mod (-1..1), arp_speed,           // pitch jump and when
    pub duty, duty_ramp (-1..1),              // square width and its sweep
    pub repeat_speed,
    pub phaser_offset (-1..1), phaser_ramp (-1..1),
    pub lpf_freq, lpf_ramp (-1..1), lpf_resonance,
    pub hpf_freq, hpf_ramp (-1..1),
    pub crush,             // bit crush / sample-rate reduction (0 = off)
    pub volume_db,         // -24..+6
}
```

The fields keep sfxr's normalised ranges so recipes match what sfxr/jsfxr users know and the generators port directly; every field's doc comment says what it does and its range (that text becomes what the AI reads). Out-of-range values are clamped by the Command, with the error case for NaN.

The simple sliders are views onto fields: Pitch = `base_freq`, Length = `sustain` + `decay` together, Slide = `freq_ramp`, Punch = `punch`, Tone = `lpf_freq`, Crunch = `crush`.

`Project` gains `sounds: Vec<SoundEffect>` (`serde(default, skip_serializing_if = "Vec::is_empty")`, so old files load and song-only files don't change).

Generators (pure, in `daw-model/src/sound.rs`): `generate(category, seed) -> SoundRecipe` and `vary(&recipe, seed) -> SoundRecipe`, using a small built-in deterministic random generator (no new dependency). They follow sfxr's published category recipes. The UI picks the seed; the same seed gives the same sound.

## Commands

In `daw-model/src/command/sounds.rs`, following `markers.rs`:

- `AddSound { name, recipe: Option<SoundRecipe>, category: Option<Category>, seed: Option<u64> }`: with a recipe, uses it; with a category, generates one (seed defaults to a fixed value so the Command stays pure). Names are made unique (`coin_2`). Inverse: `RemoveSound`.
- `SetSound { sound_id, recipe }`: replaces the recipe. Coalesces with itself for the same sound (a slider drag is one undo step). Inverse: `SetSound` with the old recipe.
- `VarySound { sound_id, seed }`: applies `vary`. Inverse: `SetSound`.
- `RenameSound { sound_id, name }`, `RemoveSound { sound_id }` (inverse `RestoreSound { sound, index }`).

Errors: unknown sound id, empty name, NaN in a recipe. Each gets a JSON round-trip and apply/undo test, and a case in `app/src/preview.ts`. The MCP tools are generated from these as usual; `get_song`'s summary lists sounds (id, name, wave, length in seconds).

## Making the audio

New crate `daw-sfx` (pure DSP, no I/O): `render(&SoundRecipe, sample_rate_hz) -> Vec<f32>` (mono). A from-scratch Rust synthesis following the sfxr algorithm (sfxr and jsfxr are MIT, compatible with GPL-3; if any code is ported line by line, it gets a header and a `THIRD_PARTY.md` entry). Rendering is deterministic: the noise source uses a fixed seed. Sounds are capped at 10 seconds.

## Hearing it

Live action (not a project edit): `Request::PlaySound { sound_id }` (and a Tauri command for the UI) renders the sound off the audio thread, then sends it to the engine with a new `EngineMessage::PlayOneShot(Arc<[f32]>)`. The processor mixes it into the output on top of whatever is playing (the song keeps going) and hands the buffer back as garbage when done or replaced. It allocates nothing on the audio thread; `crates/daw-engine/tests/no_alloc.rs` is extended to cover it.

## Export

`Request::ExportSounds { project_dir, folder (default "sfx"), sound_ids (default all) }` in `daw-control`, using `daw-export`'s Godot folder checks (must contain `project.godot`, no escaping paths) and `file_slug` for names: writes `<folder>/<name>.wav` (16-bit, 44.1 kHz). If a sound would clip, it is turned down to peak at -1 dBFS and the report says so. No loop settings: Godot's default import (no loop) is right for effects. The report lists each file with its length and peak, so the AI can check its work. MCP tool: `export_sounds`; MCP tool `play_sound`.

## Guide and notes

- New guide page `docs/guide/sound-effects.md` (registered in `crates/daw-mcp/src/guide.rs`): the buttons, what each slider does, recipes for common effects, exporting to Godot, and how to ask your AI.
- `docs/guide/godot.md` links to it; `docs/tutorial-notes.md` item ticked.

## Tests

- `daw-sfx`: every category over several seeds renders with no NaN/inf, isn't silent, peaks within limits, and stops within its length; same recipe gives identical samples.
- `daw-model`: Command round-trips and apply/undo; generators deterministic per seed; `vary` stays close to the original; old project files without `sounds` still load.
- `daw-engine`: one-shot plays, mixes with a playing song, is returned as garbage; `no_alloc` stays green.
- `daw-control`: export writes the files into a temporary Godot project, refuses a non-Godot folder, turns down a clipping sound.
- UI (vitest, preview backend): category button adds and selects a sound; slider drag is one undo step; Vary changes the recipe.

## Not in this version

Sound effects on song tracks; importing/exporting jsfxr's text format; OGG for effects; per-sound Godot `AudioStreamRandomizer` resources. Each can be added later.
