use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Command, CommandError, check_length, check_position, invalid};
use crate::project::{Clip, ClipId, Note, NoteId, Project, full_chance};

fn default_velocity() -> u8 {
    100
}

/// A note to add. Times are in beats relative to the clip start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NoteInput {
    /// MIDI note number, 0–127. 60 = middle C. Drums: 36 kick, 38 snare,
    /// 42 closed hat, 46 open hat, 49 crash.
    pub pitch: u8,
    /// Start, in beats from the clip start.
    pub start_beats: f64,
    /// Length in beats (0.25 = sixteenth note in 4/4).
    pub length_beats: f64,
    /// Loudness, 1–127 (default 100).
    #[serde(default = "default_velocity")]
    pub velocity: u8,
    /// How often it plays, 1–100 % (default 100 = every time). Use for
    /// occasional ghost notes and fills; the pattern of skips is fixed, so
    /// playback and exports always match.
    #[serde(default = "full_chance")]
    pub chance: u8,
    /// Leave out; only used by undo to restore notes with their old ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<NoteId>,
}

/// Changes to one note. Omitted fields keep their value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NoteEdit {
    pub id: NoteId,
    pub pitch: Option<u8>,
    pub start_beats: Option<f64>,
    pub length_beats: Option<f64>,
    pub velocity: Option<u8>,
    /// How often it plays, 1–100 %.
    #[serde(default)]
    pub chance: Option<u8>,
}

impl NoteEdit {
    /// Same note and the same fields being changed.
    pub(crate) fn same_shape(&self, other: &NoteEdit) -> bool {
        self.id == other.id
            && self.pitch.is_some() == other.pitch.is_some()
            && self.start_beats.is_some() == other.start_beats.is_some()
            && self.length_beats.is_some() == other.length_beats.is_some()
            && self.velocity.is_some() == other.velocity.is_some()
            && self.chance.is_some() == other.chance.is_some()
    }
}

fn check_pitch(pitch: u8) -> Result<(), CommandError> {
    if pitch <= 127 {
        Ok(())
    } else {
        Err(invalid("pitch", format!("must be 0–127, got {pitch}")))
    }
}

fn check_velocity(velocity: u8) -> Result<(), CommandError> {
    if (1..=127).contains(&velocity) {
        Ok(())
    } else {
        Err(invalid(
            "velocity",
            format!("must be 1–127, got {velocity}"),
        ))
    }
}

fn check_chance(chance: u8) -> Result<(), CommandError> {
    if (1..=100).contains(&chance) {
        Ok(())
    } else {
        Err(invalid("chance", format!("must be 1–100 %, got {chance}")))
    }
}

pub(crate) fn validate_input(n: &NoteInput) -> Result<(), CommandError> {
    check_pitch(n.pitch)?;
    check_velocity(n.velocity)?;
    check_chance(n.chance)?;
    check_position("note start", n.start_beats)?;
    check_length("note length", n.length_beats)
}

/// Turns inputs into notes, reusing requested ids (undo) or allocating new
/// ones. Validates everything before changing the project.
pub(crate) fn materialize(
    project: &mut Project,
    inputs: Vec<NoteInput>,
) -> Result<Vec<Note>, CommandError> {
    let mut requested = Vec::new();
    for n in &inputs {
        validate_input(n)?;
        if let Some(id) = n.id {
            if project.id_in_use(id) || requested.contains(&id) {
                return Err(CommandError::IdInUse(id));
            }
            requested.push(id);
        }
    }
    for &id in &requested {
        project.reserve_id(id);
    }
    Ok(inputs
        .into_iter()
        .map(|n| Note {
            id: n.id.unwrap_or_else(|| project.allocate_id()),
            pitch: n.pitch,
            start_beats: n.start_beats,
            length_beats: n.length_beats,
            velocity: n.velocity,
            chance: n.chance,
        })
        .collect())
}

pub(crate) fn sort_notes(notes: &mut [Note]) {
    notes.sort_by(|a, b| {
        a.start_beats
            .total_cmp(&b.start_beats)
            .then(a.pitch.cmp(&b.pitch))
    });
}

