//! Song-wide arrangement edits by whole bars: open a gap, cut a range out,
//! repeat a range. Each is built from ordinary Commands (split, move,
//! duplicate, marker and chord moves, automation points, loop), applied one
//! at a time so later steps can see the ids earlier ones made; the inverse is
//! all their inverses in reverse, so undo keeps every id.

use super::{Command, CommandError, invalid};
use crate::project::{AutomationPoint, ClipId, Id, MAX_BEATS, MIN_LENGTH_BEATS, Project, TrackId};

/// Positions closer than this are the same beat.
const EPS: f64 = 1e-9;
/// Most copies one repeat_bars makes.
const MAX_REPEATS: u32 = 64;
/// Longest marker name (matches the marker module).
const MAX_MARKER_NAME_CHARS: usize = 40;

/// Applies Commands and remembers how to undo them.
struct Edit<'a> {
    project: &'a mut Project,
    undo: Vec<Command>,
}

impl Edit<'_> {
    fn run(&mut self, command: Command) -> Result<(), CommandError> {
        let inverse = command.apply(self.project)?;
        self.undo.push(inverse);
        Ok(())
    }

    /// Splits a clip at `at_beats`; when the split can't be made (an audio
    /// clip running past the end of its recording), the clip is shortened
    /// to end there instead, since what lies past the file end is silence.
    /// Returns the id of the right-hand piece, if one was made.
    fn split(&mut self, clip_id: ClipId, at_beats: f64) -> Result<Option<ClipId>, CommandError> {
        let before = self.project.next_id;
        let split = Command::SplitClip { clip_id, at_beats };
        match split.apply(self.project) {
            Ok(inverse) => {
                self.undo.push(inverse);
                Ok(newest_clip(self.project, before))
            }
            Err(_) => {
                let start = self
                    .project
                    .clip(clip_id)
                    .ok_or(CommandError::UnknownClip(clip_id))?
                    .1
                    .start_beats;
                self.run(Command::ResizeClip {
                    clip_id,
                    length_beats: at_beats - start,
                })?;
                Ok(None)
            }
        }
    }
}

/// Runs `edit` as one step: all of it, or (on error) none of it.
fn record(
    project: &mut Project,
    edit: impl FnOnce(&mut Edit) -> Result<(), CommandError>,
) -> Result<Command, CommandError> {
    let mut e = Edit {
        project,
        undo: Vec::new(),
    };
    match edit(&mut e) {
        Ok(()) => {
            e.undo.reverse();
            Ok(Command::Batch { commands: e.undo })
        }
        Err(err) => {
            for inverse in e.undo.into_iter().rev() {
                // Inverses of successful applies are valid by construction.
                let _ = inverse.apply(e.project);
            }
            Err(err)
        }
    }
}

/// The clip with the highest id made since `next_id` was `before`.
fn newest_clip(project: &Project, before: Id) -> Option<ClipId> {
    project
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .filter(|c| c.id >= before)
        .map(|c| c.id)
        .max()
}

/// Every clip as (track, id, start, end).
fn clip_spans(project: &Project) -> Vec<(TrackId, ClipId, f64, f64)> {
    project
        .tracks
        .iter()
        .flat_map(|t| {
            t.clips
                .iter()
                .map(move |c| (t.id, c.id, c.start_beats, c.start_beats + c.length_beats))
        })
        .collect()
}

/// Whether `at` lies far enough inside (start, end) to split there.
fn inside(start: f64, end: f64, at: f64) -> bool {
    at - start >= MIN_LENGTH_BEATS && end - at >= MIN_LENGTH_BEATS
}

/// Converts a 1-based bar range to beats, checking it.
fn bar_range(project: &Project, first_bar: u32, bars: u32) -> Result<(f64, f64), CommandError> {
    if first_bar < 1 {
        return Err(invalid("bar", "numbers start at 1 (the song start)"));
    }
    if bars < 1 {
        return Err(invalid("bars", "must be at least 1"));
    }
    let bpb = project.beats_per_bar();
    let start = f64::from(first_bar - 1) * bpb;
    let len = f64::from(bars) * bpb;
    if start + len > MAX_BEATS {
        return Err(invalid(
            "bars",
            format!("reach past the longest song ({MAX_BEATS} beats)"),
        ));
    }
    Ok((start, len))
}

