//! Exports into a (fake) Godot project: the files loop seamlessly, decode,
//! and carry the import settings Godot needs.

use std::path::Path;

use daw_engine::offline::{TimedMessage, render_project};
use daw_engine::{AudioPool, EngineMessage};
use daw_export::{
    AudioFormat, GAME_SAMPLE_RATE_HZ, GodotExport, Section, export_to_godot, file_slug, render_loop,
};
use daw_model::{Command, EffectKind, NoteInput, Project};

/// A two-bar drum loop with a long reverb on the drums, so the end of the
/// loop rings over into its start.
fn song() -> Project {
    let mut p = Project::default();
    Command::RenameProject {
        name: "Boss Theme".into(),
    }
    .apply(&mut p)
    .expect("name");
    let hits = (0..8)
        .map(|i| NoteInput {
            pitch: if i % 2 == 0 { 36 } else { 38 },
            start_beats: f64::from(i),
            length_beats: 0.25,
            velocity: 110,
            id: None,
        })
        .collect();
    Command::CreateClip {
        track_id: 3,
        start_beats: 0.0,
        length_beats: 8.0,
        name: None,
        notes: hits,
    }
    .apply(&mut p)
    .expect("drums");
    Command::CreateClip {
        track_id: 2,
        start_beats: 0.0,
        length_beats: 8.0,
        name: None,
        notes: vec![NoteInput {
            pitch: 36,
            start_beats: 0.0,
            length_beats: 7.5,
            velocity: 100,
            id: None,
        }],
    }
    .apply(&mut p)
    .expect("bass");
    Command::AddEffect {
        track_id: Some(3),
        kind: EffectKind::Reverb,
        index: None,
    }
    .apply(&mut p)
    .expect("reverb");
    Command::SetLoop {
        enabled: Some(true),
        start_beats: Some(0.0),
        end_beats: Some(8.0),
    }
    .apply(&mut p)
    .expect("loop");
    p
}

fn godot_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tmp");
    std::fs::write(dir.path().join("project.godot"), "config_version=5\n").expect("godot");
    dir
}

fn spec(dir: &Path) -> GodotExport {
    GodotExport {
        project_dir: dir.to_owned(),
        folder: "music/boss".into(),
        name: "Boss Theme".into(),
        format: AudioFormat::Ogg,
        start_beats: None,
        end_beats: None,
        looped: true,
        stems: false,
        layers_resource: false,
        sections: Vec::new(),
        target_lufs: None,
    }
}

