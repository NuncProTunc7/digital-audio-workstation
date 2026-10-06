# AGENTS.md

Instructions for AI coding agents (Claude Code and others) working in this repository.

## Project

**Nunc Pro Tune**: an open-source (GPL-3.0-or-later) digital audio workstation for making game music, fully controllable by Claude through MCP. Full plan: [`docs/PLAN.md`](docs/PLAN.md). Read it before starting any feature work.

## Working with the owner

- The owner is **not a programmer**. They test builds and judge how things sound and feel.
- Explain changes in plain language: what they can now do, how to try it, what to listen for.
- Ask before changing anything in the "Locked decisions" table in `docs/PLAN.md`.
- Bug reports will be descriptive ("it crackles when I add reverb"). Turn them into a reproducible test before fixing.
- The owner works on **Windows**. Every feature must work there first.

## Stack

- Rust workspace in `crates/`: `daw-model` (project, tracks/clips/notes, mixer, Commands, instrument and effect catalogs, `.nptune` file format), `daw-dsp` (oscillators, filters, biquads, envelopes), `daw-instruments` (Synth, DrumMachine), `daw-effects` (EQ, compressor, reverb, delay, chorus, distortion, limiter), `daw-engine` (real-time processor with clip sequencer, mixer, loop, recording capture; Engine handle; sound card; MIDI input; offline render), `daw-cli` (`npt` headless tool).
- Tauri 2 app in `app/`: `app/src/` is the React + TypeScript + Vite UI, `app/src-tauri/` is the Rust shell.
- In a plain browser (`npm --prefix app run dev`), the UI uses an in-memory preview backend (`app/src/backend.ts`) so it can be developed without the engine.
- Audio I/O: cpal (WASAPI; ASIO in Phase 6). MIDI: midir. MCP: rmcp.

## Commands

Run from the repository root unless noted. The UI must be built once (`npm run build` in `app/`) before Rust builds, because Tauri embeds it.

```
cd app && npm ci && npm run build && cd ..   # install UI deps, build UI into app/dist
cargo fmt --all                              # format Rust
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                       # unit + headless render tests (no sound card needed)
npm --prefix app run typecheck
npm --prefix app run lint                    # oxlint
npm --prefix app test                        # vitest
npm --prefix app run tauri dev               # run the desktop app with hot reload
npm --prefix app run tauri build             # release build + Windows installer (target/release/bundle/nsis/)
cargo run -p daw-cli -- render-test-tone --out tone.wav   # headless render
cargo run -p daw-cli -- schema               # JSON Schema of every Command
cargo run -p daw-cli -- instruments          # instrument parameters, presets, drum pads (JSON)
cargo run -p daw-cli -- render-demo --out demo.wav   # demo song rendered to WAV
cargo run -p daw-cli -- demo-project --out demo.nptune   # demo song as a project file
cargo run -p daw-cli -- render --project song.nptune --out song.wav   # any project to WAV
```

Dev builds optimize the audio crates (see `[profile.dev.package.*]` in `Cargo.toml`); unoptimized DSP cannot keep up in real time.

Linux builds need: `libwebkit2gtk-4.1-dev libasound2-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev libxdo-dev`.

CI (`.github/workflows/ci.yml`) runs all checks on Linux and builds the Windows installer, uploaded as the `nunc-pro-tune-windows-installer` artifact.

## Rules

### 1. Real-time audio thread
Code reachable from the audio callback must **never**:
- allocate or free memory (no `Vec::push`, `Box::new`, `String`, `format!`, cloning `Arc` that may drop last),
- take a lock (`Mutex`, `RwLock`) or block on a channel,
- do file, network, or console I/O (no `println!`, no logging),
- panic (avoid `unwrap`, indexing that can go out of bounds).

Use lock-free queues (`rtrb`) to talk to the audio thread. Pre-allocate buffers. Free old graphs off-thread. Mark real-time functions with a `// RT-SAFE` comment.

### 2. Commands are the only way to change a project
- All edits — from UI, shortcuts, or MCP — go through a `Command` in `daw-model`.
- Live actions (notes, play/stop, metronome, device choice) are not project edits: they go straight to the `Engine` and are not undoable. Don't use them to change saved state.
- Every Command's `apply` returns its exact inverse. Removals return a `Restore*` Command carrying the full object (with ids), so undo/redo keeps ids stable. Commands that a drag repeats (sliders, clip moves, note edits) must be listed in `Command::coalesces_with` so a drag is one undo step; the UI calls `end_gesture` when the drag ends.
- Ids (`Id`) are allocated from `Project::next_id` and never reused within a project.
- Instrument parameters and presets are data in `daw-model/src/instrument.rs` (append-only order: the index is the engine's parameter id). The DSP in `daw-instruments` matches on parameter ids. Effect parameters likewise live in `daw-model/src/effect.rs`, with DSP in `daw-effects`. After changing any of these, regenerate the UI copy: `cargo run -p daw-cli -- instruments > app/src/generated/instruments.json` (a test fails if you forget).
- The browser preview backend (`app/src/preview.ts`) mirrors Commands loosely so UI tests can run without Rust. When adding a Command, add a case there too.
- Each Command: `serde` + `schemars` derive, a doc comment (it becomes the MCP tool description Claude reads), `apply`, and `undo`.
- Adding a UI feature without a Command is a bug. The MCP tool list is generated from Commands; do not hand-write MCP tools that bypass them.

### 3. Tests
- New DSP or instrument code: add a headless render test (no NaN/inf, no clipping beyond expectations, deterministic output).
- Anything reachable from the audio callback must keep `crates/daw-engine/tests/no_alloc.rs` passing (it fails on any heap allocation during processing). Extend it when you add new engine messages.
- New Command: add a JSON round-trip + apply/undo test.
- Bug fix: add a test that fails before the fix.
- Tests must not require a sound card or MIDI device.

### 4. Licensing and borrowed code
- Only port code from licenses compatible with GPL-3.0-or-later (GPL-3, GPL-2.0-or-later, LGPL, MIT, BSD, Apache-2.0, zlib). See `docs/PLAN.md` §10.
- **Never copy code from AGPL projects (openDAW, Zrythm) or GPL-2.0-only code.** Ideas are fine.
- Ported code: add a header naming the source project, file, and license, and add an entry to `THIRD_PARTY.md`.
- Bundled audio samples/presets: CC0 or CC-BY only, recorded in `assets/LICENSES.md`.
- New Rust/npm dependencies must have GPL-3-compatible licenses.

### 5. Style
- Rust: `cargo fmt`, clippy clean, no `unsafe` without a `// SAFETY:` comment.
- TypeScript: strict mode, no `any`. UI types in `app/src/types.ts` mirror the Rust types; keep them in sync.
- Comments explain *why*, not *what*. Doc comments on public items.
- Keep audio units explicit in names: `gain_db`, `freq_hz`, `time_samples`, `time_beats`.

### 6. Commits
- Small, focused commits with descriptive messages (`engine: add metronome click`).
- Do not commit generated installers, rendered audio, or large sample libraries (use release downloads).
- Update `docs/PLAN.md` when a decision changes, and this file when commands or rules change.