/// The latest beat anything in the song sits at.
fn last_beat(project: &Project) -> f64 {
    let points = project
        .tracks
        .iter()
        .flat_map(|t| &t.automation)
        .flat_map(|l| &l.points)
        .map(|p| p.beats);
    let markers = project.markers.iter().map(|m| m.start_beats);
    let chords = project.chords.iter().map(|c| c.start_beats);
    points
        .chain(markers)
        .chain(chords)
        .chain([project.end_beats(), project.loop_region.end_beats])
        .fold(0.0, f64::max)
}

fn check_room(project: &Project, extra_beats: f64) -> Result<(), CommandError> {
    if last_beat(project) + extra_beats > MAX_BEATS {
        return Err(invalid(
            "bars",
            format!("would push the song past its longest length ({MAX_BEATS} beats)"),
        ));
    }
    Ok(())
}

/// Moves clips, markers, chords and the loop at or after `at` later by
/// `len`, splitting clips that cross `at`. Automation too when `automation`.
fn open_gap(e: &mut Edit, at: f64, len: f64, automation: bool) -> Result<(), CommandError> {
    for (_, id, start, end) in clip_spans(e.project) {
        if inside(start, end, at) {
            e.split(id, at)?;
        }
    }
    for (_, id, start, _) in clip_spans(e.project) {
        if start >= at - EPS {
            e.run(Command::MoveClip {
                clip_id: id,
                start_beats: Some(start + len),
                track_id: None,
            })?;
        }
    }
    // Latest first, so a moved one never lands on one still to move.
    let markers: Vec<(Id, f64)> = e
        .project
        .markers
        .iter()
        .rev()
        .filter(|m| m.start_beats >= at - EPS)
        .map(|m| (m.id, m.start_beats))
        .collect();
    for (marker_id, start) in markers {
        e.run(Command::MoveMarker {
            marker_id,
            start_beats: start + len,
        })?;
    }
    let chords: Vec<(Id, f64)> = e
        .project
        .chords
        .iter()
        .rev()
        .filter(|c| c.start_beats >= at - EPS)
        .map(|c| (c.id, c.start_beats))
        .collect();
    for (chord_id, start) in chords {
        e.run(Command::MoveChord {
            chord_id,
            start_beats: start + len,
        })?;
    }
    // Tempo changes from the gap on move with the music (latest first).
    let tempos: Vec<(Id, f64)> = e
        .project
        .tempo_changes
        .iter()
        .rev()
        .filter(|c| c.beats >= at - EPS)
        .map(|c| (c.id, c.beats))
        .collect();
    for (tempo_change_id, beats) in tempos {
        e.run(Command::MoveTempoChange {
            tempo_change_id,
            beats: beats + len,
        })?;
    }
    if automation {
        for (track_id, lane_id, points) in lanes(e.project) {
            let (before, after): (Vec<AutomationPoint>, Vec<AutomationPoint>) =
                points.iter().copied().partition(|p| p.beats < at - EPS);
            if after.is_empty() {
                continue;
            }
            let mut new = before.clone();
            if !before.is_empty() {
                // Hold the value the curve had at the gap through it.
                let held = value_before(&points, at);
                new.push(point(at, held));
                new.push(point(at + len, held));
            }
            new.extend(after.iter().map(|p| point(p.beats + len, p.value)));
            e.run(Command::SetAutomationPoints {
                track_id,
                lane_id,
                points: new,
            })?;
        }
    }
    let lp = e.project.loop_region;
    let shift = |x: f64, edge_moves: bool| if edge_moves { x + len } else { x };
    let (start, end) = (
        shift(lp.start_beats, lp.start_beats >= at - EPS),
        shift(lp.end_beats, lp.end_beats > at + EPS),
    );
    if start != lp.start_beats || end != lp.end_beats {
        e.run(Command::SetLoop {
            enabled: None,
            start_beats: Some(start),
            end_beats: Some(end),
        })?;
    }
    Ok(())
}

/// The value a curve approaches just before `beats` (so a jump placed
/// exactly at `beats` doesn't count yet).
fn value_before(points: &[AutomationPoint], beats: f64) -> f64 {
    let i = points.partition_point(|p| p.beats < beats - EPS);
    let Some(a) = i.checked_sub(1).and_then(|i| points.get(i)) else {
        return points.first().map_or(0.0, |p| p.value);
    };
    match points.get(i) {
        Some(b) if b.beats - a.beats > EPS => {
            a.value + (b.value - a.value) * (beats - a.beats) / (b.beats - a.beats)
        }
        Some(b) => b.value,
        None => a.value,
    }
}

