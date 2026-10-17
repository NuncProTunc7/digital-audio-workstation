use daw_export::{SoundExport, export_sounds};
use daw_model::sound::{SoundCategory, SoundRecipe};
use daw_model::{Command, Project};

fn godot_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tmp");
    std::fs::write(dir.path().join("project.godot"), "config_version=5\n").expect("godot");
    dir
}

fn add(
    p: &mut Project,
    name: &str,
    recipe: Option<SoundRecipe>,
    category: Option<SoundCategory>,
) -> daw_model::Id {
    Command::AddSound {
        name: Some(name.into()),
        recipe,
        category,
        seed: Some(2),
    }
    .apply(p)
    .expect("add");
    p.sounds.last().expect("sound").id
}

fn spec(dir: &std::path::Path) -> SoundExport {
    SoundExport {
        project_dir: dir.to_owned(),
        folder: "sfx".into(),
        sound_ids: None,
    }
}

/// (channels, sample rate, bits, frames) from a WAV header.
fn wav_info(bytes: &[u8]) -> (u16, u32, u16, usize) {
    assert_eq!(&bytes[0..4], b"RIFF");
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().expect("4"));
    let channels = u16_at(22);
    let bits = u16_at(34);
    let data = u32_at(40) as usize;
    (
        channels,
        u32_at(24),
        bits,
        data / (usize::from(channels) * usize::from(bits / 8)),
    )
}

fn peak_dbfs(bytes: &[u8]) -> f64 {
    let data = u32::from_le_bytes(bytes[40..44].try_into().expect("4")) as usize;
    let peak = bytes[44..44 + data]
        .chunks(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]).unsigned_abs())
        .max()
        .unwrap_or(0);
    20.0 * (f64::from(peak) / 32768.0).log10()
}

#[test]
fn writes_one_wav_per_sound() {
    let dir = godot_project();
    let mut p = Project::default();
    let coin = add(&mut p, "coin", None, Some(SoundCategory::Coin));
    add(&mut p, "jump", None, Some(SoundCategory::Jump));
    let report = export_sounds(&p, &spec(dir.path())).expect("export");
    let paths: Vec<&str> = report.files.iter().map(|f| f.res_path.as_str()).collect();
    assert_eq!(paths, ["res://sfx/coin.wav", "res://sfx/jump.wav"]);
    let bytes = std::fs::read(dir.path().join("sfx/coin.wav")).expect("coin");
    let (channels, rate, bits, frames) = wav_info(&bytes);
    assert_eq!((channels, rate, bits), (2, 44_100, 16));
    let recipe = p.sound(coin).expect("coin").recipe;
    assert_eq!(frames, daw_sfx::render(&recipe, 44_100).len());
    assert!((report.files[0].seconds - frames as f64 / 44_100.0).abs() < 1e-6);
    assert_eq!(report.files[0].sound_id, coin);
}

#[test]
fn exports_only_the_chosen_sounds() {
    let dir = godot_project();
    let mut p = Project::default();
    add(&mut p, "coin", None, Some(SoundCategory::Coin));
    let jump = add(&mut p, "jump", None, Some(SoundCategory::Jump));
    let report = export_sounds(
        &p,
        &SoundExport {
            sound_ids: Some(vec![jump]),
            ..spec(dir.path())
        },
    )
    .expect("export");
    assert_eq!(report.files.len(), 1);
    assert!(!dir.path().join("sfx/coin.wav").exists());
    assert!(dir.path().join("sfx/jump.wav").exists());
}

#[test]
fn refuses_a_folder_without_project_godot() {
    let dir = tempfile::tempdir().expect("tmp");
    let mut p = Project::default();
    add(&mut p, "coin", None, Some(SoundCategory::Coin));
    let err = export_sounds(&p, &spec(dir.path())).expect_err("not godot");
    assert!(err.to_string().contains("project.godot"), "{err}");
}

#[test]
fn refuses_folders_that_escape_the_project() {
    let dir = godot_project();
    let mut p = Project::default();
    add(&mut p, "coin", None, Some(SoundCategory::Coin));
    for folder in ["../x", "sfx/../../x", "C:/x"] {
        assert!(
            export_sounds(
                &p,
                &SoundExport {
                    folder: folder.into(),
                    ..spec(dir.path())
                }
            )
            .is_err(),
            "{folder}"
        );
    }
}

#[test]
fn turns_down_a_clipping_sound() {
    let dir = godot_project();
    let mut p = Project::default();
    let mut loud = daw_model::sound::generate(SoundCategory::Explosion, 0);
    loud.volume_db = 6.0;
    add(&mut p, "boom", Some(loud), None);
    let report = export_sounds(&p, &spec(dir.path())).expect("export");
    assert!(report.files[0].turned_down_db > 0.0);
    assert!(
        report.files[0].peak_dbfs <= -0.99,
        "{}",
        report.files[0].peak_dbfs
    );
    let bytes = std::fs::read(dir.path().join("sfx/boom.wav")).expect("boom");
    assert!(peak_dbfs(&bytes) <= -0.95, "{}", peak_dbfs(&bytes));
}

