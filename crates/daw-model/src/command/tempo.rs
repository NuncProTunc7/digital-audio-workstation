//! Tempo changes within the song (the tempo map). The starting tempo is
//! `set_tempo`; these add, move, change and remove the changes after it.

use super::{Command, CommandError, invalid};
use crate::project::{Id, MAX_BEATS, MAX_TEMPO_BPM, MIN_TEMPO_BPM, Project};
use crate::tempo::{MAX_TEMPO_CHANGES, TempoChange};

/// A change must come after the start and inside the song's range.
fn check_beats(beats: f64) -> Result<(), CommandError> {
    if !(beats.is_finite() && beats > 0.0) {
        return Err(invalid(
            "tempo change",
            format!(
                "must be after beat 0, got {beats} (use set_tempo to change the starting tempo)"
            ),
        ));
    }
    if beats > MAX_BEATS {
        return Err(invalid(
            "tempo change",
            format!("must be at most {MAX_BEATS} beats, got {beats}"),
        ));
    }
    Ok(())
}

fn check_bpm(bpm: f64) -> Result<(), CommandError> {
    // `contains` is false for NaN.
    if (MIN_TEMPO_BPM..=MAX_TEMPO_BPM).contains(&bpm) {
        Ok(())
    } else {
        Err(CommandError::TempoOutOfRange(bpm))
    }
}

/// One change per beat, so the tempo at every beat is clear.
fn check_free(project: &Project, beats: f64, except: Option<Id>) -> Result<(), CommandError> {
    check_beats(beats)?;
    match project
        .tempo_changes
        .iter()
        .find(|c| Some(c.id) != except && (c.beats - beats).abs() < 1e-9)
    {
        Some(c) => Err(invalid(
            "tempo change",
            format!(
                "there is already one at beat {beats} (id {}); change that one instead",
                c.id
            ),
        )),
        None => Ok(()),
    }
}

fn index_of(project: &Project, id: Id) -> Result<usize, CommandError> {
    project
        .tempo_changes
        .iter()
        .position(|c| c.id == id)
        .ok_or_else(|| {
            invalid(
                "tempo change",
                format!("there is no tempo change with id {id}"),
            )
        })
}

fn sort(project: &mut Project) {
    project
        .tempo_changes
        .sort_by(|a, b| a.beats.total_cmp(&b.beats));
}

fn check_room(project: &Project) -> Result<(), CommandError> {
    if project.tempo_changes.len() >= MAX_TEMPO_CHANGES {
        return Err(invalid(
            "tempo change",
            format!("a song can have at most {MAX_TEMPO_CHANGES} tempo changes"),
        ));
    }
    Ok(())
}

pub(super) fn add(
    project: &mut Project,
    beats: f64,
    bpm: f64,
    ramp: bool,
) -> Result<Command, CommandError> {
    check_bpm(bpm)?;
    check_free(project, beats, None)?;
    check_room(project)?;
    let id = project.allocate_id();
    project.tempo_changes.push(TempoChange {
        id,
        beats,
        bpm,
        ramp,
    });
    sort(project);
    Ok(Command::RemoveTempoChange {
        tempo_change_id: id,
    })
}

pub(super) fn move_to(project: &mut Project, id: Id, beats: f64) -> Result<Command, CommandError> {
    let index = index_of(project, id)?;
    check_free(project, beats, Some(id))?;
    let old = std::mem::replace(&mut project.tempo_changes[index].beats, beats);
    sort(project);
    Ok(Command::MoveTempoChange {
        tempo_change_id: id,
        beats: old,
    })
}

pub(super) fn set(
    project: &mut Project,
    id: Id,
    bpm: Option<f64>,
    ramp: Option<bool>,
) -> Result<Command, CommandError> {
    let index = index_of(project, id)?;
    if let Some(bpm) = bpm {
        check_bpm(bpm)?;
    }
    let change = &mut project.tempo_changes[index];
    let old = *change;
    if let Some(bpm) = bpm {
        change.bpm = bpm;
    }
    if let Some(ramp) = ramp {
        change.ramp = ramp;
    }
    Ok(Command::SetTempoChange {
        tempo_change_id: id,
        bpm: bpm.map(|_| old.bpm),
        ramp: ramp.map(|_| old.ramp),
    })
}

pub(super) fn remove(project: &mut Project, id: Id) -> Result<Command, CommandError> {
    let index = index_of(project, id)?;
    let change = project.tempo_changes.remove(index);
    Ok(Command::RestoreTempoChange { change })
}

pub(super) fn restore(project: &mut Project, change: TempoChange) -> Result<Command, CommandError> {
    check_bpm(change.bpm)?;
    if project.id_in_use(change.id) {
        return Err(CommandError::IdInUse(change.id));
    }
    check_free(project, change.beats, None)?;
    check_room(project)?;
    project.reserve_id(change.id);
    project.tempo_changes.push(change);
    sort(project);
    Ok(Command::RemoveTempoChange {
        tempo_change_id: change.id,
    })
}

