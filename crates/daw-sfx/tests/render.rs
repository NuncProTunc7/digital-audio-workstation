use daw_model::sound::{SoundCategory, SoundRecipe, Wave, generate};
use daw_sfx::{peak, render};

const RATE: u32 = 44_100;

#[test]
fn every_category_makes_a_clean_sound() {
    for c in SoundCategory::ALL {
        for seed in 0..20 {
            let recipe = generate(c, seed);
            let out = render(&recipe, RATE);
            assert!(!out.is_empty(), "{c:?} {seed}: empty");
            assert!(out.iter().all(|s| s.is_finite()), "{c:?} {seed}: NaN");
            assert!(out.len() <= 10 * RATE as usize, "{c:?} {seed}: too long");
            if c != SoundCategory::Random {
                assert!(peak(&out) > 0.01, "{c:?} {seed}: silent ({})", peak(&out));
            }
        }
    }
}

#[test]
fn most_random_sounds_are_audible() {
    let audible = (0..50)
        .filter(|&s| peak(&render(&generate(SoundCategory::Random, s), RATE)) > 0.01)
        .count();
    assert!(
        audible >= 40,
        "only {audible} of 50 random sounds are audible"
    );
}

#[test]
fn most_sounds_leave_headroom() {
    // Typical sounds peak below full scale, so few need turning down.
    let mut over = 0;
    let mut total = 0;
    for c in SoundCategory::ALL {
        for seed in 0..20 {
            total += 1;
            if peak(&render(&generate(c, seed), RATE)) > 1.0 {
                over += 1;
            }
        }
    }
    assert!(over * 10 <= total, "{over} of {total} sounds clip");
}

#[test]
fn rendering_is_deterministic() {
    let r = generate(SoundCategory::Explosion, 3);
    assert_eq!(render(&r, RATE), render(&r, RATE));
}

#[test]
fn volume_db_scales_the_output() {
    let r = generate(SoundCategory::Coin, 2);
    let loud = peak(&render(&r, RATE));
    let quiet = peak(&render(
        &SoundRecipe {
            volume_db: -6.0,
            ..r
        },
        RATE,
    ));
    let ratio = quiet / loud;
    assert!((ratio - 0.501).abs() < 0.01, "ratio {ratio}");
}

#[test]
fn length_follows_the_envelope() {
    let r = SoundRecipe::default();
    let expected = ((r.attack.powi(2) + r.sustain.powi(2) + r.decay.powi(2)) * 100_000.0) as usize;
    let n = render(&r, RATE).len();
    assert!(n.abs_diff(expected) <= 4, "{n} vs {expected}");
    let half = render(&r, RATE / 2).len();
    assert!(
        half.abs_diff(expected / 2) <= 4,
        "{half} vs {}",
        expected / 2
    );
}

#[test]
fn every_wave_sounds_different() {
    let waves = [
        Wave::Square,
        Wave::Saw,
        Wave::Sine,
        Wave::Triangle,
        Wave::Noise,
    ];
    let outs: Vec<Vec<f32>> = waves
        .iter()
        .map(|&wave| {
            render(
                &SoundRecipe {
                    wave,
                    ..SoundRecipe::default()
                },
                RATE,
            )
        })
        .collect();
    for i in 0..outs.len() {
        assert!(peak(&outs[i]) > 0.05, "{:?} silent", waves[i]);
        for j in i + 1..outs.len() {
            assert_ne!(outs[i], outs[j], "{:?} = {:?}", waves[i], waves[j]);
        }
    }
}

#[test]
fn crush_changes_the_sound() {
    let r = generate(SoundCategory::Laser, 1);
    let crushed = SoundRecipe { crush: 1.0, ..r };
    assert_ne!(render(&r, RATE), render(&crushed, RATE));
    assert!(render(&crushed, RATE).iter().all(|s| s.is_finite()));
}

