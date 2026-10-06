# Project Plan

**Status:** Draft v0.1 for owner review. Nothing here is built yet.
**License:** GPL-3.0-or-later.

## 1. What we are building

A free, open-source digital audio workstation (DAW) for making game music, with Claude able to operate every part of it.

**Goals**
- Record one live instrument at a time (built so more inputs can be added later), then edit, mix, and fine-tune.
- Compose on the computer with built-in instruments: **piano/keys, synth, drums, bass**.
- Write and read **sheet music**, and turn sheet music into tracks (including from a photo of a printed score).
- Let **Claude Desktop and Claude Code** control the whole app through MCP (Model Context Protocol).
- Export straight into **Godot**: seamless loops, stems, and adaptive-music layers.

**Non-goals (for now)**
- AI-generated raw audio (AI vocals, neural instrument rendering). Claude composes through MIDI and our instruments.
- macOS/Linux builds. The code stays cross-platform, but we test and ship on Windows first.
- Third-party plugins (VST3/CLAP). Planned for Phase 6; the architecture leaves room for them from day one.
- Video, notation engraving at publisher quality, live clip launching (Ableton Session view).

## 2. Locked decisions

| Area | Decision | Reason |
|---|---|---|
| Audio engine | **Rust** | Memory-safe; the compiler catches the crash and glitch bugs that plague C++ audio code. |
| Desktop shell | **Tauri 2** | Small Windows installer; Rust backend plus a web UI. |
| UI | **TypeScript + React + Vite**; timeline, piano roll, and waveforms drawn on `<canvas>` | Largest ecosystem; DOM is too slow for dense editors. |
| Audio I/O | **cpal**: WASAPI by default, **ASIO** as an option | ASIO gives the lowest latency for audio interfaces. Steinberg made the ASIO SDK GPLv3-compatible in Oct 2025. |
| MIDI I/O | **midir** | Standard Rust MIDI library; works on Windows. |
| Audio file import | **symphonia** (WAV, FLAC, MP3, OGG, AAC) | Pure Rust. |
| Audio file export | WAV (**hound**), OGG Vorbis (libvorbis bindings), FLAC | OGG is Godot's preferred music format. |
| Loudness analysis | **ebur128** crate | Standard LUFS/true-peak measurement. |
| Sheet music rendering | **OpenSheetMusicDisplay** (BSD-3, built on VexFlow) | Renders MusicXML in the browser. |
| MIDI files | **midly** | Read/write Standard MIDI Files. |
| AI control | MCP server written with **rmcp** (official Rust MCP SDK) | Works with both Claude Desktop and Claude Code. |
| Layout | Linear timeline first (GarageBand-style) | Simpler; suits writing complete tracks. |
| Plugins (later) | **CLAP first, then VST3** | Both are open (CLAP: MIT; VST3: MIT since SDK 3.8, Oct 2025). |
| Internal audio format | 32-bit float, 48 kHz default, stereo buses | Industry norm; matches Godot's mixer. |
| Project file | Folder: `MySong.daw/project.json` + `audio/` | Human-readable, diff-able, easy for Claude to inspect. |

## 3. Architecture

```
┌──────────────────────────── DAW app (one Windows process) ────────────────────────────┐
│                                                                                        │
│  UI (React, in Tauri webview)                                                          │
│     │  sends Commands / receives state updates  (Tauri IPC)                            │
│     ▼                                                                                  │
│  Core (Rust, normal threads)                                                           │
│     • Project model (tracks, clips, notes, mixer, tempo)                               │
│     • Command bus  ── every edit is a Command → undo/redo, autosave, AI control        │
│     • Graph compiler: turns the model into a real-time render graph                    │
│     │                                                                ▲                 │
│     │  lock-free queue (new graph, parameter changes)                │ meters, playhead│
│     ▼                                                                │                 │
│  Audio engine (Rust, real-time thread)                               │                 │
│     • instruments → effects → mixer → master → sound card            │                 │
│     • recording: input → disk writer thread                          │                 │
│                                                                                        │
│  Control server: JSON-RPC over WebSocket on 127.0.0.1 (token-protected)                │
└───────────────────────────────────────────▲────────────────────────────────────────────┘
                                            │
                              ┌─────────────┴─────────────┐
                              │  daw-mcp.exe (MCP bridge) │  ◄── stdio ──  Claude Desktop / Claude Code
                              └───────────────────────────┘
```

### Key design rules
1. **One command system for everyone.** The UI, keyboard shortcuts, and Claude all go through the same Command bus. If a button exists, Claude can press it. This also gives us undo/redo and crash-safe autosave.
2. **Commands are defined once.** Each Command is a Rust type with a JSON schema (`serde` + `schemars`). The MCP tool list is generated from those types, so the UI and AI never drift apart.
3. **The real-time thread never waits.** No memory allocation, locks, file I/O, or logging on the audio thread. The core builds a new render graph off-thread and swaps it in via a lock-free queue; old graphs are freed off-thread.
4. **Headless mode.** The engine can render a project to a file with no UI or sound card. This powers tests, CI, and Claude's "render and analyze" tools.

