use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::instrument::{self, Instrument};
use crate::project::{MAX_TEMPO_BPM, MIN_TEMPO_BPM, Project, TimeSignature, TrackId};

/// An edit to a project.
///
/// Doc comments on each variant become the tool descriptions Claude reads
/// over MCP, so write them for someone who has never seen the code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    /// Rename the project.
    RenameProject {
        /// New project name. Must not be empty.
        name: String,
    },
    /// Set the project tempo in beats per minute (20–999).
    SetTempo {
        /// Tempo in beats per minute.
        bpm: f64,
    },
    /// Set the time signature, for example 4/4, 3/4, or 6/8.
    SetTimeSignature {
        /// Beats per bar (1–32).
        numerator: u8,
        /// Note value of one beat: 1, 2, 4, 8, 16, or 32.
        denominator: u8,
    },
    /// Change one instrument parameter on a track, such as filter cutoff or
    /// attack time. Use `describe_instruments` to list parameter ids and ranges.
    /// Choice parameters (like `osc1.wave`) take the option's index.
    SetInstrumentParam {
        /// Track to change.
        track_id: TrackId,
        /// Parameter id, for example `filter.cutoff_hz`.
        param: String,
        /// New value, within the parameter's range.
        value: f64,
    },
    /// Load a factory preset onto a track's instrument, replacing all its
    /// parameters. Preset names are listed by `describe_instruments`.
    LoadPreset {
        /// Track to change.
        track_id: TrackId,
        /// Preset name, for example `Warm Keys` or `Acid Bass`.
        preset: String,
    },
    /// Replace a track's entire instrument settings at once. Parameters left
    /// out are set to their defaults.
    SetInstrument {
        /// Track to change.
        track_id: TrackId,
        /// The complete instrument settings.
        instrument: Instrument,
    },
}

/// Why a Command was rejected. A rejected Command leaves the project unchanged.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum CommandError {
    #[error("project name must not be empty")]
    EmptyName,
    #[error("tempo must be between {MIN_TEMPO_BPM} and {MAX_TEMPO_BPM} BPM, got {0}")]
    TempoOutOfRange(f64),
    #[error("time signature {0}/{1} is not supported")]
    InvalidTimeSignature(u8, u8),
    #[error("there is no track with id {0}")]
    UnknownTrack(TrackId),
    #[error("{kind} has no parameter named \"{param}\"")]
    UnknownParam { kind: String, param: String },
    #[error("{param} must be {expected}, got {value}")]
    ParamOutOfRange {
        param: String,
        expected: String,
        value: f64,
    },
    #[error("{kind} has no preset named \"{preset}\"")]
    UnknownPreset { kind: String, preset: String },
}

