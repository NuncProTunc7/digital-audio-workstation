//! The sampler plays an SFZ pack: right recording for the note and
//! velocity, at the right pitch, releasing when the key goes up.

use std::path::Path;
use std::time::{Duration, Instant};

use daw_audio::{AudioData, write_wav};
use daw_model::{Instrument, InstrumentKind};
use daw_sampler::{PackStatus, pack_status};

const SR: f32 = 48_000.0;

fn sine(freq: f32, amp: f32, seconds: f32, sr: u32) -> AudioData {
    let n = (seconds * sr as f32) as usize;
    AudioData {
        sample_rate_hz: sr,
        channels: vec![
            (0..n)
                .map(|i| amp * (std::f32::consts::TAU * freq * i as f32 / sr as f32).sin())
                .collect(),
        ],
    }
}

/// A tiny pack: A4 (440 Hz) recorded soft and loud, at 44.1 kHz.
fn make_pack(dir: &Path) -> std::path::PathBuf {
    std::fs::create_dir_all(dir.join("samples")).expect("dir");
    write_wav(
        &dir.join("samples/a4 soft.wav"),
        &sine(440.0, 0.2, 2.0, 44_100),
    )
    .expect("soft");
    write_wav(
        &dir.join("samples/a4 loud.wav"),
        &sine(440.0, 0.8, 2.0, 44_100),
    )
    .expect("loud");
    let sfz = dir.join("Test Piano.sfz");
    std::fs::write(
        &sfz,
        "<control> default_path=samples/\n\
         <global> ampeg_release=0.1 amp_veltrack=0\n\
         <group> lovel=1 hivel=64\n<region> sample=a4 soft.wav lokey=a3 hikey=c6 pitch_keycenter=a4\n\
         <group> lovel=65 hivel=127\n<region> sample=a4 loud.wav lokey=a3 hikey=c6 pitch_keycenter=a4\n",
    )
    .expect("sfz");
    sfz
}

fn wait_ready(path: &Path) {
    let start = Instant::now();
    while !matches!(pack_status(path), PackStatus::Ready { .. }) {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{:?}",
            pack_status(path)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn render(inst: &mut dyn daw_instruments::InstrumentProcessor, seconds: f32) -> Vec<f32> {
    let n = (seconds * SR) as usize;
    let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
    for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
        inst.process(a, b);
    }
    l
}

fn pitch_hz(x: &[f32]) -> f32 {
    let crossings = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    crossings as f32 * SR / x.len() as f32
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

#[test]
fn plays_the_right_layer_at_the_right_pitch_and_releases() {
    let dir = tempfile::tempdir().expect("tmp");
    let sfz = make_pack(dir.path());
    let mut inst = Instrument::from_preset(InstrumentKind::Sampler, "Sample pack").expect("preset");
    inst.sample_pack = Some(sfz.display().to_string());
    let mut s = daw_instruments::create(&inst, SR);
    wait_ready(&sfz);

    // Soft A5: the soft recording an octave up (resampled from 44.1 kHz).
    s.note_on(81, 0.3);
    let held = render(s.as_mut(), 0.5);
    assert!(
        (pitch_hz(&held[4_800..]) - 880.0).abs() < 8.0,
        "{}",
        pitch_hz(&held[4_800..])
    );
    assert!(
        (peak(&held[4_800..]) - 0.2).abs() < 0.02,
        "soft {}",
        peak(&held[4_800..])
    );
    s.note_off(81);
    let after = render(s.as_mut(), 0.5);
    assert!(
        peak(&after[12_000..]) < 0.001,
        "released {}",
        peak(&after[12_000..])
    );

    // Loud middle C: the loud layer, a minor sixth down.
    s.note_on(60, 1.0);
    let loud = render(s.as_mut(), 0.5);
    assert!(
        (pitch_hz(&loud[4_800..]) - 261.6).abs() < 4.0,
        "{}",
        pitch_hz(&loud[4_800..])
    );
    assert!(
        (peak(&loud[4_800..]) - 0.8).abs() < 0.05,
        "loud {}",
        peak(&loud[4_800..])
    );

    // Outside the mapped keys: nothing.
    s.all_notes_off();
    s.note_on(30, 1.0);
    assert_eq!(peak(&render(s.as_mut(), 0.1)), 0.0);
}

#[test]
fn sustain_pedal_holds_notes() {
    let dir = tempfile::tempdir().expect("tmp");
    let sfz = make_pack(dir.path());
    let mut inst = Instrument::from_preset(InstrumentKind::Sampler, "Sample pack").expect("preset");
    inst.sample_pack = Some(sfz.display().to_string());
    let mut s = daw_instruments::create(&inst, SR);
    wait_ready(&sfz);
    s.control_change(64, 127);
    s.note_on(69, 1.0);
    render(s.as_mut(), 0.1);
    s.note_off(69);
    assert!(peak(&render(s.as_mut(), 0.3)[9_000..]) > 0.5, "pedal holds");
    s.control_change(64, 0);
    assert!(
        peak(&render(s.as_mut(), 0.5)[12_000..]) < 0.001,
        "pedal up releases"
    );
}

#[test]
fn missing_packs_are_silent() {
    let mut inst = Instrument::from_preset(InstrumentKind::Sampler, "Sample pack").expect("preset");
    inst.sample_pack = Some("/nowhere/at/all.sfz".into());
    let mut s = daw_instruments::create(&inst, SR);
    let start = Instant::now();
    while !matches!(
        pack_status(Path::new("/nowhere/at/all.sfz")),
        PackStatus::Failed(_)
    ) {
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    s.note_on(60, 1.0);
    assert_eq!(peak(&render(s.as_mut(), 0.1)), 0.0);
}
