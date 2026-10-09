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
            chance: 100,
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
            chance: 100,
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
        intro: false,
        stems: false,
        bus_stems: false,
        layers_resource: false,
        sections: Vec::new(),
        target_lufs: None,
    }
}

#[test]
fn intro_plays_once_then_the_loop_region_repeats_seamlessly() {
    // Loop bars 2 (beats 4..8); bar 1 is the intro.
    let mut p = song();
    Command::SetLoop {
        enabled: Some(true),
        start_beats: Some(4.0),
        end_beats: Some(8.0),
    }
    .apply(&mut p)
    .expect("loop");
    let pool = AudioPool::in_temp_dir();
    let file = daw_export::render_with_intro(&p, &pool, 0.0, 4.0, 8.0, true, Some(3));
    let sr = GAME_SAMPLE_RATE_HZ as usize;
    // 8 beats at 120 BPM = 4 s, looping from 2 s.
    assert_eq!(file.stereo.len(), 4 * sr * 2);
    assert_eq!(file.loop_start_frame(), 2 * sr);
    // Ground truth: the app playing from the start with the loop on. Its
    // first 2 s are the intro; its third and fourth second the first lap,
    // seconds 4-6 the second lap (with the first lap's reverb in it).
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
        6.0,
        GAME_SAMPLE_RATE_HZ,
    );
    let rms = |x: &[f32]| (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt();
    let close = |a: &[f32], b: &[f32], what: &str| {
        for (i, (x, y)) in a.chunks(2 * 882).zip(b.chunks(2 * 882)).enumerate() {
            let (rx, ry) = (rms(x), rms(y));
            assert!(
                (rx - ry).abs() <= 0.1 * rx.max(ry) + 0.002,
                "{what} window {i}: file {rx} vs app {ry}"
            );
        }
    };
    // The intro is untouched: the loop's tail is not folded into it.
    close(&file.stereo[..2 * sr * 2], &live[..2 * sr * 2], "intro");
    // The looping part sounds like a later lap, ring-out included.
    close(
        &file.stereo[2 * sr * 2..],
        &live[4 * sr * 2..6 * sr * 2],
        "loop",
    );

    // Godot is told where the loop starts.
    let dir = godot_project();
    let mut ogg = spec(dir.path());
    ogg.intro = true;
    let report = export_to_godot(&p, &pool, &ogg).expect("export");
    assert!((report.loop_start_seconds - 2.0).abs() < 1e-9);
    let import = std::fs::read_to_string(dir.path().join("music/boss/boss_theme.ogg.import"))
        .expect("import");
    assert!(
        import.contains("loop=true") && import.contains("loop_offset=2.0"),
        "{import}"
    );
    assert!(import.contains("beat_count=8"), "{import}");
    let mut wav = spec(dir.path());
    wav.intro = true;
    wav.format = AudioFormat::Wav;
    export_to_godot(&p, &pool, &wav).expect("export wav");
    let import = std::fs::read_to_string(dir.path().join("music/boss/boss_theme.wav.import"))
        .expect("import");
    assert!(
        import.contains(&format!("edit/loop_begin={}", 2 * sr)),
        "{import}"
    );
    assert!(
        import.contains(&format!("edit/loop_end={}", 4 * sr)),
        "{import}"
    );
    let bytes = std::fs::read(dir.path().join("music/boss/boss_theme.wav")).expect("wav");
    let smpl = bytes
        .windows(4)
        .position(|w| w == b"smpl")
        .expect("smpl chunk");
    let loop_begin = u32::from_le_bytes(
        bytes[smpl + 8 + 36 + 8..smpl + 8 + 36 + 12]
            .try_into()
            .expect("4"),
    );
    assert_eq!(loop_begin as usize, 2 * sr);
}

/// For checking with a real Godot: `NPT_GODOT_DIR=<godot project> cargo test
/// -p daw-export --test godot -- --ignored` writes an intro loop there as
/// OGG and WAV (`music/intro_ogg.ogg`, `music/intro_wav.wav`).
#[test]
#[ignore = "needs a Godot project folder in NPT_GODOT_DIR"]
fn writes_intro_loops_for_a_real_godot() {
    let Some(dir) = std::env::var_os("NPT_GODOT_DIR") else {
        return;
    };
    let mut p = song();
    Command::SetLoop {
        enabled: Some(true),
        start_beats: Some(4.0),
        end_beats: Some(8.0),
    }
    .apply(&mut p)
    .expect("loop");
    let pool = AudioPool::in_temp_dir();
    for (name, format) in [
        ("intro_ogg", AudioFormat::Ogg),
        ("intro_wav", AudioFormat::Wav),
    ] {
        let mut s = spec(Path::new(&dir));
        s.folder = "music".into();
        s.name = name.into();
        s.format = format;
        s.intro = true;
        export_to_godot(&p, &pool, &s).expect("export");
    }
}