fn clip_mut(project: &mut Project, clip_id: ClipId) -> Result<&mut Clip, CommandError> {
    let clip = project
        .clip_mut(clip_id)
        .ok_or(CommandError::UnknownClip(clip_id))?;
    if clip.is_audio() {
        return Err(super::invalid(
            "clip",
            "is an audio clip; it has no notes to edit",
        ));
    }
    Ok(clip)
}

pub(super) fn add(
    project: &mut Project,
    clip_id: ClipId,
    inputs: Vec<NoteInput>,
) -> Result<Command, CommandError> {
    clip_mut(project, clip_id)?;
    let notes = materialize(project, inputs)?;
    let note_ids = notes.iter().map(|n| n.id).collect();
    let clip = clip_mut(project, clip_id)?;
    clip.notes.extend(notes);
    sort_notes(&mut clip.notes);
    Ok(Command::RemoveNotes { clip_id, note_ids })
}

pub(super) fn remove(
    project: &mut Project,
    clip_id: ClipId,
    note_ids: Vec<NoteId>,
) -> Result<Command, CommandError> {
    let clip = clip_mut(project, clip_id)?;
    if let Some(&missing) = note_ids
        .iter()
        .find(|id| !clip.notes.iter().any(|n| n.id == **id))
    {
        return Err(CommandError::UnknownNote(missing));
    }
    let (removed, kept): (Vec<Note>, Vec<Note>) = std::mem::take(&mut clip.notes)
        .into_iter()
        .partition(|n| note_ids.contains(&n.id));
    clip.notes = kept;
    Ok(Command::AddNotes {
        clip_id,
        notes: removed
            .into_iter()
            .map(|n| NoteInput {
                pitch: n.pitch,
                start_beats: n.start_beats,
                length_beats: n.length_beats,
                velocity: n.velocity,
                chance: n.chance,
                id: Some(n.id),
            })
            .collect(),
    })
}

pub(super) fn edit(
    project: &mut Project,
    clip_id: ClipId,
    edits: Vec<NoteEdit>,
) -> Result<Command, CommandError> {
    let clip = clip_mut(project, clip_id)?;
    // Validate every edit before touching anything.
    for e in &edits {
        if !clip.notes.iter().any(|n| n.id == e.id) {
            return Err(CommandError::UnknownNote(e.id));
        }
        if let Some(p) = e.pitch {
            check_pitch(p)?;
        }
        if let Some(v) = e.velocity {
            check_velocity(v)?;
        }
        if let Some(s) = e.start_beats {
            check_position("note start", s)?;
        }
        if let Some(l) = e.length_beats {
            check_length("note length", l)?;
        }
        if let Some(c) = e.chance {
            check_chance(c)?;
        }
    }
    let mut inverse = Vec::with_capacity(edits.len());
    for e in edits {
        let Some(n) = clip.notes.iter_mut().find(|n| n.id == e.id) else {
            continue;
        };
        inverse.push(NoteEdit {
            id: n.id,
            pitch: e.pitch.map(|_| n.pitch),
            start_beats: e.start_beats.map(|_| n.start_beats),
            length_beats: e.length_beats.map(|_| n.length_beats),
            velocity: e.velocity.map(|_| n.velocity),
            chance: e.chance.map(|_| n.chance),
        });
        if let Some(p) = e.pitch {
            n.pitch = p;
        }
        if let Some(s) = e.start_beats {
            n.start_beats = s;
        }
        if let Some(l) = e.length_beats {
            n.length_beats = l;
        }
        if let Some(v) = e.velocity {
            n.velocity = v;
        }
        if let Some(c) = e.chance {
            n.chance = c;
        }
    }
    sort_notes(&mut clip.notes);
    // Undo restores in reverse order so repeated ids resolve correctly.
    inverse.reverse();
    Ok(Command::EditNotes {
        clip_id,
        edits: inverse,
    })
}

