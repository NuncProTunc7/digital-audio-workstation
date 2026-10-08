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
| Plugins | **VST3 first, then CLAP** (changed Oct 2026; owner approved) | Both are open (CLAP: MIT; VST3: MIT since SDK 3.8, Oct 2025). The free plugins the owner wants (Spitfire LABS, Valhalla Supermassive, Vital) ship as VST3 only, so VST3 comes first. |
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
- **Tools are generated from the Command schema**, so every project edit the UI can make, Claude can make, with the same validation and undo. `batch` applies many Commands as one all-or-nothing undo step. Extra tools: `get_song`, `get_track`, `get_clip`, `describe_instruments`, `undo`, `redo`, `play`, `stop`, `locate`, `set_metronome`, `set_count_in`, `transport_status`, `analyze_mix`, `export_wav`, `save_project`, `open_project`, `new_project`.
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
| Transport | ✅ `play`, `stop`, `locate`, `set_loop`, `set_metronome`, `set_count_in`, `transport_status` |
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
| **6. Expand** (in progress; see §13 for what comes first) | ✅ Automation lanes (volume, pan, any instrument or effect setting); ✅ SFZ sampler for recorded pianos/basses (memory-capped velocity layers); ✅ time-stretch (audio clips can follow tempo, pitch kept); next: VST3 then CLAP hosting, ASIO and multi-input recording | Use outside plugins, record a band |

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

A design review found gaps that matter more than plugin hosting. Do these **before** CLAP/VST3 and ASIO, in this order. Items 1–4 are done; the working order now continues in E below. Each item lists what to build and how to know it's done. Items 1–4 were promised by this plan (§3 rule 1, §11 risks) but not built.

### A. Protect the work (do first)

