//! Linked clips: copies of a note clip that stay the same. Editing the
//! notes (or swing) of one changes all of them, so a melody fixed in the
//! quiet section is fixed in the boss section too.

use super::{Command, CommandError, invalid};
use crate::project::{ClipId, Id, Note, Project};

/// The clip a note-changing command edits, if it is one.
pub(super) fn note_target(command: &Command) -> Option<ClipId> {
    use Command as C;
    match command {
        C::AddNotes { clip_id, .. }
        | C::RemoveNotes { clip_id, .. }
        | C::EditNotes { clip_id, .. }
        | C::QuantizeNotes { clip_id, .. }
        | C::TransposeNotes { clip_id, .. }
        | C::HumanizeNotes { clip_id, .. }
        | C::SetClipSwing { clip_id, .. } => Some(*clip_id),
        _ => None,
    }
}

/// Clips cut by this command stop being linked (they become their own
/// parts).
pub(super) fn cut_target(command: &Command) -> Option<ClipId> {
    match command {
        Command::SplitClip { clip_id, .. } | Command::TrimClipStart { clip_id, .. } => {
            Some(*clip_id)
        }
        _ => None,
    }
}

/// The other clips linked to `clip_id`.
fn partners(project: &Project, clip_id: ClipId) -> Vec<ClipId> {
    let Some(link) = project.clip(clip_id).and_then(|(_, c)| c.link) else {
        return Vec::new();
    };
    project
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .filter(|c| c.id != clip_id && c.link == Some(link))
        .map(|c| c.id)
        .collect()
}

/// Copies `source`'s notes and swing into its linked partners (with ids of
/// their own). Returns the commands that put the partners back.
pub(super) fn propagate(project: &mut Project, source: ClipId) -> Vec<Command> {
    let Some((_, src)) = project.clip(source) else {
        return Vec::new();
    };
    let (notes, swing) = (src.notes.clone(), src.swing);
    let mut undo = Vec::new();
    for partner in partners(project, source) {
        let copies: Vec<Note> = notes
            .iter()
            .map(|n| Note {
                id: project.allocate_id(),
                ..n.clone()
            })
            .collect();
        if let Some(c) = project.clip_mut(partner) {
            let old_notes = std::mem::replace(&mut c.notes, copies);
            let old_swing = std::mem::replace(&mut c.swing, swing);
            // Restores notes and swing together, without propagating.
            undo.push(Command::SetClipNotes {
                clip_id: partner,
                notes: old_notes,
                swing: old_swing,
            });
        }
    }
    undo
}

/// Replaces a clip's notes and swing wholesale (undo of linked edits).
pub(super) fn set_notes(
    project: &mut Project,
    clip_id: ClipId,
    notes: Vec<Note>,
    swing: Option<crate::project::Swing>,
) -> Result<Command, CommandError> {
    for n in &notes {
        super::validate_note_pub(n)?;
    }
    let clip = project
        .clip(clip_id)
        .ok_or(CommandError::UnknownClip(clip_id))?
        .1;
    if let Some(n) = notes
        .iter()
        .find(|n| project.id_in_use(n.id) && !clip.notes.iter().any(|o| o.id == n.id))
    {
        return Err(CommandError::IdInUse(n.id));
    }
    for n in &notes {
        project.reserve_id(n.id);
    }
    let clip = project
        .clip_mut(clip_id)
        .ok_or(CommandError::UnknownClip(clip_id))?;
    let old = std::mem::replace(&mut clip.notes, notes);
    let old_swing = std::mem::replace(&mut clip.swing, swing);
    Ok(Command::SetClipNotes {
        clip_id,
        notes: old,
        swing: old_swing,
    })
}

/// Sets a clip's link group (None unlinks it).
pub(super) fn set_link(
    project: &mut Project,
    clip_id: ClipId,
    link: Option<Id>,
) -> Result<Command, CommandError> {
    let clip = project
        .clip_mut(clip_id)
        .ok_or(CommandError::UnknownClip(clip_id))?;
    if clip.is_audio() && link.is_some() {
        return Err(invalid("clip", "only note clips can be linked"));
    }
    let old = std::mem::replace(&mut clip.link, link);
    Ok(Command::SetClipLink { clip_id, link: old })
}

/// Links clips: all take the first one's notes and swing and stay in step.
pub(super) fn link(project: &mut Project, clip_ids: Vec<ClipId>) -> Result<Command, CommandError> {
    if clip_ids.len() < 2 {
        return Err(invalid("clips", "give at least two clips to link"));
    }
    for id in &clip_ids {
        let (_, c) = project.clip(*id).ok_or(CommandError::UnknownClip(*id))?;
        if c.is_audio() {
            return Err(invalid("clips", "only note clips can be linked"));
        }
    }
    let first = clip_ids[0];
    let group = match project.clip(first).and_then(|(_, c)| c.link) {
        Some(l) => l,
        None => project.allocate_id(),
    };
    let mut undo = Vec::new();
    for id in &clip_ids {
        undo.push(set_link(project, *id, Some(group))?);
    }
    undo.extend(propagate(project, first));
    undo.reverse();
    Ok(Command::Batch { commands: undo })
}
