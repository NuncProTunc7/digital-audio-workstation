# Third-party code

Code ported or adapted from other open-source projects. Each entry names the
source, the original license, and where the code lives in this repository.
See `AGENTS.md` §4 for the rules.

| Source project | Original file(s) | License | Used in |
|---|---|---|---|
| [vst3-rs](https://github.com/coupler-rs/vst3-rs) | `examples/gain.rs` (plugin skeleton: factory, processor, controller) | MIT OR Apache-2.0 | `crates/daw-test-plugin/src/lib.rs` (test-only plugin, not shipped) |

## Published algorithms (no code copied)

These public designs were implemented from scratch; listed for credit.

| Algorithm | Author / source | Status | Used in |
|---|---|---|---|
| Freeverb reverb structure (comb + all-pass tunings) | Jezar at Dreampoint | Public domain | `crates/daw-effects/src/reverb.rs` |
| Audio EQ Cookbook biquad formulas | Robert Bristow-Johnson | Public domain | `crates/daw-dsp/src/biquad.rs` |
| Topology-preserving-transform state-variable filter | Published DSP literature | Equations only | `crates/daw-dsp/src/filter.rs` |
| PolyBLEP anti-aliasing | Published DSP literature | Equations only | `crates/daw-dsp/src/oscillator.rs` |

Rust and npm dependencies are listed in `Cargo.lock` and `app/package-lock.json`
and keep their own licenses. Notable: symphonia (MPL-2.0, GPL-compatible) for
decoding audio files, rubato (MIT/Apache-2.0) for resampling, midly (Unlicense)
for MIDI files, roxmltree (MIT/Apache-2.0) and zip (MIT) for MusicXML, vorbis_rs
(BSD-3-Clause, bundling the BSD-licensed aoTuV/libvorbis and libogg) for OGG
encoding, OpenSheetMusicDisplay (BSD-3-Clause) for drawing sheet music, and vst3
(MIT/Apache-2.0, bindings generated from the MIT-licensed VST3 SDK 3.8) and
libloading (ISC) for hosting VST3 plugins.

Test audio in `crates/daw-audio/tests/fixtures/` was generated for this project
(CC0).
