//! Every format a phone or computer is likely to hand us decodes to the
//! tone that was encoded: right length, right pitch, right level.

use std::path::{Path, PathBuf};

use daw_audio::{AudioData, AudioPool, decode_file, peaks, read_wav, resample, write_wav};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Frequency estimated from zero crossings of the first channel.
fn pitch_hz(d: &AudioData) -> f64 {
    let x = &d.channels[0];
    // Skip encoder warm-up at the edges.
    let (a, b) = (x.len() / 10, x.len() * 9 / 10);
    let crossings = x[a..b]
        .windows(2)
        .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
        .count();
    crossings as f64 * f64::from(d.sample_rate_hz) / (b - a) as f64
}

fn check_tone(name: &str, seconds: f64) {
    let d = decode_file(&fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert!(
        (d.seconds() - seconds).abs() < 0.08,
        "{name}: {} s",
        d.seconds()
    );
    let hz = pitch_hz(&d);
    assert!((hz - 440.0).abs() < 5.0, "{name}: {hz} Hz");
    // ffmpeg's sine is 1/8 full scale, the fixtures halve it, and the
    // stereo WAV loses another 3 dB in the mono-to-stereo upmix.
    let peak = d.peak();
    assert!((0.04..0.08).contains(&peak), "{name}: peak {peak}");
    assert!(d.channels.iter().flatten().all(|s| s.is_finite()));
}

#[test]
fn iphone_and_android_voice_memos_decode() {
    check_tone("voice-memo.m4a", 1.0);
    check_tone("alac.m4a", 1.0);
}

#[test]
fn mp3_flac_ogg_and_wav_decode() {
    check_tone("tone.mp3", 1.0);
    check_tone("tone.flac", 1.0);
    check_tone("tone.ogg", 1.0);
    check_tone("stereo16.wav", 0.25);
    let d = decode_file(&fixture("stereo16.wav")).expect("wav");
    assert_eq!(d.channels.len(), 2);
}

#[test]
fn junk_files_give_a_readable_error() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("notes.m4a");
    std::fs::write(&path, b"this is not audio").expect("write");
    let err = decode_file(&path).expect_err("junk");
    assert!(err.to_string().contains("isn't an audio file"), "{err}");
}

#[test]
fn resampling_keeps_pitch_and_length() {
    let d = decode_file(&fixture("tone.flac")).expect("flac");
    assert_eq!(d.sample_rate_hz, 44_100);
    let r = resample(d.clone(), 48_000).expect("resample");
    assert_eq!(r.sample_rate_hz, 48_000);
    assert!((r.seconds() - d.seconds()).abs() < 0.002, "{}", r.seconds());
    assert!((pitch_hz(&r) - 440.0).abs() < 2.0);
    assert!((r.peak() - d.peak()).abs() < 0.03);
}

#[test]
fn float_wav_round_trips_exactly() {
    let dir = tempfile::tempdir().expect("tmp");
    let d = AudioData {
        sample_rate_hz: 48_000,
        channels: vec![vec![0.0, 0.5, -0.25, 1.0], vec![0.1, 0.2, 0.3, 0.4]],
    };
    let path = dir.path().join("x.wav");
    write_wav(&path, &d).expect("write");
    assert_eq!(read_wav(&path).expect("read"), d);
}

#[test]
fn peaks_bracket_the_waveform() {
    let d = decode_file(&fixture("tone.flac")).expect("flac");
    let p = peaks(&d);
    assert_eq!(p.min_max.len(), 2 * 200);
    assert!(
        p.min_max
            .chunks(2)
            .skip(5)
            .all(|mm| mm[0] < -0.04 && mm[1] > 0.04)
    );
}

#[test]
fn import_copies_into_the_audio_folder_once() {
    let dir = tempfile::tempdir().expect("tmp");
    let pool = AudioPool::new(dir.path().join("scratch"));
    let a = pool.import(&fixture("voice-memo.m4a")).expect("import");
    assert!(
        a.file.starts_with("voice-memo-") && a.file.ends_with(".wav"),
        "{}",
        a.file
    );
    assert_eq!(a.name, "voice-memo");
    assert!(dir.path().join("scratch").join(&a.file).is_file());
    // Same audio again: same file.
    let b = pool.import(&fixture("voice-memo.m4a")).expect("again");
    assert_eq!(a.file, b.file);

    // Once the project is saved, gather moves it next to the project.
    let song_audio = dir.path().join("Song Audio");
    let missing = pool
        .gather([a.file.as_str(), "gone.wav"], &song_audio)
        .expect("gather");
    assert_eq!(missing, vec!["gone.wav".to_owned()]);
    assert!(song_audio.join(&a.file).is_file());

    let buffer = pool.buffer(&a.file, 48_000).expect("buffer");
    assert!((buffer.frames() as f64 / 48_000.0 - 1.0).abs() < 0.08);
    assert!(buffer.right.is_none(), "mono stays mono");
}

#[test]
fn missing_audio_is_reported_by_name() {
    let pool = AudioPool::new(tempfile::tempdir().expect("tmp").path().to_owned());
    let err = pool.buffer("lost.wav", 48_000).expect_err("missing");
    assert!(err.to_string().contains("lost.wav"));
}
