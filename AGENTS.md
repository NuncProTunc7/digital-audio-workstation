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

- Rust workspace in `crates/` (engine, DSP, instruments, model, MCP).
- Tauri 2 app in `app/` (`src-tauri/` Rust shell, `ui/` React + TypeScript + Vite).
- Audio I/O: cpal (WASAPI, optional ASIO). MIDI: midir. MCP: rmcp.

## Commands

Not set up yet; Phase 0 will establish these. Update this section once they exist.

```
cargo build --workspace            # build all Rust crates
cargo test --workspace             # unit + headless render tests
cargo clippy --workspace -- -D warnings
cargo fmt --all
npm --prefix app/ui run lint
npm --prefix app/ui run test
npm --prefix app run tauri build   # Windows installer
```

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
- Each Command: `serde` + `schemars` derive, a doc comment (it becomes the MCP tool description Claude reads), `apply`, and `undo`.
- Adding a UI feature without a Command is a bug. The MCP tool list is generated from Commands; do not hand-write MCP tools that bypass them.

### 3. Tests
- New DSP or instrument code: add a headless render test (no NaN/inf, no clipping beyond expectations, deterministic output).
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
- TypeScript: strict mode, no `any`.
- Comments explain *why*, not *what*. Doc comments on public items.
- Keep audio units explicit in names: `gain_db`, `freq_hz`, `time_samples`, `time_beats`.

### 6. Commits
- Small, focused commits with descriptive messages (`engine: add metronome click`).
- Do not commit generated installers, rendered audio, or large sample libraries (use release downloads).
- Update `docs/PLAN.md` when a decision changes, and this file when commands or rules change.
