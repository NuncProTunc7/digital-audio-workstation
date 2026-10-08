use super::{Command, CommandError, check_name, invalid};
use crate::instrument::{Instrument, InstrumentKind, presets};
use crate::project::{MAX_TRACKS, MAX_VOLUME_DB, MIN_VOLUME_DB, Mixer, Project, Track, TrackId};

use super::instruments::{complete_instrument, kind_name};

fn check_volume(volume_db: f64) -> Result<(), CommandError> {
    if volume_db.is_finite() && (MIN_VOLUME_DB..=MAX_VOLUME_DB).contains(&volume_db) {
        Ok(())
    } else {
        Err(invalid(
            "volume",
            format!("must be between {MIN_VOLUME_DB} and {MAX_VOLUME_DB} dB, got {volume_db}"),
        ))
    }
}

pub(super) fn add(
    project: &mut Project,
    name: String,
    kind: InstrumentKind,
    preset: Option<String>,
    index: Option<usize>,
) -> Result<Command, CommandError> {
    let name = check_name(name)?;
    if kind == InstrumentKind::Plugin {
        return Err(super::invalid(
            "instrument",
            "add a track with any instrument, then choose its plugin with load_plugin",
        ));
    }
    if project.tracks.len() >= MAX_TRACKS {
        return Err(CommandError::TooManyTracks(MAX_TRACKS));
    }
    let preset = preset.unwrap_or_else(|| {
        presets(kind)
            .first()
            .map_or_else(String::new, |p| p.name.to_owned())
    });
    let instrument =
        Instrument::from_preset(kind, &preset).ok_or_else(|| CommandError::UnknownPreset {
            kind: kind_name(kind),
            preset,
        })?;
    let id = project.allocate_id();
    let index = index
        .unwrap_or(project.tracks.len())
        .min(project.tracks.len());
    project.tracks.insert(
        index,
        Track {
            frozen: None,
            output: None,
            sends: Vec::new(),
            id,
            name,
            instrument,
            mixer: Mixer::default(),
            clips: Vec::new(),
            automation: Vec::new(),
        },
    );
    Ok(Command::RemoveTrack { track_id: id })
}

pub(super) fn remove(project: &mut Project, track_id: TrackId) -> Result<Command, CommandError> {
    let index = project
        .track_index(track_id)
        .ok_or(CommandError::UnknownTrack(track_id))?;
    let track = project.tracks.remove(index);
    Ok(Command::RestoreTrack { index, track })
}

pub(super) fn restore(
    project: &mut Project,
    index: usize,
    mut track: Track,
) -> Result<Command, CommandError> {
    if project.tracks.len() >= MAX_TRACKS {
        return Err(CommandError::TooManyTracks(MAX_TRACKS));
    }
    track.name = check_name(track.name)?;
    track.instrument = complete_instrument(track.instrument)?;
    for clip in &track.clips {
        super::audio::check_clip_fits(track.instrument.kind, clip)?;
    }
    // Every id inside the track must be free.
    let probe = Project {
        tracks: vec![track.clone()],
        ..Project::default()
    };
    for id in probe.all_ids() {
        if project.id_in_use(id) {
            return Err(CommandError::IdInUse(id));
        }
    }
    for id in probe.all_ids() {
        project.reserve_id(id);
    }
    let track_id = track.id;
    let index = index.min(project.tracks.len());
    project.tracks.insert(index, track);
    Ok(Command::RemoveTrack { track_id })
}

pub(super) fn rename(
    project: &mut Project,
    track_id: TrackId,
    name: String,
) -> Result<Command, CommandError> {
    let name = check_name(name)?;
    let track = project
        .track_mut(track_id)
        .ok_or(CommandError::UnknownTrack(track_id))?;
    let old = std::mem::replace(&mut track.name, name);
    Ok(Command::RenameTrack {
        track_id,
        name: old,
    })
}

pub(super) fn move_to(
    project: &mut Project,
    track_id: TrackId,
    index: usize,
) -> Result<Command, CommandError> {
    let from = project
        .track_index(track_id)
        .ok_or(CommandError::UnknownTrack(track_id))?;
    let track = project.tracks.remove(from);
    let to = index.min(project.tracks.len());
    project.tracks.insert(to, track);
    Ok(Command::MoveTrack {
        track_id,
        index: from,
    })
}

pub(super) fn set_mixer(
    project: &mut Project,
    track_id: TrackId,
    volume_db: Option<f64>,
    pan: Option<f64>,
    mute: Option<bool>,
    solo: Option<bool>,
) -> Result<Command, CommandError> {
    if let Some(v) = volume_db {
        check_volume(v)?;
    }
    if let Some(p) = pan
        && !(p.is_finite() && (-1.0..=1.0).contains(&p))
    {
        return Err(invalid("pan", format!("must be between -1 and 1, got {p}")));
    }
    let track = project
        .track_mut(track_id)
        .ok_or(CommandError::UnknownTrack(track_id))?;
    let m = &mut track.mixer;
    let inverse = Command::SetTrackMixer {
        track_id,
        volume_db: volume_db.map(|_| m.volume_db),
        pan: pan.map(|_| m.pan),
        mute: mute.map(|_| m.mute),
        solo: solo.map(|_| m.solo),
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
    if let Some(x) = solo {
        m.solo = x;
    }
    Ok(inverse)
}

pub(super) fn set_master_volume(
    project: &mut Project,
    volume_db: f64,
) -> Result<Command, CommandError> {
    check_volume(volume_db)?;
    let old = std::mem::replace(&mut project.master.volume_db, volume_db);
    Ok(Command::SetMasterVolume { volume_db: old })
}
