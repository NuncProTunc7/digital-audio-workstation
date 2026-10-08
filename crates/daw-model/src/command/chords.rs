//! The chord track: which chord plays when. A chord lasts until the next
//! one. It guides writing (scale and chord-tone highlighting, Claude's bass
//! lines and melodies); it makes no sound by itself.

use super::{Command, CommandError, check_position, invalid};
use crate::music::{Chord, ChordQuality};
use crate::project::{Id, Project};

/// Most chords a song's chord track holds.
const MAX_CHORDS: usize = 1_000;

fn check_pc(what: &str, pc: u8) -> Result<(), CommandError> {
    if pc <= 11 {
        Ok(())
    } else {
        Err(invalid(
            what,
            format!("must be a pitch class 0–11 (0 = C), got {pc}"),
        ))
    }
}

fn check_free(project: &Project, start_beats: f64, except: Option<Id>) -> Result<(), CommandError> {
    check_position("chord", start_beats)?;
    match project
        .chords
        .iter()
        .find(|c| Some(c.id) != except && (c.start_beats - start_beats).abs() < 1e-9)
    {
        Some(c) => Err(invalid(
            "chord",
            format!("{} already starts at beat {start_beats}", c.name()),
        )),
        None => Ok(()),
    }
}

fn index_of(project: &Project, chord_id: Id) -> Result<usize, CommandError> {
    project
        .chords
        .iter()
        .position(|c| c.id == chord_id)
        .ok_or_else(|| invalid("chord", format!("there is no chord with id {chord_id}")))
}

fn sort(project: &mut Project) {
    project
        .chords
        .sort_by(|a, b| a.start_beats.total_cmp(&b.start_beats));
}

pub(super) fn add(
    project: &mut Project,
    start_beats: f64,
    root: u8,
    quality: ChordQuality,
    bass: Option<u8>,
) -> Result<Command, CommandError> {
    check_pc("chord root", root)?;
    if let Some(b) = bass {
        check_pc("bass note", b)?;
    }
    check_free(project, start_beats, None)?;
    if project.chords.len() >= MAX_CHORDS {
        return Err(invalid(
            "chords",
            format!("a song holds at most {MAX_CHORDS}"),
        ));
    }
    let id = project.allocate_id();
    project.chords.push(Chord {
        id,
        start_beats,
        root,
        quality,
        bass,
    });
    sort(project);
    Ok(Command::RemoveChord { chord_id: id })
}

pub(super) fn set(
    project: &mut Project,
    chord_id: Id,
    root: u8,
    quality: ChordQuality,
    bass: Option<u8>,
) -> Result<Command, CommandError> {
    check_pc("chord root", root)?;
    if let Some(b) = bass {
        check_pc("bass note", b)?;
    }
    let index = index_of(project, chord_id)?;
    let c = &mut project.chords[index];
    let inverse = Command::SetChord {
        chord_id,
        root: c.root,
        quality: c.quality,
        bass: c.bass,
    };
    c.root = root;
    c.quality = quality;
    c.bass = bass;
    Ok(inverse)
}

pub(super) fn move_to(
    project: &mut Project,
    chord_id: Id,
    start_beats: f64,
) -> Result<Command, CommandError> {
    let index = index_of(project, chord_id)?;
    check_free(project, start_beats, Some(chord_id))?;
    let old = std::mem::replace(&mut project.chords[index].start_beats, start_beats);
    sort(project);
    Ok(Command::MoveChord {
        chord_id,
        start_beats: old,
    })
}

pub(super) fn remove(project: &mut Project, chord_id: Id) -> Result<Command, CommandError> {
    let index = index_of(project, chord_id)?;
    let chord = project.chords.remove(index);
    Ok(Command::RestoreChord { chord })
}

pub(super) fn restore(project: &mut Project, chord: Chord) -> Result<Command, CommandError> {
    check_pc("chord root", chord.root)?;
    if project.id_in_use(chord.id) {
        return Err(CommandError::IdInUse(chord.id));
    }
    check_free(project, chord.start_beats, None)?;
    project.reserve_id(chord.id);
    let id = chord.id;
    project.chords.push(chord);
    sort(project);
    Ok(Command::RemoveChord { chord_id: id })
}