**1. Autosave and crash recovery** ✅ (built Oct 2026; §3 rule 1 promised it).
- `daw_control::autosave`: every 30 s, if the project is dirty and changed, the app writes `<app data>/autosave/<run>.nptune` plus a `<run>.json` describing it (project path, audio folder, time). Writes happen outside the session lock. Save, New, Open, a clean exit, or a clean session removes it.
- At launch, the newest autosave from another run is offered ("Recover your unsaved changes to …?"). It's skipped if its song file was saved after it, or if it's over two weeks old. Recovered work opens marked unsaved, keeps its original file path, and its unsaved recordings still play (the old run's audio folder is reused).
- Every save keeps the last 3 versions as `Song.nptune.bak1..3` beside the song.
- Verified: unit tests (crash → recover, stale, cleanup, backups) and live: edit, wait for the autosave, `kill -9` the app, relaunch, answer Yes; the song came back with its changes.

**2. Recording latency for Bluetooth** ✅ (built Oct 2026; the owner records with a Bluetooth headset; §11 top risk).
- Per-input-device **recording delay** (−500..+500 ms) in `<app data>/settings.json` (`daw_control::settings`), not the project, because it belongs to the hardware. `end_take` moves takes that much earlier.
- **Calibrate…** (`daw_control::calibrate`): plays only metronome clicks at 80 BPM. The user lets 4 go by and claps on the next 8 while the mic records. Each clap's distance from its beat is measured on the timeline the take would have been placed on, and the median is the delay. Claps were chosen over hearing the click itself because Bluetooth headsets cancel echo, which would hide a played-back click from their own mic. The result is kept only if at least 4 claps were heard within ±40 ms of each other.
- The Audio tab warns when the input's name looks like Bluetooth and no delay is set. Claude has `recording_delay` and `calibrate_recording`.
- Verified live through a PulseAudio loopback adding about 160 ms: takes landed 158 ms late, calibration measured 167 ms, and the next take landed 4.5 ms off the beat (the loopback itself drifts by ~10 ms).

**3. Sound card buffer size** ✅ (built Oct 2026; §11 glitch mitigation).
- Status-bar **Buffer** menu: Default or 128/256/512/1024/2048 frames, each with its delay in ms (`cpal::BufferSize::Fixed`, fitted into the device's supported range, falling back to the default if refused), saved in app settings (`buffer_frames`). Changing it restarts the stream on the same device and puts the playhead back; refused while recording.
- WASAPI doesn't report underruns, so the engine counts **overloads**: callbacks that took longer than the sound they produced (`EngineStatus::overloads`, plus `cpu_peak`). The status bar offers the next size up when CPU passes 80% or an overload happened in the last 15 s.

**4. Diagnostic report** ✅ (built Oct 2026; §11 "owner can't debug"). `daw_control::diagnostics`: an in-memory log (50 lines, 10 errors; never written from the audio thread) that the app fills with device, recording, file, Claude-request and UI errors; `report()` renders it with devices, buffer/latency, recording delay, CPU now/peak, overloads, MIDI, count-in, Claude's connection, the song's size and sample packs. Paths are cut to their last part. **Report** in the status bar, **Copy diagnostic report** in the Claude panel, and Claude's `diagnostic_report` tool.

Original spec:
- A **Copy diagnostic report** button (Claude panel and status bar) and a `diagnostic_report` control tool: app version, Windows version, output/input devices, sample rate, buffer, measured latency/offset, CPU peak, underrun count, MIDI devices, loaded sample packs, last 50 log lines (from a ring buffer filled off the audio thread), and recent errors. No file paths beyond the project name.
- Done when the report pastes as plain text and Claude can fetch it.

### B. Game music

**5. Intro, then loop** ✅ (built Oct 2026). `GodotExport::intro` renders from the song start to the loop end and folds the ring-out onto the loop start (`render_with_intro`); OGG `.import` gets `loop_offset`, WAV gets `edit/loop_begin` and the `smpl` loop start. Verified with Godot 4.6.1 headless: it imports `loop_offset=2.0, beat_count=8` and `loop_begin=88200, loop_end=176400` for a one-bar intro at 120 BPM. Godot dialog checkbox and the `intro` option of `export_godot`.

Original spec: many game tracks play an intro once and then loop the body.
- No new model field: reuse the loop region and add an export option **"Play from the song start, loop the loop region"**. Render from 0 to loop end; fold the tail onto the loop start (not sample 0).
- Godot: OGG `.import` gets `loop_offset=<seconds of loop start>`; WAV `smpl` chunk gets `loop_begin=<frame>`. Verify with Godot 4.6 headless as in Phase 5.
- Done when an export test shows a seamless join at the loop start and Godot reports the offset.

**6. Section markers** ✅ (built Oct 2026). `Project.markers` (also kept in saved versions) with `AddMarker`/`MoveMarker` (coalesces)/`RenameMarker`/`RemoveMarker`/`RestoreMarker`; `Project::sections()`; markers and sections appear in `get_song`. Marker strip under the ruler (double-click to add and name, drag, ✕ to delete) with dashed lines across the lanes; Godot dialog **Sections from markers** and `sections_from_markers` for `export_godot`.

Original spec: adaptive "explore/combat" export was Claude-only.
- Model: `Project.markers: Vec<Marker { id, name, start_beats }>` with Commands `AddMarker`, `MoveMarker`, `RenameMarker`, `RemoveMarker`/`RestoreMarker` (+ round-trip tests, preview.ts cases). A section runs from its marker to the next one (or the song end).
- UI: a marker strip under the ruler (double-click to add, drag, double-click name to rename). Godot dialog: "Sections from markers" builds an `AudioStreamInteractive`.
- Done when the dialog exports sections the user marked by hand.

**7. Orchestral and acoustic sounds.** ✅ (built Oct 2026)
- Built: `daw_control::library`, an in-app downloader that fetches SFZ instruments from their publishers on GitHub (sfzinstruments; nothing redistributed) into `<app data>/sample-library`: Salamander Grand Piano (CC-BY 3.0), Headroom Piano (CC-BY 4.0), Karoryfer/Bigcat cello (CC0), Smolken double bass (CC0), Ixox flute (CC-BY 4.0), Karoryfer war tuba (CC0). Sampler panel **Free instruments** list (Download / Use, credit lines) and Claude's `sample_library` / `download_sample_pack` tools. *VSCO 2 CE* was left out: its CE release has raw WAVs but no SFZ mapping. Sonatina's license is unclear, so it is also left out.
- Add composing-playbook recipes that use them (orchestral exploration, epic boss).

**8. Group tracks and shared effects (sends/buses)** ✅ (built Oct 2026).
- Model: `Project.buses: Vec<Bus { id, name, mixer }>` (in saved versions too); `Track.output: Option<bus id>` and `Track.sends: Vec<Send { bus_id, level_db, pre_fader }>`. Commands `AddBus`, `RemoveBus`/`RestoreBus` (restores routing), `RenameBus`, `SetBusMixer` (coalesces), `SetTrackOutput`, `SetSend` (adds or changes; coalesces), `RemoveSend`. Bus effects use the effect Commands with the bus id as `track_id`. Files drop routing to missing buses.
- Engine: `BusSlot`s with preallocated buffers, processed after the tracks and before the master; track output and sends (post-fader, or pre-fader respecting mute) per sample; bus peaks in the status. Routing changes rebuild tracks (and buses only when their layout changes); levels are live messages. `no_alloc.rs` covers buses and sends.
- UI: bus strips (pan, mute, rename, delete, members), **+ Bus**, per-track **Out** and **→ Bus** sends. Godot `bus_stems`: one stem per bus plus "other".

**9. Tempo changes mid-song** (future; owner agreed Oct 2026 to leave it for later). A tempo map (`Vec<(beats, bpm)>`), beats↔seconds conversion everywhere that assumes one tempo (engine, recording alignment, export, analysis, MIDI/MusicXML tempo events).

### C. Smaller

**10. User presets** ✅ (built Oct 2026): `daw_control::presets` keeps them in `<app data>/presets.json` (not a Command: it isn't song data); applying one is an ordinary `SetInstrument`. Preset list shows **Your presets** after the built-in ones, with **Save preset…** and delete. Claude: `save_preset`, `user_presets`, `load_user_preset`.
**11. Humanize** ✅ (built Oct 2026): `HumanizeNotes { clip_id, timing_beats ≤ 0.25, velocity ≤ 64, seed, note_ids }`, splitmix per note id so the same seed repeats; undo is the `EditNotes` inverse. Piano roll **Humanize** button (new seed per click).
**12. Key signature** ✅ (built Oct 2026): `Project.key: Option<Key { tonic, mode }>` with nine modes (`daw_model::music`), `SetKey`; top-bar Key and mode menus; MusicXML `<key><fifths><mode>` and MIDI key-signature meta; `get_song` gives the name and scale notes.

Owner requests (Oct 2026), done:
- ✅ **Count-in before recording.** Off / 1 bar (default) / 2 bars, in app settings (`count_in_bars`), next to the metronome; Claude has `set_count_in`. The engine's `CountIn` message starts the transport that many beats before the playhead with clicks only (even with the metronome off) and the song silent, so the clock stays continuous and recording alignment is unchanged. Notes played early land on the start; audio captured before the record point is trimmed off the take.
- ✅ **Trim note clips from the left.** Note clips got the left-edge handle audio clips had. Because trimming removes notes before the new start, the UI previews the drag and sends one `TrimClipStart` on release.

### D. Getting builds to the owner

**13. Releases and auto-update.** Releases ✅ (built Oct 2026): `.github/workflows/release.yml` publishes the installer for `v*` tags; `scripts/set-version.mjs` sets the version. Auto-update ✅ (built Oct 2026): `tauri-plugin-updater` checks `releases/latest/download/latest.json` 5 s after launch; a banner offers **Update and restart** (saving the song first). The public key is in `tauri.conf.json`; the private key was generated on the owner's PC (`%USERPROFILE%\.tauri\nunc-pro-tune-updater.key`, no password) and is stored as the `TAURI_SIGNING_PRIVATE_KEY` repository secret (added Oct 2026), so releases carry update files. Original plan: A tag (`v0.x.y`) builds the installer and publishes a GitHub Release (no login needed, doesn't expire). Add `tauri-plugin-updater` with a signing key in repository secrets; the app checks on launch and offers "Update and restart". Unsaved work is protected by item 1.
**14. Code signing** (optional, costs money: certificate ~$100–400/yr or Azure Trusted Signing [Confidence: Med]). Removes the SmartScreen warning. **Decided Oct 2026: not doing it**; the release page tells users to click More info → Run anyway.

### E. Owner's recommendations (Oct 2026)

The owner proposed these. Their priorities: finish items 3–4 first, then the drum step sequencer, snapshots with A/B listening, and the interactive game-music preview. **Working order:** 3, 4, E3 (step sequencer), E5 (snapshots/A-B), 5 (intro loop), 6 (markers), E1 (interactive preview), 10 (user presets), 8 (buses), E9 (sidechain), then the rest.

| # | Feature | Effort | Notes |
|---|---|---|---|
| E1 ✅ | **Interactive game-music preview**: Explore/Combat/Victory buttons while playing; layers fade and sections change on the beat, as Godot will | Large | Built Oct 2026. Live actions in `daw_control::game_preview`: the engine loops the section; `JumpAtNextBar` changes section on the next bar line (or at the section's end, whichever comes first) and keeps effect and release tails ringing over the cut (Godot's export crossfades 2 beats instead); `FadeLayer` ramps a track's level over N beats (`ResetLayers` ends it). 🎮 Game preview panel. |
| E2 ✅ | **Chord track and scale highlighting**: chords above the timeline, scale notes highlighted in the piano roll, shared with Claude | Medium–large | Built Oct 2026. `Project.chords: Vec<Chord { id, start_beats, root, quality, bass }>` (11 qualities, slash chords), `AddChord`/`SetChord`/`MoveChord` (coalesces)/`RemoveChord`/`RestoreChord`; chord strip under the markers with an edit menu; piano roll tints scale rows and marks the current chord's tones; `get_song` lists chords with notes and spans. |
| E3 ✅ | **Drum step sequencer**: grid of steps per drum pad, swing, velocity, rolls, probability hits | Medium | Built Oct 2026. `StepSequencer.tsx` edits ordinary notes (a drag or a step-menu change is one `Batch`). `Note.chance` (1–100 %) is rolled on the audio thread by a hash of the note's place, track and loop lap, so renders repeat exactly. `Clip.swing` (`SetClipSwing`) warps note times on the song's step grid in `build_sequence` and MIDI export; MusicXML stays straight. |
| E4 ✅ | **Arrangement variations**: quiet / normal / boss versions sharing material; linked clips | Large | Built Oct 2026 as linked clips + sections. `Clip.link` group id; `DuplicateClip { linked }`, `LinkClips`, `UnlinkClip` (+ undo-only `SetClipLink`, `SetClipNotes`). `Command::apply` copies a note edit (add/remove/edit/quantize/transpose/humanize/swing) to every linked partner and returns one undo step; split or trim-start unlinks. Ctrl+Shift+D, 🔗 on clips, **Unlink** in the piano roll; the guide shows building intensity versions as marked sections exported with "Sections from markers". |
| E5 ✅ | **Named snapshots and A/B listening**: save versions ("Before Claude's changes"), switch and compare at matched loudness | Medium | Built Oct 2026. `Project.snapshots` (inside the song file; their audio is gathered on save) with `TakeSnapshot`/`LoadSnapshot`/`RenameSnapshot`/`DeleteSnapshot` (+ `RestoreSnapshot`, `SetSongState` for undo). A/B is a live action (`daw_control::compare`): both versions are rendered and measured offline, the louder one is turned down on the master while it plays. Claude's first edit after 10 quiet minutes is batched with a "Before Claude's changes" version (one undo step). |
| E6 ✅ | **Track freeze**: render a heavy track to audio, unfreeze to edit | Medium–large | Built Oct 2026. `Track.frozen: Option<Frozen { file, fingerprint }>` (`FreezeTrack`/`UnfreezeTrack`); `daw_control::freeze_track` renders the track alone (fader flat, no sends, no master chain, volume/pan automation left live) to a WAV in the audio folder. A track plays frozen only while `Project::freeze_fingerprint` (FNV over instrument, clips, effects, shaping automation, tempo, meter) still matches, so edits fall back to live playback rather than stale audio; the engine skips the instrument and effects of frozen tracks. ❄ on track headers; `freeze_track` for Claude. |
| E7 ✅ | **Loop and export inspector**: audition the loop seam; flag missing audio, clipping, silence, mismatched stem lengths | Medium | Built Oct 2026. `daw_export::inspect` renders exactly what `export_to_godot` would and reports findings (problem/warning/ok): missing recordings, clipping and peaks above -1 dB, silence, seam clicks (jump vs. nearby motion) and level jumps, loops that aren't whole beats or bars, stem lengths, silent stems and sections. `inspect_export` for Claude; Godot dialog **Check**. Engine `AuditionSeam` plays the last bar into the first, repeating (**Listen to the loop point**). |
| E8 ✅ | **Take lanes and comping** | Large | Built Oct 2026. `Clip.muted` (`SetClipMuted`); `CompTake` mutes the clips overlapping one and unmutes it, with 10 ms fades for smooth joins (one `Batch`). A new recording over an old one is batched with `CompTake`, so the newest take plays and old ones stay muted. Timeline: **T<n>** shows overlapping clips on lanes, **✂** splits every take at the playhead, **▶ Use** comps a take; muted clips are dimmed, and the engine, MIDI and MusicXML skip them. |
| E9 ✅ | **Sidechain compression** | Medium–large | Built Oct 2026. `Effect.sidechain` (compressors; `SetEffectSidechain`). The engine renders every track's instrument first and keeps it as a key (`keys_left/right`, preallocated per track), then runs effects, so any track, bus, or master compressor can listen to any track (pre-effects, pre-fader). `EffectProcessor::process_keyed`. Mixer: **Listens to** on compressor cards. |
| E10 ✅ | **Searchable sound browser** with tags and favorites | Medium | Built Oct 2026. **Sounds** tab: built-in and user presets of every kind, search, mood chips (hand-tagged factory sounds; user presets tagged from their names), favorites (per computer, in the webview's storage), **Try on …** (load + a short demo phrase, undoable) or **+ New track** for other instrument kinds. |
| E11 ✅ | **Auditionable AI edits**: Claude offers variations, you audition each in context and keep one | Needs design | Built Oct 2026 on versions (E5): the composing playbook tells Claude to save each option as a version and restore the original; the A/B bar has a menu to switch B between options; Keep B is one undoable load. |

### Then: VST3 plugin hosting (in progress, Oct 2026)
Owner decisions (Oct 2026): VST3 first, CLAP later; ASIO waits until the owner has an audio interface; tempo changes mid-song (item 9) are a future item.

Design:
- **Crate `daw-plugins`** on the `vst3` bindings crate (MIT/Apache, generated from the MIT VST3 SDK 3.8). Loads a module (`.vst3` bundle's `Contents/x86_64-win/*.vst3`, or a single-file `.vst3`), reads its factory, and creates instances.
- **Finding plugins**: the standard folders (`%CommonProgramFiles%\VST3`, `%LOCALAPPDATA%\Programs\Common\VST3`). A bundle's `moduleinfo.json` is read without loading code; otherwise the app runs itself with `--scan-vst3 <path>` in a child process, so a plugin that crashes while being scanned can't take the app down. Results are cached per computer (path + modified time).
- **Model**: `InstrumentKind::Plugin` and `EffectKind::Plugin` with a `PluginRef { uid, name, vendor, params: {id → normalized}, state: base64 }`. Commands: `LoadPlugin` (instrument), add_effect with a plugin, `SetPluginParam` (coalesces, so a knob drag in the plugin's window is one undo step). Edits in the plugin's own window reach us through `IComponentHandler::performEdit` and become `SetPluginParam` Commands, so Claude sees them and undo works. The opaque state blob (non-parameter state such as a chosen sample set) is refreshed from the live plugin when the song is saved; it is not undoable.
- **Engine**: a plugin instance implements `InstrumentProcessor`/`EffectProcessor`; the engine rebuilds it only when the plugin (uid) changes. Our side of `process` uses preallocated event and parameter queues (no allocation); what the plugin does inside its own `process` is outside our control. Offline renders (export, freeze, A/B) create their own instances from the saved state.
- **Threads**: plugins are created and their windows opened on the app's main thread (JUCE-based plugins such as Spitfire LABS require it).
- **Editor**: the plugin's own window (`IPlugView`) in a native window the app opens; it resizes when the plugin asks.
- **Claude**: `plugins` (installed list), `load_plugin`, `plugin_params` (names, current values and display text), `set_plugin_param`.
- **Crashes**: plugins run inside the app at first. A plugin crash closes the app, but autosave and crash recovery keep the song; on restart the app offers to open it with that plugin switched off. Running plugins in a separate process is a later step.
- **Tests**: a tiny test plugin crate (synth + gain) built by the tests, so no downloads are needed.

Order: (1) load + scan + test plugin ✅, (2) instruments playing in the engine and offline ✅, (3) plugin windows ✅, (4) parameters, state and Claude tools ✅, (5) effects ✅, (6) crash-safe restart. CLAP follows, reusing the same model and engine path.

Built (Oct 2026, steps 1–4): `daw-plugins` (module loading, `moduleinfo.json` or child-process scan via the app's `--scan-vst3`, `Instance`/`PluginProcessor`, Win32 plugin windows in `editor.rs`, `main_thread` runner installed by the Tauri shell). The engine keeps live plugins across track-graph rebuilds and, in the app, loads new ones on the main thread in the background (`ReplaceInstrument` swaps them in; `on_plugin_loaded` tells `daw-control`, which records the parameter list). Plugin tracks: **Plugin** menu in the instrument panel, **Open plugin window**, searchable sliders. Plugin effects (step 5): `EffectKind::Plugin` with `Effect.plugin`, `AddPluginEffect` (Claude: `add_plugin_effect`), `SetPluginParams` addresses an effect by effect_id + owner; the engine keeps live effect plugins by effect id (`ReplaceEffect` swaps in background loads) and sends all plugin parameter changes in one pass after any rebuilds; mixer **Plugins** group in **+ Add effect…**, **Open plugin window** on the card. Verified end to end against the test plugin; not yet with a commercial plugin (none installed on the dev machine).
