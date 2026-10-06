use super::notes::{NoteInput, materialize, sort_notes};
use super::{Command, CommandError, check_length, check_name, check_position};
use crate::project::{Clip, ClipId, Project, TrackId};

fn insert_sorted(clips: &mut Vec<Clip>, clip: Clip) {
    let at = clips
        .iter()
        .position(|c| c.start_beats > clip.start_beats)
        .unwrap_or(clips.len());
    clips.insert(at, clip);
}

pub(super) fn create(
    project: &mut Project,
    track_id: TrackId,
    start_beats: f64,
    length_beats: f64,
    name: Option<String>,
    notes: Vec<NoteInput>,
) -> Result<Command, CommandError> {
    check_position("clip start", start_beats)?;
    check_length("clip length", length_beats)?;
    let track_name = project
        .track(track_id)
        .ok_or(CommandError::UnknownTrack(track_id))?
        .name
        .clone();
    let name = match name {
        Some(n) => check_name(n)?,
        None => track_name,
    };
    let mut notes = materialize(project, notes)?;
    sort_notes(&mut notes);
    let id = project.allocate_id();
    let clip = Clip {
        id,
        name,
        start_beats,
        length_beats,
        notes,
    };
    if let Some(t) = project.track_mut(track_id) {
        insert_sorted(&mut t.clips, clip);
    }
    Ok(Command::DeleteClip { clip_id: id })
}

pub(super) fn delete(project: &mut Project, clip_id: ClipId) -> Result<Command, CommandError> {
    let (track_id, _) = project
        .clip(clip_id)
        .ok_or(CommandError::UnknownClip(clip_id))?;
    let track = project
        .track_mut(track_id)
        .ok_or(CommandError::UnknownTrack(track_id))?;
    let index = track
        .clips
        .iter()
        .position(|c| c.id == clip_id)
        .ok_or(CommandError::UnknownClip(clip_id))?;
    let clip = track.clips.remove(index);
    Ok(Command::RestoreClip { track_id, clip })
}

pub(super) fn restore(
    project: &mut Project,
    track_id: TrackId,
    mut clip: Clip,
) -> Result<Command, CommandError> {
    project
        .track(track_id)
        .ok_or(CommandError::UnknownTrack(track_id))?;
    check_position("clip start", clip.start_beats)?;
    check_length("clip length", clip.length_beats)?;
    clip.name = check_name(clip.name)?;
    if project.id_in_use(clip.id) {
        return Err(CommandError::IdInUse(clip.id));
    }
    let inputs = std::mem::take(&mut clip.notes)
        .into_iter()
        .map(|n| NoteInput {
            pitch: n.pitch,
            start_beats: n.start_beats,
            length_beats: n.length_beats,
            velocity: n.velocity,
            id: Some(n.id),
        })
        .collect();
    clip.notes = materialize(project, inputs)?;
    sort_notes(&mut clip.notes);
    project.reserve_id(clip.id);
    let clip_id = clip.id;
    if let Some(t) = project.track_mut(track_id) {
        insert_sorted(&mut t.clips, clip);
    }
    Ok(Command::DeleteClip { clip_id })
}

pub(super) fn move_clip(
    project: &mut Project,
    clip_id: ClipId,
    start_beats: Option<f64>,
    track_id: Option<TrackId>,
) -> Result<Command, CommandError> {
    let (from_track, clip) = project
        .clip(clip_id)
        .ok_or(CommandError::UnknownClip(clip_id))?;
    let old_start = clip.start_beats;
    if let Some(s) = start_beats {
        check_position("clip start", s)?;
    }
    let to_track = track_id.unwrap_or(from_track);
    project
        .track(to_track)
        .ok_or(CommandError::UnknownTrack(to_track))?;

    let source = project
        .track_mut(from_track)
        .ok_or(CommandError::UnknownTrack(from_track))?;
    let index = source
        .clips
        .iter()
        .position(|c| c.id == clip_id)
        .ok_or(CommandError::UnknownClip(clip_id))?;
    let mut clip = source.clips.remove(index);
    if let Some(s) = start_beats {
        clip.start_beats = s;
    }
    if let Some(t) = project.track_mut(to_track) {
        insert_sorted(&mut t.clips, clip);
    }
    Ok(Command::MoveClip {
        clip_id,
        start_beats: start_beats.map(|_| old_start),
        track_id: track_id.map(|_| from_track),
    })
}

pub(super) fn resize(
    project: &mut Project,
    clip_id: ClipId,
    length_beats: f64,
) -> Result<Command, CommandError> {
    check_length("clip length", length_beats)?;
    let clip = project
        .clip_mut(clip_id)
        .ok_or(CommandError::UnknownClip(clip_id))?;
    let old = std::mem::replace(&mut clip.length_beats, length_beats);
    Ok(Command::ResizeClip {
        clip_id,
        length_beats: old,
    })
}

pub(super) fn rename(
    project: &mut Project,
    clip_id: ClipId,
    name: String,
) -> Result<Command, CommandError> {
    let name = check_name(name)?;
    let clip = project
        .clip_mut(clip_id)
        .ok_or(CommandError::UnknownClip(clip_id))?;
    let old = std::mem::replace(&mut clip.name, name);
    Ok(Command::RenameClip { clip_id, name: old })
}

pub(super) fn duplicate(
    project: &mut Project,
    clip_id: ClipId,
    start_beats: Option<f64>,
) -> Result<Command, CommandError> {
    let (track_id, clip) = project
        .clip(clip_id)
        .ok_or(CommandError::UnknownClip(clip_id))?;
    let start = start_beats.unwrap_or(clip.start_beats + clip.length_beats);
    let notes = clip
        .notes
        .iter()
        .map(|n| NoteInput {
            pitch: n.pitch,
            start_beats: n.start_beats,
            length_beats: n.length_beats,
            velocity: n.velocity,
            id: None,
        })
        .collect();
    let (length, name) = (clip.length_beats, clip.name.clone());
    create(project, track_id, start, length, Some(name), notes)
}
