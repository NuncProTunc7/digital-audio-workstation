# Mix analysis tab: design

Owner request (backlog, Section 9 of the tutorial): see on screen what `analyze_mix` tells the AI. The owner skipped the design questions; every choice below is the builder's call.

## What it is

A **Mix analysis** tab in the bottom panel, beside Mixer and Sounds. It runs the analysis the AI already uses (`daw_analysis::analyze`: an offline render, so it measures exactly what the song sounds like, including plugins and effects) when the owner clicks **Analyze**. It doesn't analyze live while playing; the Mixer already has live meters.

## Layout (top to bottom)

1. **Toolbar**: what to analyze (**Whole song**, or **Loop region** when Loop is on), the **Analyze** button ("Analyzing…" while it runs), and a line saying the song has changed since the last analysis, when it has.
2. **Three verdict cards**, green / amber / red against game-music targets:
   - **Loudness** (integrated LUFS): green -20 to -16 (game music), amber -23 to -20 or -16 to -13, red outside. Also shows the loudest moment (max short-term LUFS).
   - **Peaks** (true peak dBTP): green at or below -1, amber -1 to 0, red above 0 (may distort).
   - **Stereo width** (correlation): green 0.2 to 0.95, amber above 0.95 (almost mono) or 0 to 0.2 (very wide), red below 0 (parts cancel on phones).
3. **What to fix**: the plain-language hints, the same sentences the AI reads.
4. **Tone**: six bars, one per frequency band (Sub, Bass, Low mids, Mids, High mids, Highs) with their share of the energy.
5. **Tracks**: one bar per track for its share of the mix's energy, with its loudness and peak. Clicking a track selects it.
6. **Spectrogram**: the picture the AI sees (time left to right, low to high pitch bottom to top, brighter = louder), with bar numbers under it and 100 Hz / 1 kHz / 10 kHz labels beside it. Under it, **In words**: the AI's text description of it.

## How it works

- A new async Tauri command `analyze_mix` (renders, so off the window thread) calls `daw_control::handle` with `Request::Analyze` (per-track and spectrogram on) and returns the JSON the AI gets, including `spectrogram_png_base64`.
- The result lives in `App` so switching tabs keeps it; it is marked out of date once the project changes (compared by identity: every edit makes a new project object).
- Verdict thresholds live in a small pure module (`app/src/mixAnalysis.ts`) with unit tests.
- The browser preview backend returns a made-up analysis so the tab can be built and tested without Rust.
- No new Command: the tab only reads.

## Not now

Live loudness while playing, A/B comparison of two analyses, per-region analysis by dragging on the timeline.
