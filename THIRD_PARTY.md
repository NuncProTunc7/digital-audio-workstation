# Third-party code

Code ported or adapted from other open-source projects. Each entry names the
source, the original license, and where the code lives in this repository.
See `AGENTS.md` §4 for the rules.

| Source project | Original file(s) | License | Used in |
|---|---|---|---|
| _None yet_ | | | |

## Published algorithms (no code copied)

These public designs were implemented from scratch; listed for credit.

| Algorithm | Author / source | Status | Used in |
|---|---|---|---|
| Freeverb reverb structure (comb + all-pass tunings) | Jezar at Dreampoint | Public domain | `crates/daw-effects/src/reverb.rs` |
| Audio EQ Cookbook biquad formulas | Robert Bristow-Johnson | Public domain | `crates/daw-dsp/src/biquad.rs` |
| Topology-preserving-transform state-variable filter | Published DSP literature | Equations only | `crates/daw-dsp/src/filter.rs` |
| PolyBLEP anti-aliasing | Published DSP literature | Equations only | `crates/daw-dsp/src/oscillator.rs` |

Rust and npm dependencies are listed in `Cargo.lock` and `app/package-lock.json`
and keep their own licenses.
