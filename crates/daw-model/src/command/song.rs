use super::{Command, CommandError, check_length, check_name, check_position, invalid};
use crate::project::{MAX_TEMPO_BPM, MIN_TEMPO_BPM, Project, TimeSignature};

pub(super) fn rename(project: &mut Project, name: String) -> Result<Command, CommandError> {
    let name = check_name(name)?;
    let old = std::mem::replace(&mut project.name, name);
    Ok(Command::RenameProject { name: old })
}

pub(super) fn set_tempo(project: &mut Project, bpm: f64) -> Result<Command, CommandError> {
    // `contains` is false for NaN, so NaN is rejected too.
    if !(MIN_TEMPO_BPM..=MAX_TEMPO_BPM).contains(&bpm) {
        return Err(CommandError::TempoOutOfRange(bpm));
    }
    let old = std::mem::replace(&mut project.tempo_bpm, bpm);
    Ok(Command::SetTempo { bpm: old })
}

pub(super) fn set_time_signature(
    project: &mut Project,
    numerator: u8,
    denominator: u8,
) -> Result<Command, CommandError> {
    let valid_denominator = matches!(denominator, 1 | 2 | 4 | 8 | 16 | 32);
    if !(1..=32).contains(&numerator) || !valid_denominator {
        return Err(CommandError::InvalidTimeSignature(numerator, denominator));
    }
    let old = std::mem::replace(
        &mut project.time_signature,
        TimeSignature {
            numerator,
            denominator,
        },
    );
    Ok(Command::SetTimeSignature {
        numerator: old.numerator,
        denominator: old.denominator,
    })
}

pub(super) fn set_loop(
    project: &mut Project,
    enabled: Option<bool>,
    start_beats: Option<f64>,
    end_beats: Option<f64>,
) -> Result<Command, CommandError> {
    let old = project.loop_region;
    let mut new = old;
    if let Some(e) = enabled {
        new.enabled = e;
    }
    if let Some(s) = start_beats {
        check_position("loop start", s)?;
        new.start_beats = s;
    }
    if let Some(e) = end_beats {
        check_position("loop end", e)?;
        new.end_beats = e;
    }
    check_length("loop", new.end_beats - new.start_beats)
        .map_err(|_| invalid("loop end", "must be after the loop start"))?;
    project.loop_region = new;
    Ok(Command::SetLoop {
        enabled: Some(old.enabled),
        start_beats: Some(old.start_beats),
        end_beats: Some(old.end_beats),
    })
}
