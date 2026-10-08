//! Buses: group tracks (drums, music, ambience) and share effects (one
//! reverb for several tracks). A track plays into the master or one bus,
//! and can also send some of its sound to other buses. Buses play into the
//! master.

use super::{Command, CommandError, check_name, invalid};
use crate::project::{Bus, Id, MAX_VOLUME_DB, MIN_VOLUME_DB, Mixer, Project, Send, TrackId};

/// Most buses a song can have.
pub const MAX_BUSES: usize = 16;

fn bus_index(project: &Project, bus_id: Id) -> Result<usize, CommandError> {
    project
        .buses
        .iter()
        .position(|b| b.id == bus_id)
        .ok_or_else(|| invalid("bus", format!("there is no bus with id {bus_id}")))
}

fn check_level(what: &str, db: f64) -> Result<(), CommandError> {
    if db.is_finite() && (MIN_VOLUME_DB..=MAX_VOLUME_DB).contains(&db) {
        Ok(())
    } else {
        Err(invalid(
            what,
            format!("must be {MIN_VOLUME_DB} to {MAX_VOLUME_DB} dB, got {db}"),
        ))
    }
}

pub(super) fn add(project: &mut Project, name: String) -> Result<Command, CommandError> {
    let name = check_name(name)?;
    if project.buses.len() >= MAX_BUSES {
        return Err(invalid(
            "buses",
            format!("a song can have at most {MAX_BUSES}"),
        ));
    }
    let id = project.allocate_id();
    project.buses.push(Bus {
        id,
        name,
        mixer: Mixer::default(),
    });
    Ok(Command::RemoveBus { bus_id: id })
}

pub(super) fn remove(project: &mut Project, bus_id: Id) -> Result<Command, CommandError> {
    let index = bus_index(project, bus_id)?;
    let bus = project.buses.remove(index);
    // Whatever played into the bus plays into the master instead.
    let mut outputs = Vec::new();
    let mut sends = Vec::new();
    for t in &mut project.tracks {
        if t.output == Some(bus_id) {
            t.output = None;
            outputs.push(t.id);
        }
        if let Some(i) = t.sends.iter().position(|s| s.bus_id == bus_id) {
            sends.push((t.id, t.sends.remove(i)));
        }
    }
    Ok(Command::RestoreBus {
        bus,
        index,
        outputs,
        sends,
    })
}

pub(super) fn restore(
    project: &mut Project,
    bus: Bus,
    index: usize,
    outputs: Vec<TrackId>,
    sends: Vec<(TrackId, Send)>,
) -> Result<Command, CommandError> {
    let name = check_name(bus.name.clone())?;
    if project.id_in_use(bus.id) {
        return Err(CommandError::IdInUse(bus.id));
    }
    if let Some(e) = bus.mixer.effects.iter().find(|e| project.id_in_use(e.id)) {
        return Err(CommandError::IdInUse(e.id));
    }
    for t in outputs.iter().chain(sends.iter().map(|(t, _)| t)) {
        if project.track(*t).is_none() {
            return Err(CommandError::UnknownTrack(*t));
        }
    }
    let id = bus.id;
    project.reserve_id(id);
    for e in &bus.mixer.effects {
        project.reserve_id(e.id);
    }
    let index = index.min(project.buses.len());
    project.buses.insert(index, Bus { name, ..bus });
    for t in &mut project.tracks {
        if outputs.contains(&t.id) {
            t.output = Some(id);
        }
        for (track_id, send) in &sends {
            if *track_id == t.id && !t.sends.iter().any(|s| s.bus_id == id) {
                t.sends.push(send.clone());
            }
        }
    }
    Ok(Command::RemoveBus { bus_id: id })
}

pub(super) fn rename(
    project: &mut Project,
    bus_id: Id,
    name: String,
) -> Result<Command, CommandError> {
    let name = check_name(name)?;
    let index = bus_index(project, bus_id)?;
    let old = std::mem::replace(&mut project.buses[index].name, name);
    Ok(Command::RenameBus { bus_id, name: old })
}

pub(super) fn set_mixer(
    project: &mut Project,
    bus_id: Id,
    volume_db: Option<f64>,
    pan: Option<f64>,
    mute: Option<bool>,
) -> Result<Command, CommandError> {
    if let Some(v) = volume_db {
        check_level("bus volume", v)?;
    }
    if let Some(p) = pan
        && !(p.is_finite() && (-1.0..=1.0).contains(&p))
    {
        return Err(invalid("pan", format!("must be -1.0 to 1.0, got {p}")));
    }
    let index = bus_index(project, bus_id)?;
    let m = &mut project.buses[index].mixer;
    let inverse = Command::SetBusMixer {
        bus_id,
        volume_db: volume_db.map(|_| m.volume_db),
        pan: pan.map(|_| m.pan),
        mute: mute.map(|_| m.mute),
    };
    if let Some(v) = volume_db {
        m.volume_db = v;
    }
    if let Some(p) = pan {
        m.pan = p;
    }
    if let Some(x) = mute {
        m.mute = x;
    }
    Ok(inverse)
}

pub(super) fn set_output(
    project: &mut Project,
    track_id: TrackId,
    bus_id: Option<Id>,
) -> Result<Command, CommandError> {
    if let Some(b) = bus_id {
        bus_index(project, b)?;
    }
    let track = project
        .track_mut(track_id)
        .ok_or(CommandError::UnknownTrack(track_id))?;
    let old = std::mem::replace(&mut track.output, bus_id);
    Ok(Command::SetTrackOutput {
        track_id,
        bus_id: old,
    })
}

pub(super) fn set_send(
    project: &mut Project,
    track_id: TrackId,
    bus_id: Id,
    level_db: Option<f64>,
    pre_fader: Option<bool>,
) -> Result<Command, CommandError> {
    if let Some(v) = level_db {
        check_level("send level", v)?;
    }
    bus_index(project, bus_id)?;
    let track = project
        .track_mut(track_id)
        .ok_or(CommandError::UnknownTrack(track_id))?;
    match track.sends.iter_mut().find(|s| s.bus_id == bus_id) {
        Some(s) => {
            let inverse = Command::SetSend {
                track_id,
                bus_id,
                level_db: level_db.map(|_| s.level_db),
                pre_fader: pre_fader.map(|_| s.pre_fader),
            };
            if let Some(v) = level_db {
                s.level_db = v;
            }
            if let Some(p) = pre_fader {
                s.pre_fader = p;
            }
            Ok(inverse)
        }
        None => {
            track.sends.push(Send {
                bus_id,
                level_db: level_db.unwrap_or(Send::DEFAULT_LEVEL_DB),
                pre_fader: pre_fader.unwrap_or(false),
            });
            Ok(Command::RemoveSend { track_id, bus_id })
        }
    }
}

pub(super) fn remove_send(
    project: &mut Project,
    track_id: TrackId,
    bus_id: Id,
) -> Result<Command, CommandError> {
    let track = project
        .track_mut(track_id)
        .ok_or(CommandError::UnknownTrack(track_id))?;
    let i = track
        .sends
        .iter()
        .position(|s| s.bus_id == bus_id)
        .ok_or_else(|| {
            invalid(
                "send",
                format!("track {track_id} has no send to bus {bus_id}"),
            )
        })?;
    let s = track.sends.remove(i);
    Ok(Command::SetSend {
        track_id,
        bus_id,
        level_db: Some(s.level_db),
        pre_fader: Some(s.pre_fader),
    })
}
