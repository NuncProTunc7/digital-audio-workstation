//! Named versions of the song kept inside the project, so the user (or
//! Claude) can try ideas and go back, or compare two versions.

use super::{Command, CommandError, invalid};
use crate::project::{Id, Project, Snapshot, SongState};

/// Most versions one song keeps; each holds a full copy of the song.
pub const MAX_SNAPSHOTS: usize = 50;

fn check_snapshot_name(name: String) -> Result<String, CommandError> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(invalid("version name", "can't be empty"));
    }
    if name.chars().count() > 80 {
        return Err(invalid("version name", "must be at most 80 characters"));
    }
    Ok(name)
}

fn index_of(project: &Project, snapshot_id: Id) -> Result<usize, CommandError> {
    project
        .snapshots
        .iter()
        .position(|s| s.id == snapshot_id)
        .ok_or_else(|| {
            invalid(
                "version",
                format!("there is no saved version with id {snapshot_id}"),
            )
        })
}

/// Checks a song state the same way opening a file does, filling in any
/// instrument or effect settings it lacks.
fn validated(project: &Project, song: SongState) -> Result<SongState, CommandError> {
    let mut probe = project.clone();
    probe.snapshots.clear();
    probe.set_song_state(song);
    crate::file::validate_project(&mut probe).map_err(|e| invalid("version", e.to_string()))?;
    Ok(probe.song_state())
}

pub(super) fn take(project: &mut Project, name: String) -> Result<Command, CommandError> {
    let name = check_snapshot_name(name)?;
    if project.snapshots.len() >= MAX_SNAPSHOTS {
        return Err(invalid(
            "versions",
            format!("a song keeps at most {MAX_SNAPSHOTS}; delete an old one first"),
        ));
    }
    let id = project.allocate_id();
    let song = project.song_state();
    project.snapshots.push(Snapshot { id, name, song });
    Ok(Command::DeleteSnapshot { snapshot_id: id })
}

pub(super) fn delete(project: &mut Project, snapshot_id: Id) -> Result<Command, CommandError> {
    let index = index_of(project, snapshot_id)?;
    let snapshot = project.snapshots.remove(index);
    Ok(Command::RestoreSnapshot { snapshot, index })
}

pub(super) fn restore(
    project: &mut Project,
    snapshot: Snapshot,
    index: usize,
) -> Result<Command, CommandError> {
    let name = check_snapshot_name(snapshot.name)?;
    if project.id_in_use(snapshot.id) || project.snapshots.iter().any(|s| s.id == snapshot.id) {
        return Err(CommandError::IdInUse(snapshot.id));
    }
    let song = validated(project, snapshot.song)?;
    let id = snapshot.id;
    project.reserve_id(id);
    let index = index.min(project.snapshots.len());
    project.snapshots.insert(index, Snapshot { id, name, song });
    Ok(Command::DeleteSnapshot { snapshot_id: id })
}

pub(super) fn rename(
    project: &mut Project,
    snapshot_id: Id,
    name: String,
) -> Result<Command, CommandError> {
    let name = check_snapshot_name(name)?;
    let index = index_of(project, snapshot_id)?;
    let old = std::mem::replace(&mut project.snapshots[index].name, name);
    Ok(Command::RenameSnapshot {
        snapshot_id,
        name: old,
    })
}

pub(super) fn load(project: &mut Project, snapshot_id: Id) -> Result<Command, CommandError> {
    let index = index_of(project, snapshot_id)?;
    let song = project.snapshots[index].song.clone();
    let song = validated(project, song)?;
    let old = project.set_song_state(song);
    Ok(Command::SetSongState {
        song: Box::new(old),
    })
}

pub(super) fn set_state(project: &mut Project, song: SongState) -> Result<Command, CommandError> {
    let song = validated(project, song)?;
    let old = project.set_song_state(song);
    Ok(Command::SetSongState {
        song: Box::new(old),
    })
}