/// Drops changes a file can't use (at or before the start, unusable
/// tempos, two on one beat) and sorts the rest, as opening a file does.
pub(crate) fn clean(project: &mut Project) {
    project.tempo_changes.retain(|c| {
        c.beats.is_finite()
            && c.beats > 0.0
            && c.beats <= MAX_BEATS
            && (MIN_TEMPO_BPM..=MAX_TEMPO_BPM).contains(&c.bpm)
    });
    sort(project);
    project
        .tempo_changes
        .dedup_by(|b, a| (b.beats - a.beats).abs() < 1e-9);
    project.tempo_changes.truncate(MAX_TEMPO_CHANGES);
}

#[cfg(test)]
mod tests {
    use super::super::tests::{fixture, region};
    use super::*;
    use crate::session::Session;

    #[test]
    fn tempo_change_commands_round_trip_undo_and_drag_as_one_step() {
        let mut s = Session::new(fixture());
        s.execute(Command::AddTempoChange {
            beats: 16.0,
            bpm: 150.0,
            ramp: false,
        })
        .expect("add");
        let change = s.project().tempo_changes[0];
        let id = change.id;
        assert_eq!(s.project().tempo_map().bpm_at(16.0), 150.0);
        assert_eq!(s.project().tempo_bpm, 120.0, "the starting tempo stays");
        for command in [
            Command::AddTempoChange {
                beats: 16.0,
                bpm: 150.0,
                ramp: true,
            },
            Command::MoveTempoChange {
                tempo_change_id: id,
                beats: 20.0,
            },
            Command::SetTempoChange {
                tempo_change_id: id,
                bpm: Some(90.0),
                ramp: Some(true),
            },
            Command::RemoveTempoChange {
                tempo_change_id: id,
            },
            Command::RestoreTempoChange { change },
        ] {
            let json = serde_json::to_string(&command).expect("serialize");
            assert_eq!(
                command,
                serde_json::from_str::<Command>(&json).expect("back"),
                "{json}"
            );
        }
        // `ramp` may be left out when adding.
        let added: Command =
            serde_json::from_str(r#"{"command":"add_tempo_change","beats":4,"bpm":100}"#)
                .expect("ramp defaults to false");
        assert!(matches!(added, Command::AddTempoChange { ramp: false, .. }));

        // Dragging a change, or its tempo, is one undo step each.
        for beat in [17.0, 18.0, 19.0, 20.0] {
            s.execute(Command::MoveTempoChange {
                tempo_change_id: id,
                beats: beat,
            })
            .expect("move");
        }
        s.end_gesture();
        for bpm in [140.0, 130.0, 100.0] {
            s.execute(Command::SetTempoChange {
                tempo_change_id: id,
                bpm: Some(bpm),
                ramp: None,
            })
            .expect("set");
        }
        s.end_gesture();
        assert_eq!(s.project().tempo_changes[0].beats, 20.0);
        assert_eq!(s.project().tempo_changes[0].bpm, 100.0);
        s.undo();
        assert_eq!(s.project().tempo_changes[0].bpm, 150.0);
        s.undo();
        assert_eq!(s.project().tempo_changes[0].beats, 16.0);

        // Removing and undoing keeps the id.
        s.execute(Command::RemoveTempoChange {
            tempo_change_id: id,
        })
        .expect("remove");
        assert!(s.project().tempo_changes.is_empty());
        s.undo();
        assert_eq!(s.project().tempo_changes, vec![change]);

        // Changes stay sorted by beat.
        s.execute(Command::AddTempoChange {
            beats: 8.0,
            bpm: 100.0,
            ramp: true,
        })
        .expect("add earlier");
        let beats: Vec<f64> = s.project().tempo_changes.iter().map(|c| c.beats).collect();
        assert_eq!(beats, vec![8.0, 16.0]);
        s.execute(Command::MoveTempoChange {
            tempo_change_id: id,
            beats: 4.0,
        })
        .expect("move before");
        assert_eq!(s.project().tempo_changes[0].id, id);

        // Saved versions and files keep them.
        s.execute(Command::TakeSnapshot { name: "v".into() })
            .expect("take");
        s.execute(Command::RemoveTempoChange {
            tempo_change_id: id,
        })
        .expect("remove");
        let version = s.project().snapshots[0].id;
        s.execute(Command::LoadSnapshot {
            snapshot_id: version,
        })
        .expect("load");
        assert_eq!(s.project().tempo_changes.len(), 2);
        let json = crate::file::project_to_json(s.project());
        assert_eq!(
            crate::file::project_from_json(&json)
                .expect("load")
                .tempo_changes,
            s.project().tempo_changes
        );
    }

    #[test]
    fn tempo_changes_reject_bad_positions_and_tempos() {
        let mut p = fixture();
        let add = |beats: f64, bpm: f64| Command::AddTempoChange {
            beats,
            bpm,
            ramp: false,
        };
        for bad in [add(0.0, 100.0), add(-1.0, 100.0), add(f64::NAN, 100.0)] {
            let err = bad.apply(&mut p).expect_err("not after the start");
            assert!(err.to_string().contains("set_tempo"), "{err}");
        }
        assert!(add(MAX_BEATS + 1.0, 100.0).apply(&mut p).is_err());
        assert!(add(4.0, 10.0).apply(&mut p).is_err());
        assert!(add(4.0, f64::NAN).apply(&mut p).is_err());
        add(4.0, 100.0).apply(&mut p).expect("first");
        let err = add(4.0, 90.0).apply(&mut p).expect_err("same beat");
        assert!(err.to_string().contains("already"), "{err}");
        let id = p.tempo_changes[0].id;
        add(8.0, 90.0).apply(&mut p).expect("second");
        assert!(
            Command::MoveTempoChange {
                tempo_change_id: id,
                beats: 8.0
            }
            .apply(&mut p)
            .is_err()
        );
        assert!(
            Command::SetTempoChange {
                tempo_change_id: 999,
                bpm: Some(100.0),
                ramp: None
            }
            .apply(&mut p)
            .is_err()
        );
        assert_eq!(p.tempo_changes.len(), 2, "rejected commands change nothing");
    }

    #[test]
    fn old_projects_without_tempo_changes_load_with_one_tempo() {
        let p = Project {
            tempo_bpm: 96.0,
            ..Project::default()
        };
        let json = crate::file::project_to_json(&p);
        assert!(!json.contains("tempo_changes"), "nothing extra is written");
        let back = crate::file::project_from_json(&json).expect("load");
        assert!(back.tempo_changes.is_empty());
        assert!(back.tempo_map().is_steady());
        assert_eq!(back.tempo_map().bpm_at(1000.0), 96.0);
    }

    #[test]
    fn files_with_bad_tempo_changes_are_cleaned_up_on_open() {
        let change = |id, beats, bpm| TempoChange {
            id,
            beats,
            bpm,
            ramp: false,
        };
        let p = Project {
            tempo_changes: vec![
                change(50, 8.0, 90.0),
                change(51, 0.0, 90.0),
                change(52, 4.0, 5000.0),
                change(53, 2.0, 100.0),
            ],
            ..Project::default()
        };
        let json = crate::file::project_to_json(&p);
        let back = crate::file::project_from_json(&json).expect("load");
        let kept: Vec<(f64, f64)> = back
            .tempo_changes
            .iter()
            .map(|c| (c.beats, c.bpm))
            .collect();
        assert_eq!(kept, vec![(2.0, 100.0), (8.0, 90.0)]);
    }

    #[test]
    fn audio_at_its_own_speed_fills_its_seconds_across_a_tempo_change() {
        // A 4-second file at beat 40 on the Vox track: 120 BPM until beat
        // 44 (2 s), then 60 BPM, so its last 2 s cover 2 more beats.
        let mut p = fixture();
        Command::AddTempoChange {
            beats: 44.0,
            bpm: 60.0,
            ramp: false,
        }
        .apply(&mut p)
        .expect("tempo");
        Command::AddAudioClip {
            track_id: 10,
            start_beats: 40.0,
            audio: region("vox.wav", 4.0),
            length_beats: None,
            name: None,
        }
        .apply(&mut p)
        .expect("clip");
        let clip = p.tracks[3]
            .clips
            .iter()
            .find(|c| c.start_beats == 40.0)
            .expect("clip")
            .clone();
        assert!(
            (clip.length_beats - 6.0).abs() < 1e-9,
            "{}",
            clip.length_beats
        );
        // Splitting at beat 45 starts the second part 3 s into the file.
        Command::SplitClip {
            clip_id: clip.id,
            at_beats: 45.0,
        }
        .apply(&mut p)
        .expect("split");
        let second = p.tracks[3]
            .clips
            .iter()
            .find(|c| c.start_beats == 45.0)
            .expect("second part");
        let offset = second.audio.as_ref().expect("audio").offset_seconds;
        assert!((offset - 3.0).abs() < 1e-9, "{offset}");
        // Trimming its start to beat 45.5 skips half a second more (60 BPM).
        Command::TrimClipStart {
            clip_id: second.id,
            start_beats: 45.5,
        }
        .apply(&mut p)
        .expect("trim");
        let trimmed = p.tracks[3]
            .clips
            .iter()
            .find(|c| c.start_beats == 45.5)
            .expect("trimmed");
        let offset = trimmed.audio.as_ref().expect("audio").offset_seconds;
        assert!((offset - 3.5).abs() < 1e-9, "{offset}");
    }

    #[test]
    fn freezing_notices_tempo_changes() {
        let mut p = fixture();
        let track = p.tracks[0].clone();
        let before = p.freeze_fingerprint(&track);
        Command::AddTempoChange {
            beats: 4.0,
            bpm: 60.0,
            ramp: false,
        }
        .apply(&mut p)
        .expect("tempo");
        assert_ne!(before, p.freeze_fingerprint(&track));
    }
}