impl Command {
    /// Applies the edit and returns the Command that reverses it.
    pub fn apply(self, project: &mut Project) -> Result<Command, CommandError> {
        match self {
            Command::RenameProject { name } => {
                let name = name.trim().to_owned();
                if name.is_empty() {
                    return Err(CommandError::EmptyName);
                }
                let old = std::mem::replace(&mut project.name, name);
                Ok(Command::RenameProject { name: old })
            }
            Command::SetTempo { bpm } => {
                // `contains` is false for NaN, so NaN is rejected too.
                if !(MIN_TEMPO_BPM..=MAX_TEMPO_BPM).contains(&bpm) {
                    return Err(CommandError::TempoOutOfRange(bpm));
                }
                let old = std::mem::replace(&mut project.tempo_bpm, bpm);
                Ok(Command::SetTempo { bpm: old })
            }
            Command::SetTimeSignature {
                numerator,
                denominator,
            } => {
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
            Command::SetInstrumentParam {
                track_id,
                param,
                value,
            } => {
                let track = project
                    .track_mut(track_id)
                    .ok_or(CommandError::UnknownTrack(track_id))?;
                let kind = track.instrument.kind;
                let spec =
                    instrument::spec(kind, &param).ok_or_else(|| CommandError::UnknownParam {
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
            Command::LoadPreset { track_id, preset } => {
                let track = project
                    .track_mut(track_id)
                    .ok_or(CommandError::UnknownTrack(track_id))?;
                let kind = track.instrument.kind;
                let loaded = Instrument::from_preset(kind, &preset).ok_or_else(|| {
                    CommandError::UnknownPreset {
                        kind: kind_name(kind),
                        preset,
                    }
                })?;
                let old = std::mem::replace(&mut track.instrument, loaded);
                Ok(Command::SetInstrument {
                    track_id,
                    instrument: old,
                })
            }
            Command::SetInstrument {
                track_id,
                instrument,
            } => {
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
        }
    }

    /// True when `next` edits the same thing as `self`, so a drag of one
    /// slider becomes a single undo step instead of hundreds.
    pub(crate) fn coalesces_with(&self, next: &Command) -> bool {
        match (self, next) {
            (
                Command::SetInstrumentParam {
                    track_id: a,
                    param: pa,
                    ..
                },
                Command::SetInstrumentParam {
                    track_id: b,
                    param: pb,
                    ..
                },
            ) => a == b && pa == pb,
            _ => false,
        }
    }
}

fn kind_name(kind: instrument::InstrumentKind) -> String {
    format!("{kind:?}")
}

fn check_value(spec: &instrument::ParamSpec, value: f64) -> Result<(), CommandError> {
    if spec.validate(value) {
        return Ok(());
    }
    let expected = if spec.choices.is_empty() {
        format!("between {} and {}", spec.min, spec.max)
    } else {
        let options: Vec<String> = spec
            .choices
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{i} ({c})"))
            .collect();
        format!("one of {}", options.join(", "))
    };
    Err(CommandError::ParamOutOfRange {
        param: spec.id.to_owned(),
        expected,
        value,
    })
}

/// Validates every given parameter and fills in defaults for missing ones.
fn complete_instrument(mut inst: Instrument) -> Result<Instrument, CommandError> {
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

/// JSON Schema for [`Command`]. The MCP server turns each variant into a tool.
pub fn command_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Command)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_commands() -> Vec<Command> {
        vec![
            Command::RenameProject {
                name: "Boss Theme".into(),
            },
            Command::SetTempo { bpm: 140.0 },
            Command::SetTimeSignature {
                numerator: 6,
                denominator: 8,
            },
            Command::SetInstrumentParam {
                track_id: 1,
                param: "filter.cutoff_hz".into(),
                value: 440.0,
            },
            Command::LoadPreset {
                track_id: 2,
                preset: "Acid Bass".into(),
            },
            Command::SetInstrument {
                track_id: 1,
                instrument: Instrument::from_preset(
                    instrument::InstrumentKind::Synth,
                    "Chip Square",
                )
                .expect("preset"),
            },
        ]
    }

    #[test]
    fn commands_round_trip_through_json() {
        for command in all_commands() {
            let json = serde_json::to_string(&command).expect("serialize");
            let back: Command = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(command, back, "{json}");
        }
    }

    #[test]
    fn json_uses_snake_case_tag() {
        let json = serde_json::to_value(Command::SetTempo { bpm: 90.0 }).expect("serialize");
        assert_eq!(
            json,
            serde_json::json!({ "command": "set_tempo", "bpm": 90.0 })
        );
    }

    #[test]
    fn apply_then_inverse_restores_project() {
        for command in all_commands() {
            let mut project = Project::default();
            let inverse = command.clone().apply(&mut project).expect("apply");
            assert_ne!(project, Project::default(), "{command:?} changed nothing");
            inverse.apply(&mut project).expect("undo");
            assert_eq!(project, Project::default(), "{command:?} did not undo");
        }
    }

    #[test]
    fn invalid_commands_are_rejected_without_changes() {
        let bad = [
            Command::RenameProject { name: "  ".into() },
            Command::SetTempo { bpm: 5.0 },
            Command::SetTempo { bpm: f64::NAN },
            Command::SetTimeSignature {
                numerator: 4,
                denominator: 3,
            },
            Command::SetTimeSignature {
                numerator: 0,
                denominator: 4,
            },
            Command::SetInstrumentParam {
                track_id: 99,
                param: "filter.cutoff_hz".into(),
                value: 440.0,
            },
            Command::SetInstrumentParam {
                track_id: 1,
                param: "no.such.param".into(),
                value: 1.0,
            },
            Command::SetInstrumentParam {
                track_id: 1,
                param: "filter.cutoff_hz".into(),
                value: 99_999.0,
            },
            Command::SetInstrumentParam {
                track_id: 1,
                param: "osc1.wave".into(),
                value: 1.5,
            },
            // Drum tracks don't have synth parameters.
            Command::SetInstrumentParam {
                track_id: 3,
                param: "filter.cutoff_hz".into(),
                value: 440.0,
            },
            Command::LoadPreset {
                track_id: 1,
                preset: "Classic Kit".into(),
            },
        ];
        for command in bad {
            let mut project = Project::default();
            assert!(command.clone().apply(&mut project).is_err(), "{command:?}");
            assert_eq!(project, Project::default());
        }
    }

    #[test]
    fn out_of_range_error_tells_claude_the_valid_range() {
        let err = Command::SetInstrumentParam {
            track_id: 1,
            param: "osc1.wave".into(),
            value: 9.0,
        }
        .apply(&mut Project::default())
        .expect_err("rejected");
        assert!(err.to_string().contains("1 (Saw)"), "{err}");
    }

    #[test]
    fn set_instrument_fills_missing_params_with_defaults() {
        let mut project = Project::default();
        let sparse = Instrument {
            kind: instrument::InstrumentKind::Synth,
            preset: "Custom".into(),
            params: [("filter.cutoff_hz".to_owned(), 300.0)].into(),
        };
        Command::SetInstrument {
            track_id: 1,
            instrument: sparse,
        }
        .apply(&mut project)
        .expect("ok");
        let inst = &project.track(1).expect("track").instrument;
        assert_eq!(inst.params.len(), instrument::SYNTH_PARAMS.len());
        assert_eq!(inst.value("filter.cutoff_hz"), Some(300.0));
    }

    #[test]
    fn schema_carries_descriptions_for_claude() {
        let schema = command_schema().to_string();
        assert!(schema.contains("set_tempo"));
        assert!(schema.contains("Set the project tempo in beats per minute"));
    }
}