#[test]
fn wrapped_loop_matches_the_song_looping_in_the_app() {
    let p = song();
    let pool = AudioPool::in_temp_dir();
    let lap = render_loop(&p, &pool, 0.0, 8.0, true, Some(3));
    // 8 beats at 120 BPM = 4 s.
    assert_eq!(lap.stereo.len(), 4 * GAME_SAMPLE_RATE_HZ as usize * 2);
    // Ground truth: the drums looping in the engine; the second lap has the
    // first lap's reverb tail in it, exactly what wrapping recreates.
    let mut solo = p.clone();
    for t in &mut solo.tracks {
        t.mixer.mute = t.id != 3;
    }
    let live = render_project(
        &solo,
        &pool,
        vec![TimedMessage {
            at_seconds: 0.0,
            message: EngineMessage::Play,
        }],
        8.0,
        GAME_SAMPLE_RATE_HZ,
    );
    let second_lap = &live[lap.stereo.len()..2 * lap.stereo.len()];
    // Compare loudness in 20 ms windows: drum hits may land a sample apart,
    // which is inaudible but makes sample-by-sample comparison meaningless.
    let rms = |x: &[f32]| (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt();
    let window = 2 * 882;
    for (i, (a, b)) in lap
        .stereo
        .chunks(window)
        .zip(second_lap.chunks(window))
        .enumerate()
    {
        let (ra, rb) = (rms(a), rms(b));
        assert!(
            (ra - rb).abs() <= 0.1 * ra.max(rb) + 0.002,
            "window {i}: wrapped {ra} vs looping {rb}"
        );
    }
    // And it really does carry reverb into the start (not silence before
    // the first hit's attack).
    let unwrapped = render_loop(&p, &pool, 0.0, 8.0, false, Some(3));
    let tail_at_start: f32 = lap.stereo[..200].iter().map(|s| s.abs()).sum();
    let dry_start: f32 = unwrapped.stereo[..200].iter().map(|s| s.abs()).sum();
    assert!(tail_at_start > dry_start, "{tail_at_start} vs {dry_start}");
}

#[test]
fn exports_loop_stems_layers_and_sections_into_godot() {
    let dir = godot_project();
    let mut s = spec(dir.path());
    s.stems = true;
    s.layers_resource = true;
    s.sections = vec![
        Section {
            name: "Explore".into(),
            start_beats: 0.0,
            end_beats: 4.0,
        },
        Section {
            name: "Combat".into(),
            start_beats: 4.0,
            end_beats: 8.0,
        },
    ];
    s.target_lufs = Some(-16.0);
    let report = export_to_godot(&song(), &AudioPool::in_temp_dir(), &s).expect("export");
    assert_eq!(
        report.files,
        vec![
            "res://music/boss/boss_theme.ogg",
            "res://music/boss/boss_theme_bass.ogg",
            "res://music/boss/boss_theme_drums.ogg",
            "res://music/boss/boss_theme_explore.ogg",
            "res://music/boss/boss_theme_combat.ogg",
            "res://music/boss/boss_theme_layers.tres",
            "res://music/boss/boss_theme_sections.tres",
        ]
    );
    let lufs = report.integrated_lufs.expect("measured");
    assert!((lufs + 16.0).abs() < 0.6 || report.gain_db < 20.0, "{lufs}");

    let folder = dir.path().join("music/boss");
    let ogg = daw_audio::decode_file(&folder.join("boss_theme.ogg")).expect("decodes");
    assert_eq!(ogg.sample_rate_hz, GAME_SAMPLE_RATE_HZ);
    assert!((ogg.seconds() - 4.0).abs() < 0.01, "{}", ogg.seconds());

    let import = std::fs::read_to_string(folder.join("boss_theme.ogg.import")).expect("import");
    for line in [
        "importer=\"oggvorbisstr\"",
        "loop=true",
        "bpm=120",
        "beat_count=8",
        "bar_beats=4",
    ] {
        assert!(import.contains(line), "missing {line} in:\n{import}");
    }

    let layers = std::fs::read_to_string(folder.join("boss_theme_layers.tres")).expect("layers");
    assert!(layers.starts_with("[gd_resource type=\"AudioStreamSynchronized\""));
    assert!(layers.contains("path=\"res://music/boss/boss_theme_drums.ogg\""));
    assert!(layers.contains("stream_count = 2"));
    assert!(layers.contains("stream_1/stream = ExtResource(\"2\")"));

    let sections =
        std::fs::read_to_string(folder.join("boss_theme_sections.tres")).expect("sections");
    assert!(sections.contains("clip_count = 2"));
    assert!(sections.contains("clip_1/name = &\"Combat\""));
    let count_at = sections.find("clip_count").expect("count");
    let initial_at = sections.find("initial_clip").expect("initial");
    assert!(count_at < initial_at, "clip_count must come first");
    assert!(sections.contains("Vector2i(-1, -1)"));
}

#[test]
fn wav_loops_carry_a_smpl_chunk_and_explicit_loop_settings() {
    let dir = godot_project();
    let mut s = spec(dir.path());
    s.format = AudioFormat::Wav;
    export_to_godot(&song(), &AudioPool::in_temp_dir(), &s).expect("export");
    let folder = dir.path().join("music/boss");
    let bytes = std::fs::read(folder.join("boss_theme.wav")).expect("wav");
    let at = bytes
        .windows(4)
        .position(|w| w == b"smpl")
        .expect("smpl chunk");
    let read = |o: usize| u32::from_le_bytes(bytes[at + o..at + o + 4].try_into().expect("4"));
    // Loop count, then the loop's start and end (exclusive) frames.
    assert_eq!(read(8 + 28), 1);
    assert_eq!(read(8 + 36 + 8), 0);
    assert_eq!(read(8 + 36 + 12), 4 * GAME_SAMPLE_RATE_HZ);
    let import = std::fs::read_to_string(folder.join("boss_theme.wav.import")).expect("import");
    assert!(import.contains("edit/loop_mode=2"));
    assert!(import.contains(&format!("edit/loop_end={}", 4 * GAME_SAMPLE_RATE_HZ)));
    let decoded = daw_audio::decode_file(&folder.join("boss_theme.wav")).expect("decodes");
    assert_eq!(decoded.frames(), 4 * GAME_SAMPLE_RATE_HZ as usize);
}

#[test]
fn re_export_keeps_godots_uid() {
    let dir = godot_project();
    let s = spec(dir.path());
    export_to_godot(&song(), &AudioPool::in_temp_dir(), &s).expect("first");
    let import = dir.path().join("music/boss/boss_theme.ogg.import");
    // What Godot does on first import.
    let with_uid = std::fs::read_to_string(&import).expect("read").replace(
        "type=\"AudioStreamOggVorbis\"\n",
        "type=\"AudioStreamOggVorbis\"\nuid=\"uid://b4x2kq7n1m3p\"\n",
    );
    std::fs::write(&import, with_uid).expect("write");
    export_to_godot(&song(), &AudioPool::in_temp_dir(), &s).expect("again");
    assert!(
        std::fs::read_to_string(&import)
            .expect("read")
            .contains("uid=\"uid://b4x2kq7n1m3p\"")
    );
}

#[test]
fn refuses_non_godot_folders_and_escaping_paths() {
    let plain = tempfile::tempdir().expect("tmp");
    let err = export_to_godot(&song(), &AudioPool::in_temp_dir(), &spec(plain.path()))
        .expect_err("no project.godot");
    assert!(err.to_string().contains("project.godot"));
    let dir = godot_project();
    for bad in ["../outside", "C:/music", ""] {
        let mut s = spec(dir.path());
        s.folder = bad.into();
        assert!(
            export_to_godot(&song(), &AudioPool::in_temp_dir(), &s).is_err(),
            "{bad}"
        );
    }
    assert_eq!(file_slug("Boss Theme! (v2)"), "boss_theme_v2");
}
