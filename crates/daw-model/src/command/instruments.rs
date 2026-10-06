use super::{Command, CommandError, check_value};
use crate::instrument::{self, Instrument, InstrumentKind};
use crate::project::{Project, TrackId};

pub(crate) fn kind_name(kind: InstrumentKind) -> String {
    format!("{kind:?}")
}

pub(super) fn set_param(
    project: &mut Project,
    track_id: TrackId,
    param: String,
    value: f64,
) -> Result<Command, CommandError> {
    let track = project
        .track_mut(track_id)
        .ok_or(CommandError::UnknownTrack(track_id))?;
    let kind = track.instrument.kind;
    let spec = instrument::spec(kind, &param).ok_or_else(|| CommandError::UnknownParam {
        kind: kind_name(kind),
        param: param.clone(),
    })?;
    check_value(spec, value)?;
    let old = track.instrument.value(&param).unwrap_or(spec.default);
    track.instrument.params.insert(param.clone(), value);
    Ok(Command::SetInstrumentParam {
        track_id,
        param,
        value: old,
    })
}

pub(super) fn load_preset(
    project: &mut Project,
    track_id: TrackId,
    preset: String,
) -> Result<Command, CommandError> {
    let track = project
        .track_mut(track_id)
        .ok_or(CommandError::UnknownTrack(track_id))?;
    let kind = track.instrument.kind;
    let loaded =
        Instrument::from_preset(kind, &preset).ok_or_else(|| CommandError::UnknownPreset {
            kind: kind_name(kind),
            preset,
        })?;
    let old = std::mem::replace(&mut track.instrument, loaded);
    Ok(Command::SetInstrument {
        track_id,
        instrument: old,
    })
}

pub(super) fn set_instrument(
    project: &mut Project,
    track_id: TrackId,
    instrument: Instrument,
) -> Result<Command, CommandError> {
    let complete = complete_instrument(instrument)?;
    let track = project
        .track_mut(track_id)
        .ok_or(CommandError::UnknownTrack(track_id))?;
    let old = std::mem::replace(&mut track.instrument, complete);
    Ok(Command::SetInstrument {
        track_id,
        instrument: old,
    })
}

/// Validates every given parameter and fills in defaults for missing ones.
pub(crate) fn complete_instrument(mut inst: Instrument) -> Result<Instrument, CommandError> {
    for (id, value) in &inst.params {
        let spec = instrument::spec(inst.kind, id).ok_or_else(|| CommandError::UnknownParam {
            kind: kind_name(inst.kind),
            param: id.clone(),
        })?;
        check_value(spec, *value)?;
    }
    for spec in instrument::param_specs(inst.kind) {
        inst.params
            .entry(spec.id.to_owned())
            .or_insert(spec.default);
    }
    Ok(inst)
}