### Repository layout (target)
```
crates/
  daw-model/        project data model, Commands, undo/redo, file format
  daw-engine/       real-time graph, transport, recording, audio/MIDI device I/O
  daw-dsp/          shared DSP: oscillators, filters, envelopes, effects
  daw-instruments/  Keys, Synth, Drums, Bass, Sampler (SFZ)
  daw-notation/     MusicXML + MIDI file import/export, quantization
  daw-export/       WAV/OGG/FLAC render, loop points, stems, Godot export
  daw-analysis/     loudness, peaks, spectrum summary, tempo/key estimate
  daw-control/      local JSON-RPC server; command schema registry
  daw-mcp/          MCP bridge binary (stdio ↔ control server)
  daw-cli/          headless render/analyze tool (used by tests and CI)
app/
  src-tauri/        Tauri shell, wires crates together
  ui/               React + TypeScript front end
assets/             factory presets, drum kits, sample instruments (with licenses)
docs/               PLAN.md, design notes, user guide
```

## 4. Built-in instruments (v1)

All share one voice engine (polyphony, voice stealing, MIDI CC, pitch bend, sustain pedal) and a common preset format.

| Instrument | v1 design | Ideas to borrow from |
|---|---|---|
| **Synth** | 3 oscillators (saw/square/tri/sine/noise, band-limited) + sub, multimode filter (ladder + state-variable), 2 ADSR envelopes, 2 LFOs, small mod matrix, unison/detune, glide | LMMS TripleOscillator, Surge XT (GPL-3) filters/oscillators |
| **Keys** | (a) FM electric piano + organ models (no samples needed); (b) acoustic piano via the Sampler using the **Salamander Grand Piano** (CC-BY 3.0) as an optional download | Dexed (GPL-3) FM ideas; Salamander attribution required |
| **Drums** | 16-pad drum machine. Each pad is either a synthesized voice (kick, snare, hats, clap, tom, 808-style) or a sample. Built-in step sequencer plus piano-roll editing. Choke groups (open/closed hat). | LMMS Kicker; classic 808/909 circuit models |
| **Bass** | Mono synth tuned for bass: glide, sub-osc, drive, filter env presets (sub, pluck, reese, acid). Later: sampled electric bass via Sampler. | Synth engine above with bass-focused presets |
| **Sampler** | SFZ-format player (velocity layers, round robins, loop points) | sfizz (BSD-2) as reference |

### Playing without a MIDI keyboard
- **On-screen piano:** click or drag to play; shows held notes; octave and velocity controls.
- **Musical typing:** the computer keyboard plays notes (`A S D F…` = white keys, `W E T Y U…` = black keys, `Z`/`X` = octave down/up, `C`/`V` = velocity down/up), like GarageBand and Ableton.
- **Drum pads:** a clickable 4×4 pad grid, also mapped to keys.
- All three feed the same input path as a hardware MIDI keyboard, so they can be recorded to a track, quantized, and edited in the piano roll. A USB MIDI keyboard plugs into the same path later with no extra work.

Effects v1: EQ (parametric), compressor, reverb, delay, chorus, distortion, limiter. Every channel has gain, pan, mute, solo, and sends.

Only use sample content that is CC0, CC-BY, or similar, and record its license in `assets/LICENSES.md`.

## 5. Sheet music

| Feature | How |
|---|---|
| Show any MIDI track as notation | Quantize notes → MusicXML → render with OpenSheetMusicDisplay |
| Import MusicXML (from MuseScore, online libraries, etc.) | `daw-notation` parses it into tracks and notes |
| Import a **photo or PDF** of sheet music | Claude reads the image and writes MusicXML, then calls the `import_musicxml` tool. Dense scores may need a manual check. A dedicated optical music recognition (OMR) engine such as Audiveris (AGPL) could be added later. |
| Export notation | MusicXML (opens in MuseScore for printing) and PDF |
| MIDI files | Import/export `.mid` |

## 6. Godot export

| Feature | Detail |
|---|---|
| Seamless loops | Render with a tail-wrap so reverb/delay tails at the end flow into the loop start |
| Loop points | OGG: write Godot's `loop` + `loop_offset` import settings; WAV: write a `smpl` chunk (Godot's "Detect from WAV" loop mode) |
| Stems | Export each track or bus as its own synchronized file |
| Adaptive music | Optionally generate a Godot resource (`.tres`) for `AudioStreamSynchronized` (layered stems) or `AudioStreamInteractive` (sections with transitions) |
| Loudness | Normalize to a target LUFS so every track in the game matches |
| Export to project | Point at a Godot project folder; files land in `res://music/...` |

The exact Godot `.import` and `.tres` formats must be verified against the Godot 4.x version you use before we build Phase 5.

## 7. Claude control (MCP)

**Setup:** the installer includes `daw-mcp.exe`. One line in Claude Desktop's config or `claude mcp add daw <path>` in Claude Code. The DAW must be running; the bridge connects to it automatically.

**Tool groups (generated from Commands):**

