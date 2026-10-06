//! Audio clips play at the right sample, from the right point in the file,
//! at the right level.

use std::sync::Arc;

use daw_audio::{AudioData, AudioPool};
use daw_engine::EngineMessage;
use daw_engine::offline::{TimedMessage, render_project};
use daw_model::{AudioRegion, Command, InstrumentKind, Project};

const SR: u32 = 48_000;
/// 120 BPM: one beat is half a second.
const FRAMES_PER_BEAT: usize = 24_000;

fn pool_with(file: &str, samples: Vec<f32>) -> Arc<AudioPool> {
    let pool = AudioPool::in_temp_dir();
    pool.insert(
        file,
        AudioData {
            sample_rate_hz: SR,
            channels: vec![samples],
        },
    );
    pool
}

fn region(file: &str, seconds: f64) -> AudioRegion {
    AudioRegion {
        file: file.into(),
        file_seconds: seconds,
        offset_seconds: 0.0,
        gain_db: 0.0,
        fade_in_seconds: 0.0,
        fade_out_seconds: 0.0,
    }
}

/// Default project with every instrument track removed, plus one audio
/// track (id 4) holding a clip of `audio` at `start_beats`.
fn project(audio: AudioRegion, start_beats: f64, length_beats: Option<f64>) -> Project {
    let mut p = Project::default();
    for id in [1, 2, 3] {
        Command::RemoveTrack { track_id: id }
            .apply(&mut p)
            .expect("remove");
    }
    Command::AddTrack {
        name: "Vox".into(),
        instrument: InstrumentKind::Audio,
        preset: None,
        index: None,
    }
    .apply(&mut p)
    .expect("track");
    Command::AddAudioClip {
        track_id: 4,
        start_beats,
        audio,
        length_beats,
        name: None,
    }
    .apply(&mut p)
    .expect("clip");
    p
}

fn play_from(beats: f64) -> Vec<TimedMessage> {
    vec![
        TimedMessage {
            at_seconds: 0.0,
            message: EngineMessage::Locate(beats),
        },
        TimedMessage {
            at_seconds: 0.0,
            message: EngineMessage::Play,
        },
    ]
}

fn left(stereo: &[f32]) -> Vec<f32> {
    stereo.chunks(2).map(|f| f[0]).collect()
}

#[test]
fn clip_starts_on_its_beat_and_plays_in_both_ears() {
    let pool = pool_with("dc.wav", vec![0.5; SR as usize]);
    let p = project(region("dc.wav", 1.0), 1.0, None);
    let out = render_project(&p, &pool, play_from(0.0), 1.2, SR);
    let first = out.chunks(2).position(|f| f[0] != 0.0).expect("clip plays");
    // Starts exactly on the beat, at the bottom of the declick ramp.
    assert_eq!(first, FRAMES_PER_BEAT);
    assert!(out[first * 2].abs() < 0.01);
    let settled = &out[(FRAMES_PER_BEAT + 500) * 2..][..2];
    assert!((settled[0] - 0.5).abs() < 1e-4, "{settled:?}");
    assert_eq!(settled[0], settled[1], "mono audio is centered");
}

#[test]
fn offset_and_gain_pick_the_right_samples() {
    // A ramp, so every sample is identifiable.
    let ramp: Vec<f32> = (0..SR).map(|i| i as f32 / SR as f32 * 0.5).collect();
    let pool = pool_with("ramp.wav", ramp.clone());
    let mut r = region("ramp.wav", 1.0);
    r.offset_seconds = 0.25;
    r.gain_db = -6.0;
    let p = project(r, 0.0, Some(1.0));
    let out = left(&render_project(&p, &pool, play_from(0.0), 0.6, SR));
    let gain = 10f32.powf(-6.0 / 20.0);
    for at in [1_000usize, 10_000, 20_000] {
        let want = ramp[12_000 + at] * gain;
        assert!(
            (out[at] - want).abs() < 1e-5,
            "at {at}: {} vs {want}",
            out[at]
        );
    }
    // The clip is one beat long, so it stops even though the file goes on.
    assert!(out[FRAMES_PER_BEAT + 10..].iter().all(|&s| s == 0.0));
}

#[test]
fn fades_shape_the_edges() {
    let pool = pool_with("dc.wav", vec![0.5; SR as usize]);
    let mut r = region("dc.wav", 1.0);
    r.fade_in_seconds = 0.1;
    r.fade_out_seconds = 0.2;
    let p = project(r, 0.0, Some(2.0));
    let out = left(&render_project(&p, &pool, play_from(0.0), 1.1, SR));
    // Halfway through the fade in: half level.
    assert!((out[2_400] - 0.25).abs() < 1e-3, "{}", out[2_400]);
    assert!((out[10_000] - 0.5).abs() < 1e-4);
    // Halfway through the fade out (clip ends at 1 s).
    assert!((out[48_000 - 4_800] - 0.25).abs() < 1e-3, "{}", out[43_200]);
}

#[test]
fn audio_ends_with_its_file() {
    let pool = pool_with("short.wav", vec![0.5; SR as usize / 4]);
    // 0.25 s of audio in a 2-beat (1 s) clip.
    let p = project(region("short.wav", 0.25), 0.0, Some(2.0));
    let out = left(&render_project(&p, &pool, play_from(0.0), 1.0, SR));
    assert!(out[11_000].abs() > 0.4);
    assert!(out[12_100..].iter().all(|&s| s == 0.0));
}

#[test]
fn starting_playback_mid_clip_plays_from_that_point() {
    let ramp: Vec<f32> = (0..SR).map(|i| i as f32 / SR as f32 * 0.5).collect();
    let pool = pool_with("ramp.wav", ramp.clone());
    let p = project(region("ramp.wav", 1.0), 0.0, None);
    // Beat 1 = 0.5 s into the file.
    let out = left(&render_project(&p, &pool, play_from(1.0), 0.1, SR));
    assert!((out[100] - ramp[24_100]).abs() < 1e-5);
}

#[test]
fn missing_audio_is_silent_not_fatal() {
    let pool = AudioPool::in_temp_dir();
    let p = project(region("nowhere-00000000.wav", 1.0), 0.0, None);
    let out = render_project(&p, &pool, play_from(0.0), 0.5, SR);
    assert!(out.iter().all(|&s| s == 0.0));
}

#[test]
fn output_clock_maps_device_time_to_beats() {
    let (engine, mut processor) = daw_engine::Engine::new(&Project::default(), SR);
    assert!(engine.clock().is_none());
    engine.locate(2.0);
    engine.play();
    let mut buf = vec![0.0; 960];
    processor.set_output_time(5_000_000_000);
    processor.process_interleaved(&mut buf, 2);
    let a = engine.clock().expect("anchor");
    assert_eq!(a.playback_ns, 5_000_000_000);
    assert!((a.position_beats - 2.0).abs() < 1e-9);
    assert!(a.playing);
    // Half a second later at 120 BPM: one beat on.
    assert!((a.beats_at(5_500_000_000) - 3.0).abs() < 1e-9);
}
