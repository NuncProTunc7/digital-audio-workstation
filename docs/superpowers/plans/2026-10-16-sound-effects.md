# Sound Effects Generator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An sfxr-style game sound effects maker: a list of sound recipes saved in the project, a Sound FX panel to shape and hear them, and export of each as a WAV into a Godot project, all usable by the owner's AI.

**Architecture:** Recipes are data in `daw-model` (`sound.rs`, plus pure generators), changed only through new Commands (`command/sounds.rs`). A new pure-DSP crate `daw-sfx` turns a recipe into samples. `daw-engine` gains a one-shot player for hearing a sound; `daw-export` writes the files; `daw-control` exposes `PlaySound` and `ExportSounds` requests (MCP + Tauri); the React panel `SoundFx.tsx` drives it all.

**Tech Stack:** Rust (workspace crates), rtrb engine messages, Tauri 2, React + TypeScript, vitest.

**Spec:** `docs/superpowers/specs/2026-10-16-sound-effects-design.md`

## Global Constraints

- Everything must work on Windows first; tests need no sound card.
- Audio-thread code: no allocation, locks, I/O or panics; mark `// RT-SAFE`; keep `crates/daw-engine/tests/no_alloc.rs` green and extend it.
- All project edits go through Commands; each Command: serde + schemars derive, doc comment (it is the AI's tool description: what, units, ranges, errors), exact inverse, JSON round-trip + apply/undo test, and a case in `app/src/preview.ts`; UI types in `app/src/types.ts` mirror Rust.
- No new third-party dependencies (own deterministic RNG). sfxr/jsfxr are MIT: if any code is ported line by line, add a source header and a `THIRD_PARTY.md` entry.
- Recipe fields keep sfxr's normalised ranges: 0..1, or -1..1 where marked; `volume_db` -24..+6. Out-of-range values are clamped; NaN/inf is an error.
- Sounds are capped at 10 seconds. Export: 16-bit, 44.1 kHz WAV (`GAME_SAMPLE_RATE_HZ`), default folder `sfx`, name from `file_slug(name)`; peaks over -1 dBFS are turned down to -1 dBFS and reported.
- Old project files without `sounds` load unchanged; files with no sounds serialize without the field.
- Sounds are not part of saved versions (`SongState`): loading a version keeps them.
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `npm --prefix app run typecheck`, `npm --prefix app run lint`, `npm --prefix app test` pass at the end of every task that touches them.
- User-facing text says "your AI", not "Claude".

## Review Focus

1. **Extreme slider values** (every field at 0, at 1, at -1; `freq_ramp` -1 with `base_freq` 0): must render finite samples, never hang, stop within 10 s. → Task 2 test `extreme_recipes_are_finite_and_bounded`.
2. **Sound ids colliding with other ids** after undoing a removal or loading a version: `id_in_use`/`all_ids` must include sounds. → Task 1 test `sound_ids_are_reserved_and_survive_loading_a_version`.
3. **Two sounds whose names slug to the same file** (`Coin!` and `coin`): export must not silently overwrite one with the other. → Task 4 test `names_that_make_the_same_file_get_numbered`.
4. **Playing a sound while the song plays, and pressing Play again quickly**: the song keeps going, the new sound replaces the old, the old buffer is freed off-thread. → Task 3 test `a_new_one_shot_replaces_the_old_and_the_old_comes_back`.
5. **Slider drag spam**: a drag produces one undo step and the sound replays only after release, not on every move. → Task 6 vitest `a_slider_drag_is_one_undo_step`.

---

### Task 1: Recipe data, generators, and Commands (`daw-model`)

**Files:**
- Create: `crates/daw-model/src/sound.rs`, `crates/daw-model/src/command/sounds.rs`
- Modify: `crates/daw-model/src/lib.rs` (module + re-exports), `crates/daw-model/src/project.rs` (`sounds` field, `id_in_use`, `all_ids`), `crates/daw-model/src/command/mod.rs` (variants, dispatch, `coalesces_with`), `crates/daw-model/src/summary.rs` (list sounds), `app/src/types.ts`, `app/src/preview.ts`
- Test: `crates/daw-model/src/command/tests.rs`, unit tests in `sound.rs`, `app/src/preview.test.ts`

**Interfaces:**
- Produces:
  - `pub enum Wave { Square, Saw, Sine, Triangle, Noise }` (serde snake_case)
  - `pub enum SoundCategory { Coin, Jump, Laser, Explosion, Hit, PowerUp, Blip, Random }` (serde snake_case: `power_up`)
  - `pub struct SoundRecipe { wave, attack, sustain, punch, decay, base_freq, freq_limit, freq_ramp, freq_dramp, vib_strength, vib_speed, arp_mod, arp_speed, duty, duty_ramp, repeat_speed, phaser_offset, phaser_ramp, lpf_freq, lpf_ramp, lpf_resonance, hpf_freq, hpf_ramp, crush, volume_db }`: all `f32` except `wave`; `#[serde(default)]` on each field so the AI can send a partial recipe; `Default` = a plain blip (Square, sustain 0.1, decay 0.2, base_freq 0.5, lpf_freq 1.0, volume_db 0).
  - `impl SoundRecipe { pub fn clamped(self) -> Result<SoundRecipe, CommandError> }` (NaN/inf → error naming the field; others clamped to range)
  - `pub struct SoundEffect { pub id: Id, pub name: String, pub recipe: SoundRecipe }`
  - `pub fn generate(category: SoundCategory, seed: u64) -> SoundRecipe`, `pub fn vary(recipe: &SoundRecipe, seed: u64) -> SoundRecipe` (sfxr's `mutate`: each field moves by up to ±0.05 with probability 1/2, then clamped)
  - `struct Rng(u64)` private xorshift64* in `sound.rs`; seed 0 maps to a fixed nonzero state
  - `Project::sounds: Vec<SoundEffect>`; `Project::sound(id) -> Option<&SoundEffect>`
  - Commands (snake_case tags): `AddSound { name: Option<String>, recipe: Option<SoundRecipe>, category: Option<SoundCategory>, seed: Option<u64> }` → inverse `RemoveSound`; `SetSound { sound_id, recipe }` → `SetSound` (old); `VarySound { sound_id, seed }` → `SetSound` (old); `RenameSound { sound_id, name }` → `RenameSound`; `RemoveSound { sound_id }` → `RestoreSound { sound, index }`; `RestoreSound` → `RemoveSound`.

- [ ] **Step 1: Write the failing tests** in `command/tests.rs`, following the marker tests:
  - `sound_commands_round_trip_as_json`: each of the six variants `serde_json` round-trips equal.
  - `adding_a_coin_and_undoing_removes_it`: `AddSound { category: Some(Coin), seed: Some(7), .. }` → one sound named `"coin"`; `generate(Coin, 7)` equals its recipe; undo → `sounds` empty; redo restores the same id.
  - `names_are_made_unique`: two `AddSound` with category Coin → names `"coin"`, `"coin_2"`; `AddSound` with no name/category/recipe → `"sound"` with `SoundRecipe::default()`.
  - `set_sound_clamps_and_rejects_nan`: `base_freq: 3.0` stored as 1.0; `attack: f32::NAN` → `Err`, project unchanged.
  - `set_sound_drags_coalesce`: two `SetSound` for one id coalesce; for different ids they don't.
  - `remove_and_restore_keep_id_and_position`: remove the middle of three, undo → same id at index 1.
  - `vary_sound_is_undoable_and_seeded`: same seed → same recipe; undo → original.
  - `sound_ids_are_reserved_and_survive_loading_a_version`: add a sound, take a snapshot, remove the sound, load the snapshot → `sounds` still empty (not part of the version) and undo of the removal still restores the id without `IdInUse`; `id_in_use(sound.id)` is true while it exists.
  - Unit tests in `sound.rs`: `generate_is_deterministic_per_seed` (same seed equal, different seeds differ for every category), `every_generated_recipe_is_in_range` (100 seeds × all categories pass `clamped()` unchanged), `vary_stays_close` (every field within 0.05 of the original).
  - In `project.rs` tests (or `file.rs`): `projects_without_sounds_load_and_save_without_the_field`.
- [ ] **Step 2: Run** `cargo test -p daw-model sound` → FAIL (types not defined).
- [ ] **Step 3: Implement** `sound.rs` (types, `clamped`, `Rng`, `generate` following sfxr's `GenerateCoin/Jump/Laser/Explosion/Hit/Powerup/Blip`, and `Random` as sfxr's randomize), `command/sounds.rs` (pattern of `markers.rs`; name check trims, max 40 chars, unique by suffix `_2`, `_3`...), wire into `mod.rs` (doc comments per variant written for the AI, e.g. `AddSound`: "Add a game sound effect. Give a category (coin, jump, laser, explosion, hit, power_up, blip, random) to generate one, optionally with a seed for a different take; or a full/partial recipe. Returns nothing; read ids with get_song."), `project.rs` (`#[serde(default, skip_serializing_if = "Vec::is_empty")] pub sounds`, ids in `id_in_use`/`all_ids`), and `summary.rs` (`"sounds": [{id, name, wave, seconds}]` where seconds = sfxr length `(attack²+sustain²+decay²)·100000/44100` capped at 10).
- [ ] **Step 4: Run** `cargo test -p daw-model` → PASS.
- [ ] **Step 5: Mirror in UI**: `SoundRecipe`, `SoundEffect`, `Wave`, `SoundCategory`, `Project.sounds?` and the six command shapes in `types.ts`; cases in `preview.ts` (add: name uniqueness and a fixed default recipe per category from a small TS table, set, vary = small deterministic nudge, rename, remove, restore). Test in `preview.test.ts`: `add_sound then remove_sound leaves no sounds`. Run `npm --prefix app run typecheck && npm --prefix app test` → PASS.
- [ ] **Step 6: Commit** `model: sound effect recipes and their commands`.

### Task 2: `daw-sfx` crate: recipe to samples

**Files:**
- Create: `crates/daw-sfx/Cargo.toml` (deps: `daw-model` only), `crates/daw-sfx/src/lib.rs`, `crates/daw-sfx/tests/render.rs`
- Modify: root `Cargo.toml` (`daw-sfx` in `[workspace.dependencies]`, `[profile.dev.package.daw-sfx]` opt-level like `daw-dsp`), `THIRD_PARTY.md` if code is ported

**Interfaces:**
- Consumes: `daw_model::{SoundRecipe, Wave}`
- Produces: `pub const MAX_SECONDS: f32 = 10.0;` `pub fn render(recipe: &SoundRecipe, sample_rate_hz: u32) -> Vec<f32>` (mono, unclamped float, already scaled by `volume_db`); `pub fn peak(samples: &[f32]) -> f32`

- [ ] **Step 1: Write failing tests** in `tests/render.rs`:
  - `every_category_makes_a_clean_sound`: for all `SoundCategory` × seeds 0..20, `render(&generate(c, s), 44100)`: non-empty, all finite, `peak > 0.01` (not silent), `len <= 10·44100`.
  - `rendering_is_deterministic`: two renders of `generate(Explosion, 3)` are bit-identical (noise uses a fixed seed).
  - `volume_db_scales_the_output`: `volume_db: -6` peak ≈ 0.5 × the 0 dB peak (±2 %).
  - `length_follows_the_envelope`: default recipe length == `((attack²+sustain²+decay²)·100000)` samples at 44.1 kHz ± 1 block; at 22050 Hz it's half.
  - `extreme_recipes_are_finite_and_bounded`: every field set to its min, then its max, then `freq_ramp = -1, base_freq = 0`, each finite and ≤ 10 s, test finishes in under 2 s.
- [ ] **Step 2: Run** `cargo test -p daw-sfx` → FAIL (crate empty).
- [ ] **Step 3: Implement `render`** following the sfxr synth loop: super-sampling ×8, period/slide/delta-slide with `freq_limit` cut-off, vibrato, arpeggio change at `arp_speed`, square duty + sweep, `repeat_speed` restart of pitch params, 3-stage envelope with punch, phaser (1024-sample buffer), low-pass with resonance and ramp, high-pass with ramp, noise table refreshed per period from a fixed-seed xorshift; then `crush` (hold every `1 + crush·15` samples and quantize to `16 - crush·12` bits); scale by `10^(volume_db/20)` × sfxr's master 0.5. Time at other sample rates: run at 44.1 kHz internally and resample by linear interpolation (simple, sounds are short).
- [ ] **Step 4: Run** `cargo test -p daw-sfx` → PASS; clippy clean.
- [ ] **Step 5: Commit** `sfx: render sound effect recipes`.

### Task 3: Hearing a sound: engine one-shot and `PlaySound`

**Files:**
- Modify: `crates/daw-engine/src/message.rs` (`EngineMessage::PlayOneShot(Arc<[f32]>)`, `Garbage::OneShot(Arc<[f32]>)`), `crates/daw-engine/src/processor.rs` (field `one_shot: Option<(Arc<[f32]>, usize)>`, handle message, mix in `render_block`), `crates/daw-engine/src/engine.rs` (`pub fn play_one_shot(&self, mono: Vec<f32>)`), `crates/daw-engine/tests/no_alloc.rs`, `crates/daw-control/src/protocol.rs` (`Request::PlaySound { sound_id: Id }`), `crates/daw-control/src/host.rs` (handler + `pub fn play_sound<H: Host>(host, sound_id) -> Result<f64 /*seconds*/, String>`), `crates/daw-control/Cargo.toml` (+`daw-sfx`), `crates/daw-mcp/src/tools.rs` (tool `play_sound`, read-only, mapped in `to_request`), `app/src-tauri/src/lib.rs` (`#[tauri::command(async)] fn play_sound(state, sound_id) -> Result<f64, String>`, registered), `app/src/backend.ts` + `app/src/preview.ts` (`playSound(soundId): Promise<number>`; preview returns the length without sound)

**Interfaces:**
- Consumes: `daw_sfx::render`, `Project::sound`
- Produces: `Engine::play_one_shot(mono: Vec<f32>)`; `Request::PlaySound { sound_id }` → `{"playing": id, "seconds": f64}`; Tauri `play_sound`; `backend.playSound`

- [ ] **Step 1: Write failing tests** in `engine.rs` tests: `a_one_shot_plays_on_top_of_the_song` (stopped engine: output after `play_one_shot(vec![0.5; 100])` has ~0.5 in both channels for 100 frames then silence; while playing a song, the song's notes still sound), `a_new_one_shot_replaces_the_old_and_the_old_comes_back` (second call mid-sound: output switches to the new buffer; `collect_garbage` frees the old Arc — check `Arc::strong_count` of a clone held by the test drops to 1). Extend `no_alloc.rs`: send `PlayOneShot` (buffer built before the measured region) and process blocks during and after it with no allocation. In `host.rs` tests: `play_sound_needs_a_real_sound` (unknown id → error naming it).
- [ ] **Step 2: Run** `cargo test -p daw-engine one_shot && cargo test -p daw-engine --test no_alloc && cargo test -p daw-control play_sound` → FAIL.
- [ ] **Step 3: Implement.** Processor adds one-shot samples after the master gain, before `soft_clip` (`*l = soft_clip(*l * g + s)`, same `s` both sides), independent of transport; on finish or replace, `throw_away(Garbage::OneShot(old))`. `play_sound` renders at `engine.sample_rate_hz()` on the calling (non-audio) thread. MCP description: "Play one of the project's sound effects so the user can hear it (the song keeps playing). Returns its length in seconds."
- [ ] **Step 4: Run** the Step 2 commands → PASS.
- [ ] **Step 5: Commit** `engine, control: hear a sound effect`.

### Task 4: Export to Godot (`ExportSounds`)

**Files:**
- Create: `crates/daw-export/src/sounds.rs`, `crates/daw-export/tests/sounds.rs`
- Modify: `crates/daw-export/src/lib.rs` (module, re-exports), `crates/daw-export/Cargo.toml` (+`daw-sfx`), `crates/daw-control/src/protocol.rs` (`Request::ExportSounds(SoundExportOptions)`), `crates/daw-control/src/host.rs`, `crates/daw-mcp/src/tools.rs` (tool `export_sounds`), `app/src-tauri/src/lib.rs` (`#[tauri::command(async)] fn export_sounds`), `app/src/backend.ts`, `app/src/preview.ts`, `app/src/types.ts`

**Interfaces:**
- Consumes: `daw_sfx::{render, peak}`, `godot::check_folder`, `godot::res_path`, `file_slug`, `write_wav`, `GAME_SAMPLE_RATE_HZ`
- Produces:
  - `pub struct SoundExport { pub project_dir: PathBuf, pub folder: String, pub sound_ids: Option<Vec<Id>> }`
  - `pub struct SoundFile { pub sound_id: Id, pub res_path: String, pub seconds: f64, pub peak_dbfs: f64, pub turned_down_db: f64 }` (Serialize)
  - `pub struct SoundExportReport { pub files: Vec<SoundFile> }` (Serialize)
  - `pub fn export_sounds(project: &Project, spec: &SoundExport) -> Result<SoundExportReport, ExportError>`
  - `daw_control::SoundExportOptions { project_dir: String, folder: Option<String> /* "sfx" */, sound_ids: Option<Vec<Id>> }`, `daw_control::export_sounds(host, &options)`; TS `exportSounds(options): Promise<SoundExportReport>`

- [ ] **Step 1: Write failing tests** in `tests/sounds.rs` (temp dir with `project.godot`, as in `tests/godot.rs`): `writes_one_wav_per_sound` (two sounds → `res://sfx/coin.wav`, `res://sfx/jump.wav` exist, valid RIFF/16-bit/44100, frames = render length), `exports_only_the_chosen_sounds`, `refuses_a_folder_without_project_godot`, `refuses_folders_that_escape_the_project` (`../x`), `turns_down_a_clipping_sound` (`volume_db: 6` → file peak ≤ -1 dBFS + 0.01, `turned_down_db > 0`), `names_that_make_the_same_file_get_numbered` (`Coin!` and `coin` → `coin.wav`, `coin_2.wav`), `unknown_sound_id_is_an_error`.
- [ ] **Step 2: Run** `cargo test -p daw-export --test sounds` → FAIL.
- [ ] **Step 3: Implement** `export_sounds`: render at 44100, gain down to -1 dBFS if over, duplicate mono to stereo for `write_wav(path, &stereo, GAME_SAMPLE_RATE_HZ, None)`; no `.import` file (Godot's default WAV import is right for effects). Control handler + MCP tool `export_sounds` ("Write the project's sound effects (all, or sound_ids) as WAV files into the user's Godot project, in folder (default sfx). Peaks above -1 dBFS are turned down. Returns each file's res:// path, length, and peak. Ask the user for their Godot project folder if you don't know it."), Tauri command, backend.
- [ ] **Step 4: Run** `cargo test -p daw-export && cargo test -p daw-control && cargo test -p daw-mcp` → PASS (the MCP tool-list tests may need the new names).
- [ ] **Step 5: Commit** `export: sound effects into a Godot project`.

### Task 5: MCP descriptions and AI guidance

**Files:**
- Modify: `crates/daw-mcp/src/main.rs` (server instructions: one bullet "Game sound effects: add_sound with a category, adjust with set_sound or vary_sound, hear with play_sound, write to Godot with export_sounds; read_guide('sound-effects')"), `crates/daw-mcp/src/tools.rs` (decide whether `add_sound`/`play_sound` join `CORE_TOOLS`: no, keep core small)
- Test: `crates/daw-mcp/src/tools.rs` tests

- [ ] **Step 1: Write failing test** `sound_tools_are_offered`: tool list contains `add_sound`, `set_sound`, `vary_sound`, `rename_sound`, `remove_sound`, `play_sound`, `export_sounds`, and not `restore_sound` (hidden like other `Restore*`; check how existing `restore_*` are filtered and follow it).
- [ ] **Step 2: Run** `cargo test -p daw-mcp sound_tools` → FAIL or PASS; fix filtering if `restore_sound` appears.
- [ ] **Step 3: Implement** the instructions bullet.
- [ ] **Step 4: Run** `cargo test -p daw-mcp` → PASS.
- [ ] **Step 5: Commit** `mcp: tell AIs about sound effects`.

### Task 6: The Sound FX panel (UI)

**Files:**
- Create: `app/src/components/SoundFx.tsx`, `app/src/components/SoundFx.test.tsx`
- Modify: `app/src/App.tsx` (state `soundFxOpen`, a **Sound FX** toolbar button, render the panel; Export menu item "Sound effects to Godot…" opens the panel), `app/src/App.css` (or the file holding panel styles)

**Interfaces:**
- Consumes: `backend.playSound`, `backend.exportSounds`, `backend.pickFolder`, `onCommand`, `onEndGesture`; Godot folder remembered with the same `localStorage` key as `GodotExportDialog` (export its `rememberedDir`/key from there rather than duplicating).
- Produces: `export default function SoundFx(props: { project: Project; onCommand(c: Command): Promise<void>; onEndGesture(): void; onPlay(id: number): void; onExport(o: SoundExportOptions): Promise<SoundExportReport | undefined>; onPickFolder(): Promise<string | null>; onClose(): void })`

- [ ] **Step 1: Write failing vitest tests** (preview backend, as other component tests do): `a_category_button_adds_and_selects_a_sound` (click **Coin** → list shows `coin`, it is selected, `onPlay` called with its id), `a_slider_drag_is_one_undo_step` (several `input` events on **Pitch** then `pointerup` → one `end_gesture`, `onPlay` called once after release), `vary_changes_the_recipe`, `more_shows_every_setting` (after **More**, a control for each recipe field and a waveform select), `rename_and_delete_work`, `export_lists_the_files_written`.
- [ ] **Step 2: Run** `npm --prefix app test -- SoundFx` → FAIL.
- [ ] **Step 3: Implement** the panel per the spec's "What the owner sees": list (select, double-click rename, delete, **+ New**); buttons Coin, Jump, Laser, Explosion, Hit, Power-up, Blip, Randomize (each `add_sound` with a random `seed` from `crypto.getRandomValues`, then select the new id and play it); **Vary** (`vary_sound` with a random seed, then play); simple sliders Pitch=`base_freq`, Length=`sustain`+`decay` (scaled together, keeping their ratio), Slide=`freq_ramp`, Punch=`punch`, Tone=`lpf_freq`, Crunch=`crush`, Volume=`volume_db`, using `ParamControl`'s slider/gesture pattern (`set_sound` while dragging, `onEndGesture` + play on release); **More** reveals waveform select and every other field with plain labels and a one-line tooltip each; **Play** button and Space while the panel has focus; **Export to Godot** (this sound / all sounds, folder field default `sfx`, project folder chooser) showing the report's files, lengths and any "turned down" notes. Empty state: "No sounds yet. Pick a kind of sound to start."
- [ ] **Step 4: Run** `npm --prefix app run typecheck && npm --prefix app run lint && npm --prefix app test` → PASS. Build the UI once (`npm --prefix app run build`) so Tauri embeds it.
- [ ] **Step 5: Check in the browser preview** (port 5199): open the panel, click each category, drag sliders, open More; screenshot at desktop and narrow widths, no console errors.
- [ ] **Step 6: Commit** `app: Sound FX panel`.

### Task 7: Guide, notes, and the full check

**Files:**
- Create: `docs/guide/sound-effects.md`
- Modify: `crates/daw-mcp/src/guide.rs` (register the page), `docs/guide/README.md` (list it), `docs/guide/godot.md` (link), `docs/tutorial-notes.md` (tick the backlog item with date 2026-10-16), `docs/PLAN.md` (one line under features: sound effects generator built), `AGENTS.md` (add `daw-sfx` to the Stack list)

- [ ] **Step 1: Write the guide page**: what it's for; the kind buttons, Randomize and Vary; each simple slider in one plain sentence; what More adds; recipes for common effects (softer coin, bigger explosion, retro laser, menu click); exporting to Godot and playing a WAV from `AudioStreamPlayer`; asking your AI ("make three jump sounds, one heavier").
- [ ] **Step 2: Run** `cargo test -p daw-mcp guide` → PASS (the guide-page test checks registration).
- [ ] **Step 3: Full check**: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `npm --prefix app run typecheck`, `npm --prefix app run lint`, `npm --prefix app test` → all PASS.
- [ ] **Step 4: Commit** `docs: sound effects guide page`.