#[test]
fn names_that_make_the_same_file_get_numbered() {
    let dir = godot_project();
    let mut p = Project::default();
    add(&mut p, "Coin!", None, Some(SoundCategory::Coin));
    add(&mut p, "coin", None, Some(SoundCategory::Coin));
    add(&mut p, "!!!", None, Some(SoundCategory::Blip));
    let report = export_sounds(&p, &spec(dir.path())).expect("export");
    let paths: Vec<&str> = report.files.iter().map(|f| f.res_path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "res://sfx/coin.wav",
            "res://sfx/coin_2.wav",
            "res://sfx/sound.wav"
        ]
    );
}

#[test]
fn unknown_sound_id_is_an_error() {
    let dir = godot_project();
    let mut p = Project::default();
    add(&mut p, "coin", None, Some(SoundCategory::Coin));
    let err = export_sounds(
        &p,
        &SoundExport {
            sound_ids: Some(vec![999]),
            ..spec(dir.path())
        },
    )
    .expect_err("unknown");
    assert!(err.to_string().contains("999"), "{err}");
}

#[test]
fn a_project_without_sounds_has_nothing_to_export() {
    let dir = godot_project();
    assert!(export_sounds(&Project::default(), &spec(dir.path())).is_err());
}

#[test]
fn exporting_one_sound_keeps_the_name_it_gets_when_exporting_all() {
    let dir = godot_project();
    let mut p = Project::default();
    add(&mut p, "Coin!", None, Some(SoundCategory::Coin));
    let second = add(&mut p, "coin", None, Some(SoundCategory::Laser));
    export_sounds(&p, &spec(dir.path())).expect("all");
    let first_file = std::fs::read(dir.path().join("sfx/coin.wav")).expect("coin");
    // Exporting only the second sound must not overwrite the first's file.
    let report = export_sounds(
        &p,
        &SoundExport {
            sound_ids: Some(vec![second, second]),
            ..spec(dir.path())
        },
    )
    .expect("one");
    assert_eq!(report.files.len(), 1, "duplicate ids write one file");
    assert_eq!(report.files[0].res_path, "res://sfx/coin_2.wav");
    assert_eq!(
        std::fs::read(dir.path().join("sfx/coin.wav")).expect("coin"),
        first_file
    );
}

#[test]
fn re_exporting_a_renamed_sound_points_out_its_old_file() {
    let dir = godot_project();
    let mut p = Project::default();
    let coin = add(&mut p, "coin", None, Some(SoundCategory::Coin));
    let jump = add(&mut p, "jump", None, Some(SoundCategory::Jump));
    let first = export_sounds(&p, &spec(dir.path())).expect("first");
    assert!(first.left_behind.is_empty());
    // A file of the user's own in the folder is never mistaken for one.
    std::fs::write(dir.path().join("sfx/mine.wav"), b"RIFF....WAVE").expect("mine");
    Command::RenameSound {
        sound_id: coin,
        name: "pickup".into(),
    }
    .apply(&mut p)
    .expect("rename");
    let report = export_sounds(&p, &spec(dir.path())).expect("again");
    assert_eq!(report.left_behind, ["res://sfx/coin.wav"]);
    // Nothing is deleted: a scene may still use the old file.
    assert!(dir.path().join("sfx/coin.wav").is_file());
    assert!(dir.path().join("sfx/pickup.wav").is_file());
    // Exporting only another sound says nothing about it.
    let only_jump = export_sounds(
        &p,
        &SoundExport {
            sound_ids: Some(vec![jump]),
            ..spec(dir.path())
        },
    )
    .expect("jump");
    assert!(only_jump.left_behind.is_empty());
    // Another song's sound with the same id isn't this one's.
    let mut other = p.clone();
    other.name = "Another song".into();
    Command::RenameSound {
        sound_id: coin,
        name: "gem".into(),
    }
    .apply(&mut other)
    .expect("rename");
    std::fs::remove_file(dir.path().join("sfx/coin.wav")).expect("rm");
    let report = export_sounds(&other, &spec(dir.path())).expect("other song");
    assert!(report.left_behind.is_empty(), "{:?}", report.left_behind);
}

#[test]
fn the_tag_after_the_audio_leaves_the_file_readable() {
    let dir = godot_project();
    let mut p = Project::default();
    let coin = add(&mut p, "coin", None, Some(SoundCategory::Coin));
    export_sounds(&p, &spec(dir.path())).expect("export");
    let bytes = std::fs::read(dir.path().join("sfx/coin.wav")).expect("coin");
    let riff = u32::from_le_bytes(bytes[4..8].try_into().expect("4")) as usize;
    assert_eq!(riff + 8, bytes.len(), "RIFF size covers every chunk");
    let decoded = daw_audio::decode_file(&dir.path().join("sfx/coin.wav")).expect("decodes");
    let recipe = p.sound(coin).expect("coin").recipe;
    assert_eq!(decoded.frames(), daw_sfx::render(&recipe, 44_100).len());
}
