use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::project::{MAX_TEMPO_BPM, MIN_TEMPO_BPM, Project, TimeSignature};

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
        }
    }
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
        ];
        for command in bad {
            let mut project = Project::default();
            assert!(command.clone().apply(&mut project).is_err(), "{command:?}");
            assert_eq!(project, Project::default());
        }
    }

    #[test]
    fn schema_carries_descriptions_for_claude() {
        let schema = command_schema().to_string();
        assert!(schema.contains("set_tempo"));
        assert!(schema.contains("Set the project tempo in beats per minute"));
    }
}
