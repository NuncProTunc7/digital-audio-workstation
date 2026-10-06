//! Automation lanes: curves that move a track's settings over time.

use super::{Command, CommandError, check_position, invalid};
use crate::project::{
    AutomationLane, AutomationPoint, AutomationTarget, LaneId, MAX_VOLUME_DB, MIN_VOLUME_DB,
    Project, Track, TrackId,
};
use crate::{effect, instrument};

/// Most points one lane may hold.
pub const MAX_AUTOMATION_POINTS: usize = 10_000;

/// The allowed value range of a target on `track`.
pub(crate) fn target_range(
    track: &Track,
    target: &AutomationTarget,
) -> Result<(f64, f64), CommandError> {
    match target {
        AutomationTarget::Volume => Ok((MIN_VOLUME_DB, MAX_VOLUME_DB)),
        AutomationTarget::Pan => Ok((-1.0, 1.0)),
        AutomationTarget::InstrumentParam { param } => {
            let kind = track.instrument.kind;
            instrument::spec(kind, param)
                .map(|s| (s.min, s.max))
                .ok_or_else(|| CommandError::UnknownParam {
                    kind: format!("{kind:?}"),
                    param: param.clone(),
                })
        }
        AutomationTarget::EffectParam { effect_id, param } => {
            let fx = track
                .mixer
                .effects
                .iter()
                .find(|e| e.id == *effect_id)
                .ok_or(CommandError::UnknownEffect(*effect_id))?;
            effect::effect_spec(fx.kind, param)
                .map(|s| (s.min, s.max))
                .ok_or_else(|| CommandError::UnknownParam {
                    kind: format!("{:?}", fx.kind),
                    param: param.clone(),
                })
        }
    }
}

/// Checks and sorts points for `target` on `track`.
fn checked_points(
    track: &Track,
    target: &AutomationTarget,
    mut points: Vec<AutomationPoint>,
) -> Result<Vec<AutomationPoint>, CommandError> {
    if points.len() > MAX_AUTOMATION_POINTS {
        return Err(invalid(
            "automation",
            format!("can have at most {MAX_AUTOMATION_POINTS} points"),
        ));
    }
    let (min, max) = target_range(track, target)?;
    for p in &points {
        check_position("automation point", p.beats)?;
        if !(p.value.is_finite() && (min..=max).contains(&p.value)) {
            return Err(invalid(
                "automation value",
                format!("must be between {min} and {max}, got {}", p.value),
            ));
        }
    }
    points.sort_by(|a, b| a.beats.total_cmp(&b.beats));
    Ok(points)
}

fn track_mut(project: &mut Project, id: TrackId) -> Result<&mut Track, CommandError> {
    project.track_mut(id).ok_or(CommandError::UnknownTrack(id))
}

fn lane_index(track: &Track, lane_id: LaneId) -> Result<usize, CommandError> {
    track
        .automation
        .iter()
        .position(|l| l.id == lane_id)
        .ok_or_else(|| invalid("automation lane", format!("{lane_id} isn't on this track")))
}

pub(super) fn add(
    project: &mut Project,
    track_id: TrackId,
    target: AutomationTarget,
    points: Vec<AutomationPoint>,
) -> Result<Command, CommandError> {
    let track = project
        .track(track_id)
        .ok_or(CommandError::UnknownTrack(track_id))?;
    if track.automation.iter().any(|l| l.target == target) {
        return Err(invalid(
            "automation lane",
            "for that setting already exists on this track; change its points instead",
        ));
    }
    let points = checked_points(track, &target, points)?;
    let id = project.allocate_id();
    track_mut(project, track_id)?
        .automation
        .push(AutomationLane {
            id,
            target,
            enabled: true,
            points,
        });
    Ok(Command::RemoveAutomationLane {
        track_id,
        lane_id: id,
    })
}

pub(super) fn remove(
    project: &mut Project,
    track_id: TrackId,
    lane_id: LaneId,
) -> Result<Command, CommandError> {
    let track = track_mut(project, track_id)?;
    let index = lane_index(track, lane_id)?;
    let lane = track.automation.remove(index);
    Ok(Command::RestoreAutomationLane {
        track_id,
        index,
        lane,
    })
}

pub(super) fn restore(
    project: &mut Project,
    track_id: TrackId,
    index: usize,
    mut lane: AutomationLane,
) -> Result<Command, CommandError> {
    if project.id_in_use(lane.id) {
        return Err(CommandError::IdInUse(lane.id));
    }
    let track = project
        .track(track_id)
        .ok_or(CommandError::UnknownTrack(track_id))?;
    lane.points = checked_points(track, &lane.target, lane.points)?;
    project.reserve_id(lane.id);
    let lane_id = lane.id;
    let track = track_mut(project, track_id)?;
    let index = index.min(track.automation.len());
    track.automation.insert(index, lane);
    Ok(Command::RemoveAutomationLane { track_id, lane_id })
}

pub(super) fn set_points(
    project: &mut Project,
    track_id: TrackId,
    lane_id: LaneId,
    points: Vec<AutomationPoint>,
) -> Result<Command, CommandError> {
    let track = project
        .track(track_id)
        .ok_or(CommandError::UnknownTrack(track_id))?;
    let index = lane_index(track, lane_id)?;
    let points = checked_points(track, &track.automation[index].target, points)?;
    let lane = &mut track_mut(project, track_id)?.automation[index];
    let old = std::mem::replace(&mut lane.points, points);
    Ok(Command::SetAutomationPoints {
        track_id,
        lane_id,
        points: old,
    })
}

pub(super) fn set_enabled(
    project: &mut Project,
    track_id: TrackId,
    lane_id: LaneId,
    enabled: bool,
) -> Result<Command, CommandError> {
    let track = track_mut(project, track_id)?;
    let index = lane_index(track, lane_id)?;
    let old = std::mem::replace(&mut track.automation[index].enabled, enabled);
    Ok(Command::SetAutomationEnabled {
        track_id,
        lane_id,
        enabled: old,
    })
}
