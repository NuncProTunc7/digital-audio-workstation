use std::collections::BTreeMap;
use std::sync::LazyLock;

use daw_model::{Command, CommandError, Id, Project};
use serde_json::{Value, json};

/// Commands used internally to restore state during undo.
pub const HIDDEN_COMMANDS: &[&str] = &[
    "restore_track",
    "restore_clip",
    "restore_effect",
    "restore_automation_lane",
    "restore_snapshot",
    "restore_marker",
    "restore_tempo_change",
    "restore_bus",
    "restore_chord",
    "restore_sound",
    "set_song_state",
    "set_clip_link",
    "set_clip_notes",
];

static SCHEMA: LazyLock<Value> = LazyLock::new(daw_model::command_schema);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Track,
    Clip,
}

pub(crate) struct Batch {
    pub commands: Vec<Command>,
    refs: BTreeMap<String, (Kind, Id)>,
}

impl Batch {
    pub fn prepare(raw: &[Value], project: &mut Project) -> Result<Self, String> {
        if raw.is_empty() {
            return Err("commands must contain at least one edit".into());
        }
        let mut batch = Self {
            commands: Vec::new(),
            refs: BTreeMap::new(),
        };
        batch.steps(raw, project, "commands")?;
        if batch.commands.is_empty() {
            return Err("commands must contain at least one edit".into());
        }
        Ok(batch)
    }

    pub fn refs(&self) -> Value {
        json!(
            self.refs
                .iter()
                .map(|(name, (_, id))| (name, id))
                .collect::<BTreeMap<_, _>>()
        )
    }

    fn steps(&mut self, raw: &[Value], project: &mut Project, path: &str) -> Result<(), String> {
        for (index, step) in raw.iter().enumerate() {
            self.step(step, project, &format!("{path}[{index}]"))?;
        }
        Ok(())
    }

    fn step(&mut self, step: &Value, project: &mut Project, path: &str) -> Result<(), String> {
        let mut fields = step
            .as_object()
            .cloned()
            .ok_or_else(|| format!("{path} must be an object"))?;
        let name = step["command"]
            .as_str()
            .ok_or_else(|| format!("{path}.command must be a command name"))?;
        let variant = SCHEMA["oneOf"]
            .as_array()
            .and_then(|variants| {
                variants
                    .iter()
                    .find(|v| v["properties"]["command"]["const"] == name)
            })
            .filter(|_| !HIDDEN_COMMANDS.contains(&name))
            .ok_or_else(|| format!("{path}.command: unknown command {name:?}"))?;
        let properties = &variant["properties"];
        if name == "batch" {
            if fields
                .keys()
                .any(|key| key != "command" && key != "commands")
            {
                return Err(format!(
                    "{path}: a nested batch accepts only command and commands"
                ));
            }
            let nested = step["commands"]
                .as_array()
                .ok_or_else(|| format!("{path}.commands must be an array"))?;
            return self.steps(nested, project, &format!("{path}.commands"));
        }

        let binding = fields
            .remove("ref")
            .map(|value| {
                let name = value
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| format!("{path}.ref must be a nonempty string"))?;
                if !matches!(step["command"].as_str(), Some("add_track" | "create_clip")) {
                    return Err(format!(
                        "{path}.ref is available on add_track and create_clip"
                    ));
                }
                if self.refs.contains_key(name) {
                    return Err(format!("{path}.ref: {name:?} is already defined"));
                }
                Ok(name.to_owned())
            })
            .transpose()?;

        for (reference, id_field, kind) in [
            ("track_ref", "track_id", Kind::Track),
            ("clip_ref", "clip_id", Kind::Clip),
        ] {
            let Some(value) = fields.remove(reference) else {
                continue;
            };
            if properties.get(id_field).is_none() || fields.contains_key(id_field) {
                return Err(format!(
                    "{path}.{reference} requires a command accepting {id_field} and must replace that field"
                ));
            }
            let name = value
                .as_str()
                .ok_or_else(|| format!("{path}.{reference} must be a string"))?;
            let &(actual_kind, id) = self.refs.get(name).ok_or_else(|| {
                format!("{path}.{reference}: {name:?} has not been defined by an earlier command")
            })?;
            if actual_kind != kind {
                return Err(format!(
                    "{path}.{reference}: {name:?} refers to a different kind of object"
                ));
            }
            fields.insert(id_field.into(), json!(id));
        }
        for (key, value) in &fields {
            let schema = properties
                .get(key)
                .ok_or_else(|| format!("{path}.{key}: unknown field for {name}"))?;
            if let Some(kind) = schema["type"].as_str()
                && !matches_type(value, kind)
            {
                return Err(format!("{path}.{key}: expected {kind}"));
            }
        }
        let command: Command = serde_json::from_value(Value::Object(fields)).map_err(|e| {
            if let Some(field) = e
                .to_string()
                .strip_prefix("missing field `")
                .and_then(|s| s.strip_suffix('`'))
            {
                format!("{path}.{field}: required")
            } else {
                format!("{path} ({name}): {e}")
            }
        })?;
        let inverse = command.clone().apply(project).map_err(|e| {
            let field = match &e {
                CommandError::EmptyName => ".name",
                CommandError::TempoOutOfRange(_) => ".bpm",
                CommandError::UnknownTrack(_) => {
                    if step.get("track_ref").is_some() {
                        ".track_ref"
                    } else {
                        ".track_id"
                    }
                }
                CommandError::UnknownClip(_) => {
                    if step.get("clip_ref").is_some() {
                        ".clip_ref"
                    } else {
                        ".clip_id"
                    }
                }
                CommandError::UnknownPreset { .. } => ".preset",
                CommandError::UnknownEffect(_) => ".effect_id",
                _ => "",
            };
            format!("{path}{field}: {e}")
        })?;
        if let Some(binding) = binding {
            let target = match inverse {
                Command::RemoveTrack { track_id } => (Kind::Track, track_id),
                Command::DeleteClip { clip_id } => (Kind::Clip, clip_id),
                _ => return Err(format!("{path}.ref: the command did not create an object")),
            };
            self.refs.insert(binding, target);
        }
        self.commands.push(command);
        Ok(())
    }
}

fn matches_type(value: &Value, kind: &str) -> bool {
    match kind {
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        _ => true,
    }
}

pub(crate) fn changes(before: &Project, after: &Project) -> Value {
    fn collect<'a>(value: &'a Value, out: &mut BTreeMap<u64, &'a Value>) {
        match value {
            Value::Object(fields) => {
                if let Some(id) = value["id"].as_u64() {
                    out.insert(id, value);
                }
                // A snapshot's song intentionally reuses existing IDs.
                for (key, child) in fields {
                    if key != "song" {
                        collect(child, out);
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|item| collect(item, out)),
            _ => {}
        }
    }
    let before_json = json!(before);
    let after_json = json!(after);
    let mut before = BTreeMap::new();
    let mut after = BTreeMap::new();
    collect(&before_json, &mut before);
    collect(&after_json, &mut after);
    let created: Vec<_> = after.keys().filter(|id| !before.contains_key(id)).collect();
    let changed: Vec<_> = after
        .iter()
        .filter(|(id, value)| before.get(id).is_some_and(|old| old != *value))
        .map(|(id, _)| id)
        .collect();
    let removed: Vec<_> = before.keys().filter(|id| !after.contains_key(id)).collect();
    json!({"created_ids": created, "changed_ids": changed, "removed_ids": removed})
}
