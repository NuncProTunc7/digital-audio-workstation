//! Audio clips: adding them, changing their gain and fades, and edits that
//! work on both kinds of clip (split, trim start).

use super::clips::insert_sorted;
use super::{Command, CommandError, check_length, check_name, check_position, invalid};
use crate::instrument::InstrumentKind;
use crate::project::{
    AudioRegion, Clip, ClipId, MAX_AUDIO_SECONDS, MAX_CLIP_GAIN_DB, MIN_LENGTH_BEATS,
    MIN_VOLUME_DB, Project, TrackId,
};

/// File names are plain names inside the project's audio folder, never
/// paths, so a project can't point at (or overwrite) files elsewhere.
pub(crate) fn check_file_name(name: &str) -> Result<(), CommandError> {
    let bad = name.is_empty()
        || name.len() > 200
        || name.starts_with('.')
        || name
            .chars()
            .any(|c| matches!(c, '/' | '\\' | ':' | '\0') || c.is_control());
    if bad {
        Err(invalid(
            "audio file",
            format!("must be a plain file name in the project's audio folder, got \"{name}\""),
        ))
    } else {
        Ok(())
    }
}

fn check_seconds(what: &str, seconds: f64, max: f64) -> Result<(), CommandError> {
    if seconds.is_finite() && (0.0..=max).contains(&seconds) {
        Ok(())
    } else {
        Err(invalid(
            what,
            format!("must be between 0 and {max} seconds, got {seconds}"),
        ))
    }
}

fn check_gain(gain_db: f64) -> Result<(), CommandError> {
    if gain_db.is_finite() && (MIN_VOLUME_DB..=MAX_CLIP_GAIN_DB).contains(&gain_db) {
        Ok(())
    } else {
        Err(invalid(
            "clip gain",
            format!("must be between {MIN_VOLUME_DB} and {MAX_CLIP_GAIN_DB} dB, got {gain_db}"),
        ))
    }
}

/// Checks every field of an audio region.
pub(crate) fn check_region(r: &AudioRegion) -> Result<(), CommandError> {
    check_file_name(&r.file)?;
    if !(r.file_seconds.is_finite() && r.file_seconds > 0.0 && r.file_seconds <= MAX_AUDIO_SECONDS)
    {
        return Err(invalid(
            "audio file length",
            format!(
                "must be between 0 and {MAX_AUDIO_SECONDS} seconds, got {}",
                r.file_seconds
            ),
        ));
    }
    if !(r.offset_seconds.is_finite()
        && r.offset_seconds >= 0.0
        && r.offset_seconds < r.file_seconds)
    {
        return Err(invalid(
            "audio offset",
            format!(
                "must be at least 0 and before the end of the file ({} s), got {}",
                r.file_seconds, r.offset_seconds
            ),
        ));
    }
    check_gain(r.gain_db)?;
    check_seconds("fade in", r.fade_in_seconds, MAX_AUDIO_SECONDS)?;
    check_seconds("fade out", r.fade_out_seconds, MAX_AUDIO_SECONDS)
}

/// Audio clips live only on audio tracks, and note clips only elsewhere.
pub(crate) fn check_clip_fits(kind: InstrumentKind, clip: &Clip) -> Result<(), CommandError> {
    match &clip.audio {
        None if kind.is_audio() => Err(invalid(
            "clip",
            "is a note clip, but this is an audio track (use import_audio or add_audio_clip)",
        )),
        None => Ok(()),
        Some(_) if !kind.is_audio() => Err(invalid(
            "clip",
            "is an audio clip, so it can only go on an audio track",
        )),
        Some(_) if !clip.notes.is_empty() => Err(invalid("audio clip", "can't contain notes")),
        Some(region) => check_region(region),
    }
}

/// Clip length that plays the rest of the file at this tempo.
pub(crate) fn natural_length_beats(region: &AudioRegion, tempo_bpm: f64) -> f64 {
    (region.remaining_seconds() * tempo_bpm / 60.0).max(MIN_LENGTH_BEATS)
}

pub(super) fn add(
    project: &mut Project,
    track_id: TrackId,
    start_beats: f64,
    audio: AudioRegion,
    length_beats: Option<f64>,
    name: Option<String>,
) -> Result<Command, CommandError> {
    check_position("clip start", start_beats)?;
    let track = project
        .track(track_id)
        .ok_or(CommandError::UnknownTrack(track_id))?;
    if !track.instrument.kind.is_audio() {
        return Err(invalid(
            "track",
            format!(
                "\"{}\" is not an audio track; add one with add_track (instrument \"audio\")",
                track.name
            ),
        ));
    }
    check_region(&audio)?;
    let length = length_beats.unwrap_or_else(|| natural_length_beats(&audio, project.tempo_bpm));
    check_length("clip length", length)?;
    let name = match name {
        Some(n) => check_name(n)?,
        None => default_name(&audio.file),
    };
    let id = project.allocate_id();
    let clip = Clip {
        id,
        name,
        start_beats,
        length_beats: length,
        notes: Vec::new(),
        audio: Some(audio),
    };
    if let Some(t) = project.track_mut(track_id) {
        insert_sorted(&mut t.clips, clip);
    }
    Ok(Command::DeleteClip { clip_id: id })
}

/// "Voice Memo 3-1a2b3c4d.wav" → "Voice Memo 3".
fn default_name(file: &str) -> String {
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    let stem = match stem.rsplit_once('-') {
        Some((head, tail)) if tail.len() == 8 && tail.chars().all(|c| c.is_ascii_hexdigit()) => {
            head
        }
        _ => stem,
    };
    let stem = stem.trim();
    if stem.is_empty() {
        "Audio".to_owned()
    } else {
        stem.to_owned()
    }
}

