# Nunc Pro Tune — Project Plan

**Status:** Phases 0–5 built, Phase 6 in progress (see §8). Decisions below are agreed with the owner.
**License:** GPL-3.0-or-later.

## 1. What we are building

**Nunc Pro Tune** is a free, open-source digital audio workstation (DAW) for making game music, with Claude able to operate every part of it.

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
| Audio I/O | **cpal** with **WASAPI** (standard Windows audio) for v1; **ASIO** added in Phase 6 | Owner records with a built-in/headset mic, which WASAPI handles well. ASIO matters once an audio interface is involved; Steinberg made the ASIO SDK GPLv3-compatible in Oct 2025. |
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
| Tempo convention | BPM counts the time signature's beat (quarter notes in 4/4, eighth notes in 6/8); the metronome clicks every beat | Simple and predictable; revisit if compound-meter users want dotted-quarter clicks. |
| Project file | One JSON file, `MySong.nptune` (versioned, validated on load). Recorded audio (Phase 4) goes in a `MySong Audio/` folder beside it | Human-readable, diff-able, easy for Claude to inspect; a single file is simpler to open, save, and email than a folder. Changed in Phase 2 from a `.daw` folder; owner approved. |

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
0. **Project edits vs. live actions.** Commands change the song (saved, undoable). Live actions change only what you hear right now: playing notes, play/stop, metronome, audio device. Both will be exposed to Claude in Phase 3; only Commands go in the undo history.
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
  daw-effects/      EQ, compressor, reverb, delay, chorus, distortion, limiter
  daw-export/       WAV/OGG/FLAC render, loop points, stems, Godot export
  daw-analysis/     loudness, peaks, spectrum summary, tempo/key estimate
  daw-control/      local JSON-RPC server; command schema registry
  daw-mcp/          MCP bridge binary (stdio ↔ control server)
  daw-cli/          headless render/analyze tool (used by tests and CI)
app/
  src/              React + TypeScript front end
  src-tauri/        Tauri shell, wires crates together
assets/             factory presets, drum kits, sample instruments (with licenses)
docs/               PLAN.md, design notes, user guide
```

## 4. Built-in instruments (v1)

All share one voice engine (polyphony, voice stealing, MIDI CC, pitch bend, sustain pedal) and a common preset format.

| Instrument | v1 design | Ideas to borrow from |
|---|---|---|
| **Synth** | **Built (Phase 1):** 2 band-limited oscillators (sine/saw/square/triangle) + sub + noise, state-variable filter (LP/BP/HP) with envelope, key tracking and LFO, amp + filter ADSR, vibrato, poly (16 voices) or mono with legato glide, sustain pedal, pitch bend, 10 presets. **Later:** third oscillator, ladder filter, second LFO, mod matrix, unison | LMMS TripleOscillator, Surge XT (GPL-3) filters/oscillators |
| **Keys** | (a) FM electric piano + organ models (no samples needed, later); (b) **built (Phase 6):** acoustic piano via the Sampler track with the **Salamander Grand Piano** (CC-BY 3.0) or any SFZ pack the user downloads; nothing is bundled, so the user's copy carries its own license | Dexed (GPL-3) FM ideas |
| **Drums** | **Built (Phase 1):** 16 synthesized pads on General MIDI notes 36–51 (kick, rim, 2 snares, clap, 6 toms, 3 hi-hats with choke, crash, ride), group levels, tune/decay controls, 3 kits. **Later:** sample pads, step sequencer | LMMS Kicker; classic 808/909 circuit models |
| **Bass** | **Built (Phase 1):** the Synth in mono mode with bass presets (Sub Bass, Fat Bass, Acid Bass). **Later:** drive, sampled electric bass via Sampler | Synth engine above with bass-focused presets |
| **Sampler** | **Built (Phase 6):** SFZ player (`daw-sampler`): key/velocity mapping, `<global>/<master>/<group>` inheritance, loop points, one-shots, release times, sustain pedal, `#define`/`#include`; release-trigger regions and round robins are skipped. Packs load in the background and big ones keep evenly spaced velocity layers to stay under 512 MB | sfizz (BSD-2) as reference (no code copied) |

### Playing without a MIDI keyboard
- **On-screen piano:** click or drag to play; shows held notes; octave and velocity controls.
- **Musical typing:** the computer keyboard plays notes (`A S D F…` = white keys, `W E T Y U…` = black keys, `Z`/`X` = octave down/up, `C`/`V` = velocity down/up), like GarageBand and Ableton.
- **Drum pads:** a clickable 4×4 pad grid, also mapped to keys.
- All three feed the same input path as a hardware MIDI keyboard, so they can be recorded to a track, quantized, and edited in the piano roll. A USB MIDI keyboard plugs into the same path later with no extra work.

