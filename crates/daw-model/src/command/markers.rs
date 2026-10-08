//! Section markers: named points on the timeline ("Intro", "Explore",
//! "Combat"). A section runs from its marker to the next one.

use super::{Command, CommandError, check_position, invalid};
use crate::project::{Id, Marker, Project};

/// Longest marker name, in characters.
const MAX_NAME_CHARS: usize = 40;

fn check_marker_name(name: String) -> Result<String, CommandError> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(invalid("marker name", "can't be empty"));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(invalid(
            "marker name",
            format!("must be at most {MAX_NAME_CHARS} characters"),
        ));
    }
    Ok(name)
}

/// Only one marker per beat, so sections are never empty.
fn check_free(project: &Project, start_beats: f64, except: Option<Id>) -> Result<(), CommandError> {
    check_position("marker", start_beats)?;
    match project
        .markers
        .iter()
        .find(|m| Some(m.id) != except && (m.start_beats - start_beats).abs() < 1e-9)
    {
        Some(m) => Err(invalid(
            "marker",
            format!("\"{}\" is already at beat {start_beats}", m.name),
        )),
        None => Ok(()),
    }
}

fn index_of(project: &Project, marker_id: Id) -> Result<usize, CommandError> {
    project
        .markers
        .iter()
        .position(|m| m.id == marker_id)
        .ok_or_else(|| invalid("marker", format!("there is no marker with id {marker_id}")))
}

fn sort(project: &mut Project) {
    project
        .markers
        .sort_by(|a, b| a.start_beats.total_cmp(&b.start_beats));
}

pub(super) fn add(
    project: &mut Project,
    name: String,
    start_beats: f64,
) -> Result<Command, CommandError> {
    let name = check_marker_name(name)?;
    check_free(project, start_beats, None)?;
    let id = project.allocate_id();
    project.markers.push(Marker {
        id,
        name,
        start_beats,
    });
    sort(project);
    Ok(Command::RemoveMarker { marker_id: id })
}

pub(super) fn move_to(
    project: &mut Project,
    marker_id: Id,
    start_beats: f64,
) -> Result<Command, CommandError> {
    let index = index_of(project, marker_id)?;
    check_free(project, start_beats, Some(marker_id))?;
    let old = std::mem::replace(&mut project.markers[index].start_beats, start_beats);
    sort(project);
    Ok(Command::MoveMarker {
        marker_id,
        start_beats: old,
    })
}

pub(super) fn rename(
    project: &mut Project,
    marker_id: Id,
    name: String,
) -> Result<Command, CommandError> {
    let name = check_marker_name(name)?;
    let index = index_of(project, marker_id)?;
    let old = std::mem::replace(&mut project.markers[index].name, name);
    Ok(Command::RenameMarker {
        marker_id,
        name: old,
    })
}

pub(super) fn remove(project: &mut Project, marker_id: Id) -> Result<Command, CommandError> {
    let index = index_of(project, marker_id)?;
    let marker = project.markers.remove(index);
    Ok(Command::RestoreMarker { marker })
}

pub(super) fn restore(project: &mut Project, marker: Marker) -> Result<Command, CommandError> {
    let name = check_marker_name(marker.name)?;
    if project.id_in_use(marker.id) {
        return Err(CommandError::IdInUse(marker.id));
    }
    check_free(project, marker.start_beats, None)?;
    project.reserve_id(marker.id);
    let id = marker.id;
    project.markers.push(Marker {
        id,
        name,
        start_beats: marker.start_beats,
    });
    sort(project);
    Ok(Command::RemoveMarker { marker_id: id })
}