fn point(beats: f64, value: f64) -> AutomationPoint {
    AutomationPoint { beats, value }
}

/// Every automation lane with points, as (track, lane, points).
fn lanes(project: &Project) -> Vec<(TrackId, Id, Vec<AutomationPoint>)> {
    project
        .tracks
        .iter()
        .flat_map(|t| {
            t.automation
                .iter()
                .filter(|l| !l.points.is_empty())
                .map(move |l| (t.id, l.id, l.points.clone()))
        })
        .collect()
}

pub(super) fn insert(
    project: &mut Project,
    at_bar: u32,
    bars: u32,
) -> Result<Command, CommandError> {
    let (at, len) = bar_range(project, at_bar, bars)?;
    check_room(project, len)?;
    record(project, |e| open_gap(e, at, len, true))
}

/// Of the items (id, beat) that sit in [from, end), the one that should
/// stay (moved to `from`) so what follows the cut keeps its section or
/// chord: the last one, unless another already starts right at `end`.
fn survivor(items: &[(Id, f64)], from: f64, end: f64) -> Option<Id> {
    if items.iter().any(|&(_, b)| (b - end).abs() < EPS) {
        return None;
    }
    items
        .iter()
        .filter(|&&(_, b)| b >= from - EPS && b < end - EPS)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|&(id, _)| id)
}

/// Deleting `from..end`: tempo changes inside the cut go, except the last
/// one, which moves to the cut so what followed it keeps its tempo; later
/// changes close up. At the song start, the tempo that will open the song
/// becomes the song tempo instead (a change can't sit on beat 0).
fn close_tempo_gap(e: &mut Edit, from: f64, end: f64) -> Result<(), CommandError> {
    let len = end - from;
    let changes: Vec<(Id, f64)> = e
        .project
        .tempo_changes
        .iter()
        .map(|c| (c.id, c.beats))
        .collect();
    let keep = survivor(&changes, from, end);
    for &(tempo_change_id, b) in &changes {
        if b >= from - EPS && b < end - EPS && Some(tempo_change_id) != keep {
            e.run(Command::RemoveTempoChange { tempo_change_id })?;
        }
    }
    let mut opener = None;
    if from < EPS {
        // The change that will play from beat 0: the survivor, or one
        // right on the cut's end.
        opener = keep.or_else(|| {
            changes
                .iter()
                .find(|&&(_, b)| (b - end).abs() < EPS)
                .map(|&(id, _)| id)
        });
        if let Some(tempo_change_id) = opener {
            let bpm = e
                .project
                .tempo_changes
                .iter()
                .find(|c| c.id == tempo_change_id)
                .map(|c| c.bpm);
            if let Some(bpm) = bpm {
                e.run(Command::SetTempo { bpm })?;
            }
            e.run(Command::RemoveTempoChange { tempo_change_id })?;
        }
    } else if let Some(tempo_change_id) = keep.filter(|&id| {
        changes
            .iter()
            .any(|&(c, b)| c == id && (b - from).abs() >= EPS)
    }) {
        e.run(Command::MoveTempoChange {
            tempo_change_id,
            beats: from,
        })?;
    }
    // Earliest first, so a moved one never lands on one still to move.
    for &(tempo_change_id, b) in &changes {
        if b >= end - EPS && Some(tempo_change_id) != opener {
            e.run(Command::MoveTempoChange {
                tempo_change_id,
                beats: b - len,
            })?;
        }
    }
    Ok(())
}