#[test]
fn the_inspector_passes_a_clean_loop_and_flags_problems() {
    use daw_export::{Level, inspect};
    let pool = AudioPool::in_temp_dir();
    let dir = godot_project();
    let mut s = spec(dir.path());
    s.target_lufs = Some(-16.0);
    s.stems = true;
    let report = inspect(&song(), &pool, &s).expect("inspect");
    assert!(report.ready(), "{:#?}", report.findings);
    let says = |r: &daw_export::Inspection, level: Level, text: &str| {
        r.findings
            .iter()
            .any(|f| f.level == level && f.message.contains(text))
    };
    assert!(
        says(&report, Level::Ok, "joins smoothly"),
        "{:#?}",
        report.findings
    );
    assert!(
        says(&report, Level::Ok, "exactly as long"),
        "{:#?}",
        report.findings
    );

    // Too loud with loudness matching off: it clips.
    let mut loud = song();
    Command::SetMasterVolume { volume_db: 6.0 }
        .apply(&mut loud)
        .expect("loud");
    for t in [1, 2, 3] {
        Command::SetTrackMixer {
            track_id: t,
            volume_db: Some(6.0),
            pan: None,
            mute: None,
            solo: None,
        }
        .apply(&mut loud)
        .expect("track");
    }
    s.target_lufs = None;
    let report = inspect(&loud, &pool, &s).expect("inspect");
    assert!(
        says(&report, Level::Problem, "clips") || says(&report, Level::Warning, "Peaks reach"),
        "{:#?}",
        report.findings
    );

    // A recording that isn't there, and a region with nothing in it.
    let mut broken = song();
    Command::AddTrack {
        name: "Vox".into(),
        instrument: daw_model::InstrumentKind::Audio,
        preset: None,
        index: None,
    }
    .apply(&mut broken)
    .expect("audio track");
    let vox = broken.tracks[3].id;
    Command::AddAudioClip {
        track_id: vox,
        start_beats: 0.0,
        audio: daw_model::AudioRegion {
            file: "gone.wav".into(),
            file_seconds: 2.0,
            offset_seconds: 0.0,
            gain_db: 0.0,
            fade_in_seconds: 0.0,
            fade_out_seconds: 0.0,
            source_bpm: None,
        },
        length_beats: None,
        name: None,
    }
    .apply(&mut broken)
    .expect("clip");
    s.start_beats = Some(32.0);
    s.end_beats = Some(40.0);
    let report = inspect(&broken, &pool, &s).expect("inspect");
    assert!(!report.ready());
    assert!(
        says(&report, Level::Problem, "gone.wav"),
        "{:#?}",
        report.findings
    );
    assert!(
        says(&report, Level::Problem, "silent"),
        "{:#?}",
        report.findings
    );
}

#[test]
fn stems_can_be_one_per_bus() {
    let mut p = song();
    Command::AddBus {
        name: "Drums".into(),
    }
    .apply(&mut p)
    .expect("bus");
    let bus = p.buses[0].id;
    Command::SetTrackOutput {
        track_id: 3,
        bus_id: Some(bus),
    }
    .apply(&mut p)
    .expect("route");
    let dir = godot_project();
    let mut s = spec(dir.path());
    s.stems = true;
    s.bus_stems = true;
    let pool = AudioPool::in_temp_dir();
    let report = export_to_godot(&p, &pool, &s).expect("export");
    // The drums bus, and the bass (straight to the master) as "other".
    assert!(
        report
            .files
            .contains(&"res://music/boss/boss_theme_drums.ogg".to_owned()),
        "{:?}",
        report.files
    );
    assert!(
        report
            .files
            .contains(&"res://music/boss/boss_theme_other.ogg".to_owned()),
        "{:?}",
        report.files
    );
    assert!(
        !report.files.iter().any(|f| f.ends_with("_bass.ogg")),
        "{:?}",
        report.files
    );
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

#[test]
fn muted_and_unsoloed_tracks_get_no_stem() {
    use daw_export::{Level, inspect};
    let mixer = |p: &mut Project, track_id, mute, solo| {
        Command::SetTrackMixer {
            track_id,
            volume_db: None,
            pan: None,
            mute,
            solo,
        }
        .apply(p)
        .expect("mixer");
    };
    let dir = godot_project();
    let mut s = spec(dir.path());
    s.stems = true;
    s.layers_resource = true;

    // A muted bass is not in the mix, so Godot must not play it either.
    let mut muted = song();
    mixer(&mut muted, 2, Some(true), None);
    let report = export_to_godot(&muted, &AudioPool::in_temp_dir(), &s).expect("export");
    assert_eq!(
        report.files,
        vec![
            "res://music/boss/boss_theme.ogg",
            "res://music/boss/boss_theme_drums.ogg",
            "res://music/boss/boss_theme_layers.tres",
        ]
    );
    let layers = std::fs::read_to_string(dir.path().join("music/boss/boss_theme_layers.tres"))
        .expect("layers");
    assert!(layers.contains("stream_count = 1"), "{layers}");
    let check = inspect(&muted, &AudioPool::in_temp_dir(), &s).expect("inspect");
    assert!(
        check.findings.iter().any(|f| f.level == Level::Warning
            && f.message.contains("muted")
            && f.message.contains("Bass")),
        "{:#?}",
        check.findings
    );

    // Soloing the drums leaves the bass out the same way.
    let mut soloed = song();
    mixer(&mut soloed, 3, None, Some(true));
    let report = export_to_godot(&soloed, &AudioPool::in_temp_dir(), &s).expect("export");
    assert!(
        !report.files.iter().any(|f| f.contains("bass")),
        "{:?}",
        report.files
    );
}