#[test]
fn extreme_recipes_are_finite_and_bounded() {
    let started = std::time::Instant::now();
    let lows = SoundRecipe {
        wave: Wave::Noise,
        attack: 0.0,
        sustain: 0.0,
        punch: 0.0,
        decay: 0.0,
        base_freq: 0.0,
        freq_limit: 0.0,
        freq_ramp: -1.0,
        freq_dramp: -1.0,
        vib_strength: 0.0,
        vib_speed: 0.0,
        arp_mod: -1.0,
        arp_speed: 0.0,
        duty: 0.0,
        duty_ramp: -1.0,
        repeat_speed: 0.0,
        phaser_offset: -1.0,
        phaser_ramp: -1.0,
        lpf_freq: 0.0,
        lpf_ramp: -1.0,
        lpf_resonance: 0.0,
        hpf_freq: 0.0,
        hpf_ramp: -1.0,
        crush: 0.0,
        volume_db: -24.0,
    };
    let highs = SoundRecipe {
        wave: Wave::Square,
        attack: 1.0,
        sustain: 1.0,
        punch: 1.0,
        decay: 1.0,
        base_freq: 1.0,
        freq_limit: 1.0,
        freq_ramp: 1.0,
        freq_dramp: 1.0,
        vib_strength: 1.0,
        vib_speed: 1.0,
        arp_mod: 1.0,
        arp_speed: 1.0,
        duty: 1.0,
        duty_ramp: 1.0,
        repeat_speed: 1.0,
        phaser_offset: 1.0,
        phaser_ramp: 1.0,
        lpf_freq: 1.0,
        lpf_ramp: 1.0,
        lpf_resonance: 1.0,
        hpf_freq: 1.0,
        hpf_ramp: 1.0,
        crush: 1.0,
        volume_db: 6.0,
    };
    let falling = SoundRecipe {
        freq_ramp: -1.0,
        base_freq: 0.0,
        sustain: 1.0,
        ..SoundRecipe::default()
    };
    for (name, r) in [("lows", lows), ("highs", highs), ("falling", falling)] {
        for wave in [
            Wave::Square,
            Wave::Saw,
            Wave::Sine,
            Wave::Triangle,
            Wave::Noise,
        ] {
            let out = render(&SoundRecipe { wave, ..r }, RATE);
            assert!(out.iter().all(|s| s.is_finite()), "{name} {wave:?}");
            assert!(out.len() <= 10 * RATE as usize, "{name} {wave:?}");
        }
    }
    assert!(started.elapsed().as_secs_f32() < 10.0);
}

#[test]
fn starting_below_the_lowest_pitch_is_silent_and_says_why() {
    let r = SoundRecipe {
        base_freq: 0.3,
        freq_limit: 0.5,
        ..SoundRecipe::default()
    };
    assert!(peak(&render(&r, RATE)) < 0.001, "silent");
    assert!(
        r.silent_because()
            .expect("a reason")
            .contains("Lowest pitch")
    );
    // Above the limit, or with no limit, there's nothing to explain.
    for ok in [
        SoundRecipe {
            base_freq: 0.6,
            ..r
        },
        SoundRecipe {
            freq_limit: 0.0,
            ..r
        },
    ] {
        assert!(peak(&render(&ok, RATE)) > 0.01);
        assert_eq!(ok.silent_because(), None);
    }
}

/// Level of `freq_hz` in `x` (a single-bin DFT), relative to full scale.
fn level_at(x: &[f32], freq_hz: f64, rate: u32) -> f64 {
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, &s) in x.iter().enumerate() {
        let w = std::f64::consts::TAU * freq_hz * i as f64 / f64::from(rate);
        re += f64::from(s) * w.cos();
        im += f64::from(s) * w.sin();
    }
    2.0 * (re * re + im * im).sqrt() / x.len() as f64
}

#[test]
fn a_lower_device_rate_does_not_fold_high_notes_back_down() {
    // A steady square at the top pitch: sfxr's period is 99 oversampled
    // steps, so about 3564 Hz, with a strong 5th harmonic near 17.8 kHz.
    // A 22.05 kHz output can't carry that harmonic; simple interpolation
    // folds it back to about 4.2 kHz, a band-limited resampler drops it.
    let r = SoundRecipe {
        wave: Wave::Square,
        base_freq: 1.0,
        sustain: 0.5,
        decay: 0.0,
        ..SoundRecipe::default()
    };
    let f0 = 44_100.0 * 8.0 / 99.0;
    let low_rate = RATE / 2;
    let harmonic = 5.0 * f0;
    let folded = f64::from(low_rate) - harmonic;
    let base = render(&r, RATE);
    let at_harmonic = level_at(&base, harmonic, RATE);
    let already_there = level_at(&base, folded, RATE);
    let alias = level_at(&render(&r, low_rate), folded, low_rate);
    assert!(at_harmonic > 0.02, "the harmonic is there: {at_harmonic}");
    assert!(
        alias < already_there + at_harmonic * 0.1,
        "folded to {folded:.0} Hz: {alias} (was {already_there}, harmonic {at_harmonic})"
    );
}