pub(super) fn delete(
    project: &mut Project,
    from_bar: u32,
    bars: u32,
) -> Result<Command, CommandError> {
    let (from, len) = bar_range(project, from_bar, bars)?;
    let end = from + len;
    record(project, |e| {
        // Cut clips at both edges, so every piece is inside or outside.
        for (_, id, start, clip_end) in clip_spans(e.project) {
            if inside(start, clip_end, end) {
                e.split(id, end)?;
            }
            if inside(start, clip_end.min(end), from) {
                e.split(id, from)?;
            }
        }
        for (_, id, start, clip_end) in clip_spans(e.project) {
            if start >= from - EPS && clip_end <= end + EPS {
                e.run(Command::DeleteClip { clip_id: id })?;
            } else if start >= end - EPS {
                e.run(Command::MoveClip {
                    clip_id: id,
                    start_beats: Some(start - len),
                    track_id: None,
                })?;
            }
        }

        let markers: Vec<(Id, f64)> = e
            .project
            .markers
            .iter()
            .map(|m| (m.id, m.start_beats))
            .collect();
        let keep = survivor(&markers, from, end);
        for &(marker_id, b) in &markers {
            if b >= from - EPS && b < end - EPS && Some(marker_id) != keep {
                e.run(Command::RemoveMarker { marker_id })?;
            }
        }
        if let Some(marker_id) =
            keep.filter(|&id| markers.iter().any(|&(m, b)| m == id && b != from))
        {
            e.run(Command::MoveMarker {
                marker_id,
                start_beats: from,
            })?;
        }
        for &(marker_id, b) in &markers {
            if b >= end - EPS {
                e.run(Command::MoveMarker {
                    marker_id,
                    start_beats: b - len,
                })?;
            }
        }

        let chords: Vec<(Id, f64)> = e
            .project
            .chords
            .iter()
            .map(|c| (c.id, c.start_beats))
            .collect();
        let keep = survivor(&chords, from, end);
        for &(chord_id, b) in &chords {
            if b >= from - EPS && b < end - EPS && Some(chord_id) != keep {
                e.run(Command::RemoveChord { chord_id })?;
            }
        }
        if let Some(chord_id) = keep.filter(|&id| chords.iter().any(|&(c, b)| c == id && b != from))
        {
            e.run(Command::MoveChord {
                chord_id,
                start_beats: from,
            })?;
        }
        for &(chord_id, b) in &chords {
            if b >= end - EPS {
                e.run(Command::MoveChord {
                    chord_id,
                    start_beats: b - len,
                })?;
            }
        }

        close_tempo_gap(e, from, end)?;

        for (track_id, lane_id, points) in lanes(e.project) {
            if points.iter().all(|p| p.beats < from - EPS) {
                continue;
            }
            let before: Vec<_> = points
                .iter()
                .filter(|p| p.beats < from - EPS)
                .copied()
                .collect();
            let after: Vec<_> = points
                .iter()
                .filter(|p| p.beats >= end - EPS)
                .map(|p| point(p.beats - len, p.value))
                .collect();
            let value = |b| crate::project::value_at(&points, b).unwrap_or_default();
            let mut new = before.clone();
            if !before.is_empty() {
                new.push(point(from, value_before(&points, from)));
            }
            // The curve picks up where it was at the end of the cut.
            if after.first().is_none_or(|p| (p.beats - from).abs() > EPS) {
                let resume = value(end);
                if new.last().is_none_or(|p| p.value != resume) {
                    new.push(point(from, resume));
                }
            }
            new.extend(after);
            e.run(Command::SetAutomationPoints {
                track_id,
                lane_id,
                points: new,
            })?;
        }

        let lp = e.project.loop_region;
        let map = |x: f64| {
            if x <= from {
                x
            } else if x >= end {
                x - len
            } else {
                from
            }
        };
        let (start, stop) = (map(lp.start_beats), map(lp.end_beats));
        // A loop that was entirely inside the cut stays where it was.
        if stop - start >= MIN_LENGTH_BEATS && (start != lp.start_beats || stop != lp.end_beats) {
            e.run(Command::SetLoop {
                enabled: None,
                start_beats: Some(start),
                end_beats: Some(stop),
            })?;
        }
        Ok(())
    })
}

/// "Chorus" → "Chorus 2", kept within the marker name limit.
fn copy_name(name: &str, copy: u32) -> String {
    let suffix = format!(" {}", copy + 1);
    let keep = MAX_MARKER_NAME_CHARS.saturating_sub(suffix.chars().count());
    let base: String = name.chars().take(keep).collect();
    format!("{}{suffix}", base.trim_end())
}