| Group | Example tools |
|---|---|
| Session | `get_project_state`, `new_project`, `open_project`, `save_project`, `set_tempo`, `set_time_signature`, `set_key` |
| Tracks | `add_track`, `remove_track`, `rename_track`, `set_instrument`, `load_preset` |
| Notes | `create_clip`, `add_notes`, `edit_notes`, `quantize`, `transpose`, `humanize` |
| Audio | `import_audio`, `arm_track`, `record`, `trim`, `split`, `fade`, `time_stretch` (later) |
| Mixer | `set_volume`, `set_pan`, `add_effect`, `set_effect_param`, `add_send`, `automate` |
| Transport | `play`, `stop`, `set_loop_region`, `set_playhead` |
| Notation | `import_musicxml`, `export_musicxml`, `import_midi`, `export_midi` |
| Analysis ("Claude's ears") | `render_and_analyze` → LUFS, peak, clipping, spectrum balance, per-track levels |
| Export | `export_mix`, `export_stems`, `export_godot` |
| Sound design | `list_presets`, `describe_instrument_params`, `save_preset` |

**Safety:** the control server listens on localhost only, needs a token stored in the user's app data folder, and every AI edit is undoable.

## 8. Phases

Each phase ends with a Windows installer you can download from GitHub Actions and test.

| Phase | Deliverable | You can… |
|---|---|---|
| **0. Skeleton** | Rust workspace, Tauri app opens, CI builds a Windows installer, headless render test | Install and open an empty app |
| **1. Make sound** | Audio device selection (WASAPI/ASIO), transport, metronome, Synth instrument, **on-screen piano + computer-keyboard playing + clickable drum pads**, MIDI keyboard input | Play and record the synth with your mouse or computer keyboard (no MIDI hardware needed) |
| **2. Arrange** | Timeline, MIDI clips, piano roll, Drums + Bass + Keys, mixer with basic effects, save/load, undo/redo | Write a full instrumental track |
| **3. Claude** | Control server, MCP bridge, full tool list, analysis tools | Ask Claude to build or remix a track |
| **4. Record** | Audio input recording, latency compensation, audio clip editing, import audio files | Record guitar/vocals and mix them in |
| **5. Godot + notation** | Loop/stem/adaptive export, MusicXML/MIDI import-export, notation view | Drop music straight into your game; turn sheet music into tracks |
| **6. Expand** | CLAP then VST3 hosting, multi-input recording, sampled piano/bass packs, automation lanes, time-stretch | Use outside plugins, record a band |

Phase 3 (Claude) comes before recording on purpose: once Claude can drive the app, it can help test everything that follows.

## 9. Testing and quality

- **Offline DSP tests:** render instruments/effects headless and compare against reference files (golden tests), check for NaNs, clicks, denormals, and CPU budget.
- **Command tests:** every Command round-trips through JSON, applies, and undoes cleanly.
- **MCP tests:** scripted tool calls against a headless app instance.
- **CI:** GitHub Actions on `windows-latest` builds, tests, and publishes the installer as a build artifact on every push.
- **Your role:** install the latest build and report what sounds or feels wrong. Plain-language bug reports are fine.

## 10. Borrowing from other open-source projects

We may port algorithms and code from GPL-compatible projects to save time, with attribution.

| Project | License | Use |
|---|---|---|
| Surge XT | GPL-3 | Oscillators, filters, effects — can port with attribution |
| LMMS | GPL-2.0-or-later | Instrument designs, drum synthesis — can port |
| Ardour | GPL-2.0-or-later | Recording, latency compensation, mixer design — can port |
| Dexed | GPL-3 | FM synthesis — can port |
| sfizz | BSD-2 | SFZ sampler behavior — can port |
| openDAW, Zrythm | AGPL-3 | **Ideas only, no code** (AGPL adds network-use terms we don't want) |
| Any GPL-2.0-**only** code | — | **Do not copy** (incompatible with GPL-3) |

Check each source file's own header before porting; projects sometimes mix licenses. Every port gets a header comment naming the source, plus an entry in `THIRD_PARTY.md`.

## 11. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| **Recording latency on Windows** (top risk) | Recording feels out of time | ASIO support, measured round-trip latency compensation, direct monitoring guidance |
| Scope creep | Never ships | Phase gates; each phase must produce something usable |
| Audio glitches (dropouts, clicks) | Unusable for recording | Real-time rules in §3, CPU meter, buffer-size setting, stress tests |
| Owner can't debug code | Bugs linger | Strong tests + CI, clear error messages, in-app diagnostic report |
| Sample library licensing | Legal trouble when distributing | CC0/CC-BY only, tracked in `assets/LICENSES.md` |

## 12. Open items for the owner

1. **Product name** (needed before Phase 0 for the installer and app title).
2. What do you record real instruments with (audio interface, USB mic, built-in mic)? This decides how early ASIO matters.

Answered:
- MIDI keyboard: none for now → on-screen piano and musical typing are Phase 1 requirements.
- Godot version: **4.6**. Godot export targets 4.6; verify `.import`/`.tres` formats against it.