fn selected<'a>(
    clip: &'a Clip,
    note_ids: &Option<Vec<NoteId>>,
) -> Result<Vec<&'a Note>, CommandError> {
    match note_ids {
        None => Ok(clip.notes.iter().collect()),
        Some(ids) => ids
            .iter()
            .map(|id| {
                clip.notes
                    .iter()
                    .find(|n| n.id == *id)
                    .ok_or(CommandError::UnknownNote(*id))
            })
            .collect(),
    }
}

pub(super) fn quantize(
    project: &mut Project,
    clip_id: ClipId,
    grid_beats: f64,
    strength: Option<f64>,
    lengths: bool,
    note_ids: Option<Vec<NoteId>>,
) -> Result<Command, CommandError> {
    check_length("grid", grid_beats)?;
    let strength = strength.unwrap_or(1.0);
    if !(0.0..=1.0).contains(&strength) {
        return Err(invalid("strength", format!("must be 0–1, got {strength}")));
    }
    let clip = clip_mut(project, clip_id)?;
    let edits: Vec<NoteEdit> = selected(clip, &note_ids)?
        .into_iter()
        .map(|n| {
            let snapped = (n.start_beats / grid_beats).round() * grid_beats;
            let start = n.start_beats + (snapped - n.start_beats) * strength;
            let length = lengths.then(|| {
                let snapped = ((n.length_beats / grid_beats).round() * grid_beats).max(grid_beats);
                n.length_beats + (snapped - n.length_beats) * strength
            });
            NoteEdit {
                id: n.id,
                pitch: None,
                start_beats: Some(start.max(0.0)),
                length_beats: length,
                velocity: None,
                chance: None,
            }
        })
        .collect();
    edit(project, clip_id, edits)
}

/// A number in -1.0..1.0 from `seed` and `n` (splitmix64), so humanizing
/// is repeatable.
fn jitter(seed: u64, n: u64) -> f64 {
    let mut z = seed
        .wrapping_add(n.wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
}

pub(super) fn humanize(
    project: &mut Project,
    clip_id: ClipId,
    timing_beats: f64,
    velocity: u8,
    seed: u64,
    note_ids: Option<Vec<NoteId>>,
) -> Result<Command, CommandError> {
    if !(timing_beats.is_finite() && (0.0..=0.25).contains(&timing_beats)) {
        return Err(invalid(
            "timing",
            format!("must be 0–0.25 beats, got {timing_beats}"),
        ));
    }
    if velocity > 64 {
        return Err(invalid("velocity", format!("must be 0–64, got {velocity}")));
    }
    let clip = clip_mut(project, clip_id)?;
    let edits: Vec<NoteEdit> = selected(clip, &note_ids)?
        .into_iter()
        .map(|n| {
            // Each note gets its own numbers, from its id.
            let id = u64::from(n.id);
            let start = (n.start_beats + jitter(seed, id * 2) * timing_beats).max(0.0);
            let v =
                f64::from(n.velocity) + (jitter(seed, id * 2 + 1) * f64::from(velocity)).round();
            NoteEdit {
                id: n.id,
                pitch: None,
                start_beats: (timing_beats > 0.0).then_some(start),
                length_beats: None,
                velocity: (velocity > 0).then_some(v.clamp(1.0, 127.0) as u8),
                chance: None,
            }
        })
        .collect();
    edit(project, clip_id, edits)
}

pub(super) fn transpose(
    project: &mut Project,
    clip_id: ClipId,
    semitones: i32,
    note_ids: Option<Vec<NoteId>>,
) -> Result<Command, CommandError> {
    let clip = clip_mut(project, clip_id)?;
    let mut edits = Vec::new();
    for n in selected(clip, &note_ids)? {
        let pitch = i32::from(n.pitch) + semitones;
        let pitch = u8::try_from(pitch)
            .ok()
            .filter(|p| *p <= 127)
            .ok_or_else(|| {
                invalid(
                    "transpose",
                    format!("would move note {} out of range 0–127", n.pitch),
                )
            })?;
        edits.push(NoteEdit {
            id: n.id,
            pitch: Some(pitch),
            start_beats: None,
            length_beats: None,
            velocity: None,
            chance: None,
        });
    }
    edit(project, clip_id, edits)
}
