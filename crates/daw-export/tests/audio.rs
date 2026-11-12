use daw_engine::AudioPool;
use daw_export::{
    AudioExport, AudioExportMode, AudioFormat, AudioPreview, AudioRegion, GAME_SAMPLE_RATE_HZ,
    PreviewSelection, export_audio, inspect_audio_export, preview_audio,
};
use daw_model::{Command, NoteInput, Project};

fn song() -> Project {
    let mut project = Project::default();
    for command in [
        Command::AddTempoChange {
            beats: 2.0,
            bpm: 75.0,
            ramp: false,
        },
        Command::AddTempoChange {
            beats: 6.0,
            bpm: 120.0,
            ramp: false,
        },
        Command::AddMarker {
            name: "Boss".into(),
            start_beats: 4.0,
        },
        Command::AddMarker {
            name: "Victory".into(),
            start_beats: 8.0,
        },
        Command::CreateClip {
            track_id: 2,
            start_beats: 4.0,
            length_beats: 4.0,
            name: None,
            notes: vec![NoteInput {
                pitch: 48,
                start_beats: 0.0,
                length_beats: 3.0,
                velocity: 100,
                chance: 100,
                id: None,
                guitar: None,
            }],
        },
    ] {
        command.apply(&mut project).expect("build song");
    }
    project
}

#[test]
fn marked_ogg_uses_tempo_map_and_checks_the_delivered_loop() {
    let dir = tempfile::tempdir().expect("directory");
    let project = song();
    let audio = AudioPool::in_temp_dir();
    let mut spec = AudioExport {
        path: dir.path().join("boss.ogg"),
        format: AudioFormat::Ogg,
        mode: AudioExportMode::Loop,
        region: AudioRegion::Marker {
            name: "Boss".into(),
        },
        tail_seconds: None,
        target_lufs: Some(-18.0),
        overwrite: false,
    };
    let check = inspect_audio_export(&project, &audio, &spec).expect("inspect");
    assert!(!spec.path.exists());
    assert!(!check.verified_encoded_file);
    let report = export_audio(&project, &audio, &spec).expect("export");
    let frames = (2.6 * f64::from(GAME_SAMPLE_RATE_HZ)).round() as usize;
    assert_eq!(report.loop_end_samples, Some(frames));
    assert_eq!(report.loop_start_samples, Some(0));
    assert_eq!(report.duration_seconds, 2.6);
    assert!(report.verified_encoded_file);
    assert!(report.integrated_lufs.is_some());
    assert_eq!(
        daw_audio::decode_file(&report.path)
            .expect("decode")
            .frames(),
        frames
    );
    let original = std::fs::read(&spec.path).expect("original");
    assert!(
        export_audio(&project, &audio, &spec)
            .expect_err("overwrite required")
            .to_string()
            .contains("overwrite")
    );
    spec.overwrite = true;
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Another program is reading the old asset and denies replacement.
        let reader = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&spec.path)
            .expect("open");
        assert!(export_audio(&project, &audio, &spec).is_err());
        assert_eq!(
            std::fs::read(&spec.path).expect("old file survived"),
            original
        );
        drop(reader);
    }
    spec.target_lufs = Some(-30.0);
    export_audio(&project, &audio, &spec).expect("replace");
    assert_ne!(std::fs::read(&spec.path).expect("replacement"), original);
    assert_eq!(
        std::fs::read_dir(dir.path()).expect("list").count(),
        1,
        "temporary files are removed"
    );
}

#[test]
fn a_one_shot_tail_excludes_the_following_music_and_playlist_reports_endpoints() {
    let dir = tempfile::tempdir().expect("directory");
    let project = song();
    let audio = AudioPool::in_temp_dir();
    let mut spec = AudioExport {
        path: dir.path().join("excerpt.wav"),
        format: AudioFormat::Wav,
        mode: AudioExportMode::OneShot,
        region: AudioRegion::Beats {
            start_beats: 0.0,
            end_beats: 4.0,
        },
        tail_seconds: Some(1.0),
        target_lufs: None,
        overwrite: false,
    };
    let report = export_audio(&project, &audio, &spec).expect("one shot");
    assert_eq!(report.duration_seconds, 3.6);
    assert!(
        report.integrated_lufs.is_none(),
        "the first note is beyond the selected region"
    );
    assert_eq!(report.loop_end_samples, None);
    spec.overwrite = true;
    spec.mode = AudioExportMode::Playlist;
    assert!(
        export_audio(&project, &audio, &spec).is_err(),
        "tail is only for one shot"
    );
    spec.tail_seconds = None;
    spec.region = AudioRegion::FullArrangement;
    let report = export_audio(&project, &audio, &spec).expect("playlist");
    assert!(report.integrated_lufs.is_some());
    assert!(report.opening_rms_dbfs.is_some());
    assert!(report.ending_rms_dbfs.is_some());
}

#[test]
fn preview_renders_a_playable_excerpt_across_the_loop_join() {
    let dir = tempfile::tempdir().expect("directory");
    let project = song();
    let audio = AudioPool::in_temp_dir();
    let preview = AudioPreview {
        path: dir.path().join("seam.wav"),
        format: AudioFormat::Wav,
        selection: PreviewSelection::LoopSeam {
            region: Some(AudioRegion::Marker {
                name: "Boss".into(),
            }),
        },
        overwrite: false,
    };
    let report = preview_audio(&project, &audio, &preview).expect("preview");
    assert_eq!(report.duration_seconds, 4.0);
    assert_eq!(report.preview_seam_samples, Some(88_200));
    assert_eq!(report.loop_end_samples, None);
    assert!(report.integrated_lufs.is_some());
    assert!(report.verified_encoded_file);
}