pub(super) fn repeat(
    project: &mut Project,
    from_bar: u32,
    bars: u32,
    times: u32,
) -> Result<Command, CommandError> {
    if !(1..=MAX_REPEATS).contains(&times) {
        return Err(invalid(
            "times",
            format!("must be 1 to {MAX_REPEATS}, got {times}"),
        ));
    }
    let (from, len) = bar_range(project, from_bar, bars)?;
    let end = from + len;
    let total = len * f64::from(times);
    check_room(project, total)?;
    // Read the section before anything moves.
    let original_lanes = lanes(project);
    let markers: Vec<_> = project
        .markers
        .iter()
        .filter(|m| m.start_beats >= from - EPS && m.start_beats < end - EPS)
        .map(|m| (m.name.clone(), m.start_beats))
        .collect();
    let mut chords: Vec<_> = project
        .chords
        .iter()
        .filter(|c| c.start_beats >= from - EPS && c.start_beats < end - EPS)
        .cloned()
        .collect();
    // A copy must start on the chord that was playing at the section start.
    if chords.first().is_none_or(|c| c.start_beats > from + EPS)
        && let Some(c) = project.chord_at(from)
    {
        let mut c = c.clone();
        c.start_beats = from;
        chords.insert(0, c);
    }

    // Tempo: each copy starts at the tempo the section started with, and
    // repeats the section's own changes.
    let map = project.tempo_map();
    let start_bpm = map.bpm_at(from);
    let end_bpm = map.bpm_at(end - EPS);
    let section_tempos: Vec<_> = project
        .tempo_changes
        .iter()
        .filter(|c| c.beats >= from - EPS && c.beats < end - EPS)
        .copied()
        .collect();
    let restart = section_tempos.first().is_none_or(|c| c.beats > from + EPS)
        && (start_bpm - end_bpm).abs() > 1e-9;

    record(project, |e| {
        open_gap(e, end, total, false)?;
        let sources: Vec<_> = clip_spans(e.project)
            .into_iter()
            .filter(|&(_, _, s, t)| s < end - EPS && t > from + EPS)
            .collect();
        for k in 1..=times {
            let offset = len * f64::from(k);
            for &(_, id, start, clip_end) in &sources {
                let before = e.project.next_id;
                e.run(Command::DuplicateClip {
                    clip_id: id,
                    start_beats: Some(start + offset),
                    linked: false,
                })?;
                let copy = newest_clip(e.project, before).ok_or(CommandError::UnknownClip(id))?;
                let (swing, muted) = e
                    .project
                    .clip(id)
                    .map(|(_, c)| (c.swing, c.muted))
                    .unwrap_or_default();
                if swing.is_some() {
                    e.run(Command::SetClipSwing {
                        clip_id: copy,
                        swing,
                    })?;
                }
                if muted {
                    e.run(Command::SetClipMuted {
                        clip_id: copy,
                        muted,
                    })?;
                }
                if clip_end > end + EPS
                    && let Some(tail) = e.split(copy, end + offset)?
                {
                    e.run(Command::DeleteClip { clip_id: tail })?;
                }
                if start < from - EPS {
                    e.run(Command::TrimClipStart {
                        clip_id: copy,
                        start_beats: from + offset,
                    })?;
                }
            }
            for (name, b) in &markers {
                e.run(Command::AddMarker {
                    name: copy_name(name, k),
                    start_beats: b + offset,
                })?;
            }
            for c in &chords {
                e.run(Command::AddChord {
                    start_beats: c.start_beats + offset,
                    root: c.root,
                    quality: c.quality,
                    bass: c.bass,
                })?;
            }
            if restart {
                e.run(Command::AddTempoChange {
                    beats: from + offset,
                    bpm: start_bpm,
                    ramp: false,
                })?;
            }
            for c in &section_tempos {
                e.run(Command::AddTempoChange {
                    beats: c.beats + offset,
                    bpm: c.bpm,
                    ramp: c.ramp,
                })?;
            }
        }
        for (track_id, lane_id, points) in &original_lanes {
            if points.iter().all(|p| p.beats < from - EPS) {
                continue;
            }
            let value = |b| crate::project::value_at(points, b).unwrap_or_default();
            let end_value = value_before(points, end);
            let inner: Vec<_> = points
                .iter()
                .filter(|p| p.beats > from + EPS && p.beats < end - EPS)
                .copied()
                .collect();
            let mut new: Vec<_> = points
                .iter()
                .filter(|p| p.beats < end - EPS)
                .copied()
                .collect();
            new.push(point(end, end_value));
            for k in 1..=times {
                let offset = len * f64::from(k);
                new.push(point(from + offset, value(from)));
                new.extend(inner.iter().map(|p| point(p.beats + offset, p.value)));
                new.push(point(end + offset, end_value));
            }
            let after: Vec<_> = points.iter().filter(|p| p.beats >= end - EPS).collect();
            if !after.is_empty() {
                new.push(point(end + total, value(end)));
                new.extend(after.iter().map(|p| point(p.beats + total, p.value)));
            }
            new.dedup();
            e.run(Command::SetAutomationPoints {
                track_id: *track_id,
                lane_id: *lane_id,
                points: new,
            })?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests;