pub(super) fn set(
    project: &mut Project,
    clip_id: ClipId,
    gain_db: Option<f64>,
    fade_in_seconds: Option<f64>,
    fade_out_seconds: Option<f64>,
) -> Result<Command, CommandError> {
    if let Some(g) = gain_db {
        check_gain(g)?;
    }
    if let Some(f) = fade_in_seconds {
        check_seconds("fade in", f, MAX_AUDIO_SECONDS)?;
    }
    if let Some(f) = fade_out_seconds {
        check_seconds("fade out", f, MAX_AUDIO_SECONDS)?;
    }
    let clip = project
        .clip_mut(clip_id)
        .ok_or(CommandError::UnknownClip(clip_id))?;
    let region = clip
        .audio
        .as_mut()
        .ok_or_else(|| invalid("clip", "is a note clip, not an audio clip"))?;
    let inverse = Command::SetAudioClip {
        clip_id,
        gain_db: gain_db.map(|_| region.gain_db),
        fade_in_seconds: fade_in_seconds.map(|_| region.fade_in_seconds),
        fade_out_seconds: fade_out_seconds.map(|_| region.fade_out_seconds),
    };
    if let Some(g) = gain_db {
        region.gain_db = g;
    }
    if let Some(f) = fade_in_seconds {
        region.fade_in_seconds = f;
    }
    if let Some(f) = fade_out_seconds {
        region.fade_out_seconds = f;
    }
    Ok(inverse)
}

/// Replaces a clip with an edited copy. The inverse puts the original back
/// whole, which keeps undo exact for edits that drop or move notes.
fn replace_clip(
    project: &mut Project,
    clip_id: ClipId,
    edited: Clip,
    extra: Option<Clip>,
) -> Result<Command, CommandError> {
    let (track_id, original) = project
        .clip(clip_id)
        .ok_or(CommandError::UnknownClip(clip_id))?;
    let original = original.clone();
    let track = project
        .track_mut(track_id)
        .ok_or(CommandError::UnknownTrack(track_id))?;
    track.clips.retain(|c| c.id != clip_id);
    insert_sorted(&mut track.clips, edited);
    let mut undo = Vec::new();
    if let Some(extra) = extra {
        undo.push(Command::DeleteClip { clip_id: extra.id });
        insert_sorted(&mut track.clips, extra);
    }
    undo.push(Command::DeleteClip { clip_id });
    undo.push(Command::RestoreClip {
        track_id,
        clip: original,
    });
    Ok(Command::Batch { commands: undo })
}

pub(super) fn split(
    project: &mut Project,
    clip_id: ClipId,
    at_beats: f64,
) -> Result<Command, CommandError> {
    let clip = project
        .clip(clip_id)
        .ok_or(CommandError::UnknownClip(clip_id))?
        .1
        .clone();
    let cut = at_beats - clip.start_beats;
    if !(cut.is_finite() && cut >= MIN_LENGTH_BEATS && clip.length_beats - cut >= MIN_LENGTH_BEATS)
    {
        return Err(invalid(
            "split point",
            format!(
                "must be inside the clip (beats {} to {}), got {at_beats}",
                clip.start_beats,
                clip.start_beats + clip.length_beats
            ),
        ));
    }
    let mut first = clip.clone();
    let mut second = clip.clone();
    let tempo_bpm = project.tempo_bpm;
    first.length_beats = cut;
    second.start_beats = at_beats;
    second.length_beats = clip.length_beats - cut;
    // Notes stay with the half they start in.
    first.notes.retain(|n| n.start_beats < cut);
    second.notes.retain(|n| n.start_beats >= cut);
    for n in &mut second.notes {
        n.start_beats -= cut;
    }
    if let (Some(a), Some(b)) = (first.audio.as_mut(), second.audio.as_mut()) {
        b.offset_seconds = a.offset_seconds + cut * 60.0 / tempo_bpm;
        a.fade_out_seconds = 0.0;
        b.fade_in_seconds = 0.0;
        if b.offset_seconds >= b.file_seconds {
            return Err(invalid(
                "split point",
                "is past the end of the recording; shorten the clip instead",
            ));
        }
    }
    // Allocate last so a rejected split doesn't burn an id.
    second.id = project.allocate_id();
    replace_clip(project, clip_id, first, Some(second))
}

pub(super) fn trim_start(
    project: &mut Project,
    clip_id: ClipId,
    start_beats: f64,
) -> Result<Command, CommandError> {
    check_position("clip start", start_beats)?;
    let (_, clip) = project
        .clip(clip_id)
        .ok_or(CommandError::UnknownClip(clip_id))?;
    let end = clip.start_beats + clip.length_beats;
    let delta = start_beats - clip.start_beats;
    let mut edited = clip.clone();
    edited.start_beats = start_beats;
    edited.length_beats = end - start_beats;
    check_length("clip length", edited.length_beats)?;
    if let Some(a) = edited.audio.as_mut() {
        let offset = a.offset_seconds + delta * 60.0 / project.tempo_bpm;
        // Allow a hair of float error when dragging back to the file start.
        if offset < -1e-6 {
            return Err(invalid(
                "clip start",
                "can't move before the beginning of the recording",
            ));
        }
        a.offset_seconds = offset.max(0.0);
        if a.offset_seconds >= a.file_seconds {
            return Err(invalid("clip start", "is past the end of the recording"));
        }
    }
    for n in &mut edited.notes {
        n.start_beats -= delta;
    }
    edited.notes.retain(|n| n.start_beats >= 0.0);
    replace_clip(project, clip_id, edited, None)
}