Effects v1 (**built in Phase 2**): 3-band EQ with low cut, compressor, reverb (Freeverb-style), tempo-synced delay with ping-pong, chorus, distortion, look-ahead limiter. Every track has volume, pan, mute, solo, and an effect chain; the master bus has volume and its own chain. **Later:** sends/return buses, automation.

Only use sample content that is CC0, CC-BY, or similar, and record its license in `assets/LICENSES.md`.

## 5. Sheet music

| Feature | How |
|---|---|
| Show any MIDI track as notation | ✅ Quantize notes (sixteenth grid, one voice with chords, ties across bars, treble/bass/percussion clef) → MusicXML → render with OpenSheetMusicDisplay in the Sheet music tab |
| Import MusicXML (from MuseScore, online libraries, etc.) | ✅ `daw-notation` parses partwise scores (voices, chords, ties, backup/forward, tempo, meter, drum parts, `.mxl`) into one track per part |
| Import a **photo or PDF** of sheet music | Claude reads the image and writes MusicXML, then calls the `import_musicxml` tool. Dense scores may need a manual check. A dedicated optical music recognition (OMR) engine such as Audiveris (AGPL) could be added later. |
| Export notation | ✅ MusicXML (opens in MuseScore for printing); PDF later (print from MuseScore meanwhile) |
| MIDI files | ✅ Import/export `.mid` (type 1, drums on channel 10, General MIDI programs) |

## 6. Godot export

| Feature | Detail |
|---|---|
| Seamless loops | Render with a tail-wrap so reverb/delay tails at the end flow into the loop start |
| Loop points | OGG: write Godot's `loop` + `loop_offset` import settings; WAV: write a `smpl` chunk (Godot's "Detect from WAV" loop mode) |
| Stems | Export each track or bus as its own synchronized file |
| Adaptive music | Optionally generate a Godot resource (`.tres`) for `AudioStreamSynchronized` (layered stems) or `AudioStreamInteractive` (sections with transitions) |
| Loudness | Normalize to a target LUFS so every track in the game matches |
| Export to project | Point at a Godot project folder; files land in `res://music/...` |

**Built (Phase 5).** Formats were checked against the Godot 4.6 source (`docs/godot-formats.md`) and the exporter's output was imported by a real Godot 4.6 headless run: loops, BPM/beat counts, layer and section resources all load as intended. Loops are rendered with their reverb/echo tail folded back onto the start, at 44.1 kHz (Godot's mix rate), normalized to -16 LUFS by default without peaks above -1 dBFS. Re-exporting keeps the `uid` Godot assigned, so scenes referencing the music don't break.

## 7. Claude control (MCP)

**Built (Phase 3).** How it fits together:

- The app runs a **control server** (`daw-control`) on `127.0.0.1` at a random port. It writes the port and a random 64-character token to `control.json` in the app data folder (`%APPDATA%\io.github.nuncprotunc7.nuncprotune\` on Windows), and deletes it on exit.
- The installer ships **`npt-mcp.exe`** (`daw-mcp`), a stdio MCP server. Claude Desktop or Claude Code starts it; on every tool call it reads `control.json` and forwards the request. If the app isn't open, the tool says so in plain words.
- **Setup:** the app's **Claude** button (status bar) adds the bridge to Claude Desktop's config in one click (backing up the old file), and shows the `claude mcp add --scope user nunc-pro-tune -- "<path>"` line for Claude Code.
- **Tools are generated from the Command schema**, so every project edit the UI can make, Claude can make, with the same validation and undo. `batch` applies many Commands as one all-or-nothing undo step. Extra tools: `get_song`, `get_track`, `get_clip`, `describe_instruments`, `undo`, `redo`, `play`, `stop`, `locate`, `set_metronome`, `transport_status`, `analyze_mix`, `export_wav`, `save_project`, `open_project`, `new_project`.
- **Claude's ears** (`daw-analysis`): `analyze_mix` renders offline and reports integrated/short-term LUFS (EBU R128), true peak, RMS, crest, stereo correlation, six frequency-band shares, per-track levels and plain-language hints, plus an optional spectrogram image.
- The app refreshes when Claude changes something, shows a toast, and lists recent Claude actions in the Claude panel.

Planned tool groups (✅ = built; the rest arrive with their phase):

**Tool groups (generated from Commands):**

| Group | Example tools |
|---|---|
| Session | ✅ `get_song`, `new_project`, `open_project`, `save_project`, `set_tempo`, `set_time_signature`, `batch`, `undo`, `redo`; later `set_key` |
| Tracks | ✅ `get_track`, `add_track`, `remove_track`, `rename_track`, `move_track`, `set_instrument`, `load_preset`, `set_instrument_param` |
| Notes | ✅ `get_clip`, `create_clip`, `add_notes`, `edit_notes`, `remove_notes`, `quantize_notes`, `transpose_notes`, clip move/resize/duplicate; later `humanize` |
| Audio | ✅ `import_audio`, `add_audio_clip`, `set_audio_clip` (gain, fades), `split_clip`, `trim_clip_start`, `record_audio`, `stop_recording`; later `time_stretch` |
| Mixer | ✅ `set_track_mixer`, `set_master_volume`, `add_effect`, `set_effect_param`, `set_effect_enabled`; later `add_send`, `automate` |
| Transport | ✅ `play`, `stop`, `locate`, `set_loop`, `set_metronome`, `transport_status` |
| Notation | ✅ `import_musicxml`, `export_musicxml`, `import_midi`, `export_midi` |
| Analysis ("Claude's ears") | ✅ `analyze_mix` → LUFS, true peak, spectrum balance, stereo, per-track levels, hints, spectrogram |
| Export | ✅ `export_wav`, `export_godot` (loops, stems, layers, sections) |
| Sound design | ✅ `describe_instruments` (params, presets, drum pads, effects); later `save_preset` |

**Safety:** the control server listens on localhost only, needs a token stored in the user's app data folder, and every AI edit is undoable.

## 8. Phases

Each phase ends with a Windows installer you can download from GitHub Actions and test.

| Phase | Deliverable | You can… |
|---|---|---|
| **0. Skeleton** ✅ | Rust workspace, Tauri app opens, CI builds a Windows installer, headless render test | Install and open an empty app |
| **1. Make sound** ✅ | Audio device selection (WASAPI), play/stop transport, metronome, Synth (keys, pads, leads, basses) and Drum machine instruments with presets, **on-screen piano + computer-keyboard playing + clickable drum pads**, MIDI keyboard input | Play the Keys, Bass, and Drums tracks with your mouse, computer keyboard, or a MIDI keyboard; shape sounds with presets and sliders |
| **2. Arrange** ✅ | Timeline with clips (create, move between tracks, resize, duplicate, delete), loop region, **recording what you play into clips**, piano roll (add/move/resize notes, quantize, transpose, velocity), add/rename/delete tracks, mixer (volume, pan, mute, solo, 7 effects per track and on the master), save/open/new project files, `npt render` to WAV | Record and write a full instrumental track |
| **3. Claude** ✅ | Control server, MCP bridge, full tool list, analysis tools | Ask Claude to build or remix a track |
| **4. Record** ✅ | Audio tracks; import audio (phone m4a/AAC and ALAC, mp3, wav, flac, ogg) by button or drag-and-drop; microphone recording lined up with the beat; waveforms; trim, split, clip gain, fades, normalize; audio kept in a `Song Audio` folder beside the project; Claude can import, edit, record, and analyze audio | Record guitar/vocals (or bring them over from your phone) and mix them in |
| **5. Godot + notation** ✅ | Godot export (seamless OGG/WAV loops with tail wrap, stems, `AudioStreamSynchronized` layers, `AudioStreamInteractive` sections, loudness normalization, `.import` settings), MIDI import/export, MusicXML import/export (incl. `.mxl`), sheet music view | Drop music straight into your game; turn sheet music into tracks |
| **6. Expand** (in progress; see §13 for what comes first) | ✅ Automation lanes (volume, pan, any instrument or effect setting); ✅ SFZ sampler for recorded pianos/basses (memory-capped velocity layers); ✅ time-stretch (audio clips can follow tempo, pitch kept); next: CLAP then VST3 hosting, ASIO and multi-input recording | Use outside plugins, record a band |

Phase 3 (Claude) comes before recording on purpose: once Claude can drive the app, it can help test everything that follows.

### How audio works (Phase 4)

- **Audio tracks** are tracks whose instrument kind is `audio`; their clips carry an `AudioRegion` (file name, file length, offset into the file, gain, fades) instead of notes. Commands keep the two kinds apart (an audio clip can't move onto a synth track).
- **Files:** imports are decoded with symphonia and stored as 32-bit float WAV named `<original name>-<content hash>.wav` (importing the same file twice reuses it). A saved `Song.nptune` keeps its audio in `Song Audio/` next to it; before the first save, audio waits in the app-data folder (`unsaved-audio/`, cleaned after two weeks) and is copied over on save. Clips whose file is missing play silence and are marked in the UI.
- **Playback:** `daw-audio`'s `AudioPool` loads and resamples (rubato) each file once to the sound card's rate; the engine mixes audio regions sample-accurately with gain, fades, and a 1.5 ms declick at clip edges.
- **Recording:** the microphone stream (cpal input) mixes to mono, meters, and pushes into a lock-free ring; a writer thread streams it to WAV. Alignment uses timestamps: the output callback publishes which beat the speakers play at which instant, the input callback stamps when the first sample was captured, both on one app-wide clock. The take starts at the beat the performer heard; sound captured before beat 0 is trimmed. Looping pauses during audio takes. Measured in a loopback test: within 2 ms.
- **Tempo:** audio keeps its own speed. Changing tempo keeps clips on their start beat but doesn't stretch them (time-stretch is Phase 6).

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
| **Recording latency on Windows** (top risk) | Recording feels out of time | Built: timestamp-based alignment on one shared clock (loopback-tested to ~2 ms). If a particular device reports bad timestamps: a manual offset setting, then ASIO in Phase 6 |
| Scope creep | Never ships | Phase gates; each phase must produce something usable |
| Audio glitches (dropouts, clicks) | Unusable for recording | Real-time rules in §3, CPU meter, buffer-size setting, stress tests |
| Owner can't debug code | Bugs linger | Strong tests + CI, clear error messages, in-app diagnostic report |
| Sample library licensing | Legal trouble when distributing | CC0/CC-BY only, tracked in `assets/LICENSES.md` |

## 12. Owner answers

- **Name:** Nunc Pro Tune. No existing software or GitHub repo found with that name (Oct 2026 search; not a formal trademark clearance).
- **Recording gear:** **Bluetooth** headset (confirmed Oct 2026) → WASAPI is enough for v1, but Bluetooth adds latency the automatic alignment may not see; see §13 item 2. Recommend headphones while recording so the click and backing tracks don't bleed into the mic.
- **MIDI keyboard:** none for now → on-screen piano and musical typing are Phase 1 requirements.
- **Godot version:** **4.6**. Godot export targets 4.6; verify `.import`/`.tres` formats against it.
- **Claude clients:** Claude Desktop and Claude Code.

## 13. Next work, in order (agreed Oct 2026)

A design review found gaps that matter more than plugin hosting. Do these **before** CLAP/VST3 and ASIO, in this order. Each item lists what to build and how to know it's done. Items 1–4 were promised by this plan (§3 rule 1, §11 risks) but not built.

### A. Protect the work (do first)

**1. Autosave and crash recovery** (§3 rule 1 promised "crash-safe autosave").
- Every 60 s while the project is dirty, and on each `end_gesture` after 10 s of idle, write `<app data>/autosave/<project-key>.nptune` off the UI thread (atomic: temp file + rename). Audio already lives in the project's Audio folder or `unsaved-audio/`, so only the JSON is written.
- A clean exit or a save deletes the autosave. On launch, if one exists and is newer than its project file, offer "Recover unsaved work from <time>?" → open it as dirty, keeping the original path.
- Keep the last 3 manual saves as `Song.nptune.bak1..3` beside the file (rotating), so a bad save can be undone.
- Done when a test kills the host mid-edit, relaunches, and recovers the edit; and `.bak` files rotate.

**2. Recording latency for Bluetooth** (owner records with a Bluetooth headset; §11 top risk).
- Bluetooth output and mic add roughly 100–250 ms that device timestamps often don't report [Confidence: Med]. Also, a Bluetooth headset using its mic switches to the hands-free profile: low-quality mono, and output quality drops while recording. Tell the user; recommend wired earbuds for recording if they have them.
- Add a per-input-device **recording offset (ms)** setting (−500..+500), stored in app settings (not the project; it's about the hardware). `place_take` adds it.
- Add **Calibrate**: plays clicks through the output, records them through the mic, finds the delay by cross-correlation, and sets the offset. The user holds the headset mic near the earcup. Fallback: a "tap along" calibration (tap Space with 8 clicks; median offset).
- Detect Bluetooth by device name ("Hands-Free", "Headset", "AG Audio", "Bluetooth") and show a one-line warning in the Audio tab linking to Calibrate.
- Commands: none (device setting, live action). Expose `calibrate_latency` / `set_input_offset` to Claude as control tools.
- Done when the loopback test with an artificial 180 ms delay places takes within 5 ms after calibration.

**3. Sound card buffer size** (§11 glitch mitigation).
- Status-bar device menu: buffer 128/256/512/1024/2048 frames (`cpal::BufferSize::Fixed`, falling back to default if refused), saved in app settings. Show the resulting latency in ms. Suggest a bigger buffer when the CPU meter goes past 80% or underruns are counted.
- Done when changing it restarts the stream without losing the project, and the status bar reports the new latency.

**4. Diagnostic report** (§11 "owner can't debug").
- A **Copy diagnostic report** button (Claude panel and status bar) and a `diagnostic_report` control tool: app version, Windows version, output/input devices, sample rate, buffer, measured latency/offset, CPU peak, underrun count, MIDI devices, loaded sample packs, last 50 log lines (from a ring buffer filled off the audio thread), and recent errors. No file paths beyond the project name.
- Done when the report pastes as plain text and Claude can fetch it.

### B. Game music

**5. Intro, then loop.** Many game tracks play an intro once and then loop the body.
- No new model field: reuse the loop region and add an export option **"Play from the song start, loop the loop region"**. Render from 0 to loop end; fold the tail onto the loop start (not sample 0).
- Godot: OGG `.import` gets `loop_offset=<seconds of loop start>`; WAV `smpl` chunk gets `loop_begin=<frame>`. Verify with Godot 4.6 headless as in Phase 5.
- Done when an export test shows a seamless join at the loop start and Godot reports the offset.

**6. Section markers.** Adaptive "explore/combat" export is Claude-only today.
- Model: `Project.markers: Vec<Marker { id, name, start_beats }>` with Commands `AddMarker`, `MoveMarker`, `RenameMarker`, `RemoveMarker`/`RestoreMarker` (+ round-trip tests, preview.ts cases). A section runs from its marker to the next one (or the song end).
- UI: a marker strip under the ruler (double-click to add, drag, double-click name to rename). Godot dialog: "Sections from markers" builds an `AudioStreamInteractive`.
- Done when the dialog exports sections the user marked by hand.

**7. Orchestral and acoustic sounds.**
- Guide page and Sampler panel link to free packs: *VSCO 2 Community Edition* (strings, brass, woodwinds, percussion; CC0, verify before bundling), Salamander Grand Piano (CC-BY 3.0), and Sonatina Symphonic Orchestra (check license). Prefer an in-app downloader that fetches from the publisher (no redistribution) once one is confirmed reachable.
- Add composing-playbook recipes that use them (orchestral exploration, epic boss).

**8. Group tracks and shared effects (sends/buses).**
- Model: `Bus { id, name, mixer, effects }`; tracks gain `output: Master | Bus(id)` and `sends: Vec<Send { bus, level_db, pre_fader }>`. Engine: buses processed after tracks, before master, with preallocated buffers (keep `no_alloc.rs` green). Commands for add/remove/route/send.
- Godot stems can then be per bus (drums, music, ambience).

**9. Tempo changes mid-song** (low priority). A tempo map (`Vec<(beats, bpm)>`), beats↔seconds conversion everywhere that assumes one tempo (engine, recording alignment, export, analysis, MIDI/MusicXML tempo events).

### C. Smaller

**10. User presets:** `SavePreset` saves the track's instrument settings under a name in app data; listed after factory presets; Claude gets `save_preset`.
**11. Humanize:** `HumanizeNotes { clip_id, note_ids, timing_beats, velocity }` with a seeded random so undo/redo is exact.
**12. Key signature:** `Project.key` (`SetKey`), shown in the top bar, written to MusicXML/MIDI, and given to Claude in `get_song`.

### D. Getting builds to the owner

**13. Releases and auto-update.** A tag (`v0.x.y`) builds the installer and publishes a GitHub Release (no login needed, doesn't expire). Add `tauri-plugin-updater` with a signing key in repository secrets; the app checks on launch and offers "Update and restart". Unsaved work is protected by item 1.
**14. Code signing** (optional, costs money: certificate ~$100–400/yr or Azure Trusted Signing [Confidence: Med]). Removes the SmartScreen warning. Owner decides.

### Then
CLAP, then VST3 hosting, and ASIO, once the owner says which plugins or audio interface they'll use.
