//! The tool list Claude sees. Project-editing tools are generated from the
//! `Command` JSON Schema, so the UI and Claude can never drift apart; a few
//! more tools cover reading the song, the transport, files, and analysis.

use serde_json::{Map, Value, json};

use daw_control::Request;

/// Commands that exist only so undo can restore things exactly.
const HIDDEN_COMMANDS: &[&str] = &["restore_track", "restore_clip", "restore_effect"];

/// A tool: its name, description, and JSON Schema for its arguments.
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Map<String, Value>,
    pub read_only: bool,
}

fn object_schema(properties: Value, required: &[&str]) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("type".into(), json!("object"));
    m.insert("properties".into(), properties);
    if !required.is_empty() {
        m.insert("required".into(), json!(required));
    }
    m
}

/// Replaces `{"$ref": "#"}` (the schema root) with a named reference.
fn rename_root_refs(v: &mut Value) {
    match v {
        Value::Object(o) => {
            if o.get("$ref").and_then(Value::as_str) == Some("#") {
                o.insert("$ref".into(), json!("#/$defs/Command"));
            }
            for child in o.values_mut() {
                rename_root_refs(child);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(rename_root_refs),
        _ => {}
    }
}

/// Tools generated from every `Command` variant, plus `batch`.
fn command_tools() -> Vec<ToolDef> {
    let mut schema = daw_model::command_schema();
    rename_root_refs(&mut schema);
    let defs = schema.get("$defs").cloned().unwrap_or_else(|| json!({}));
    let variants = schema
        .get("oneOf")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    // `Command` itself, for `batch`'s list of commands.
    let mut command_def = schema.clone();
    if let Some(o) = command_def.as_object_mut() {
        o.remove("$defs");
        o.remove("$schema");
    }
    let mut all_defs = defs.clone();
    if let Some(o) = all_defs.as_object_mut() {
        o.insert("Command".into(), command_def);
    }

    let mut tools = Vec::new();
    for variant in variants {
        let Some(name) = variant
            .pointer("/properties/command/const")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        if HIDDEN_COMMANDS.contains(&name.as_str()) {
            continue;
        }
        let description = variant
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .replace('\n', " ");
        let mut properties = variant
            .get("properties")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        properties.remove("command");
        let required: Vec<Value> = variant
            .get("required")
            .and_then(Value::as_array)
            .map(|r| {
                r.iter()
                    .filter(|v| v.as_str() != Some("command"))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let mut input = Map::new();
        input.insert("type".into(), json!("object"));
        input.insert("properties".into(), Value::Object(properties));
        if !required.is_empty() {
            input.insert("required".into(), Value::Array(required));
        }
        let uses_refs = serde_json::to_string(&input).is_ok_and(|s| s.contains("$ref"));
        if uses_refs {
            input.insert(
                "$defs".into(),
                if name == "batch" {
                    all_defs.clone()
                } else {
                    defs.clone()
                },
            );
        }
        tools.push(ToolDef {
            name,
            description,
            input_schema: input,
            read_only: false,
        });
    }
    tools
}

fn extra_tools() -> Vec<ToolDef> {
    let num = |d: &str| json!({ "type": "number", "description": d });
    let int = |d: &str| json!({ "type": "integer", "minimum": 0, "description": d });
    let tool =
        |name: &str, description: &str, schema: Map<String, Value>, read_only: bool| ToolDef {
            name: name.into(),
            description: description.into(),
            input_schema: schema,
            read_only,
        };
    vec![
        tool(
            "get_song",
            "Overview of the open song: tempo, time signature, length, loop, master, and every track with its clips (ids, positions, note counts). Start here.",
            object_schema(json!({}), &[]),
            true,
        ),
        tool(
            "get_track",
            "One track in full: instrument parameters, effect settings, mixer, and clip list.",
            object_schema(
                json!({ "track_id": int("Track id from get_song.") }),
                &["track_id"],
            ),
            true,
        ),
        tool(
            "get_clip",
            "One clip with every note (pitch, start_beats relative to the clip, length_beats, velocity, id).",
            object_schema(
                json!({ "clip_id": int("Clip id from get_song.") }),
                &["clip_id"],
            ),
            true,
        ),
        tool(
            "describe_instruments",
            "Every built-in instrument and effect: parameter ids, ranges, units, choices, factory presets, and the drum pad note map. Use before changing sounds.",
            object_schema(json!({}), &[]),
            true,
        ),
        tool(
            "undo",
            "Undo the most recent change (the user's or yours).",
            object_schema(json!({}), &[]),
            false,
        ),
        tool(
            "redo",
            "Redo the most recently undone change.",
            object_schema(json!({}), &[]),
            false,
        ),
        tool(
            "play",
            "Start playback through the user's speakers, optionally from a position in beats.",
            object_schema(
                json!({ "from_beats": num("Where to start, in beats.") }),
                &[],
            ),
            false,
        ),
        tool(
            "stop",
            "Stop playback. Stopping again while stopped rewinds to the start.",
            object_schema(json!({}), &[]),
            false,
        ),
        tool(
            "locate",
            "Move the playhead to a position in beats.",
            object_schema(json!({ "beats": num("Position in beats.") }), &["beats"]),
            false,
        ),
        tool(
            "set_metronome",
            "Turn the metronome click on or off.",
            object_schema(json!({ "on": { "type": "boolean" } }), &["on"]),
            false,
        ),
        tool(
            "transport_status",
            "Whether the song is playing and where the playhead is.",
            object_schema(json!({}), &[]),
            true,
        ),
        tool(
            "analyze_mix",
            "Render the song (or a region) offline and measure it, since you can't hear it: integrated loudness (LUFS), true peak, stereo correlation, energy per frequency band, per-track levels, and plain-language hints. Set spectrogram=true to also get a picture of the sound (time left to right, log frequency 30 Hz-20 kHz bottom to top, dashed guides at 100 Hz, 1 kHz, 10 kHz). Use after making changes to check the result.",
            object_schema(
                json!({
                    "start_beats": num("Start of the region (default: song start)."),
                    "end_beats": num("End of the region (default: end of the last clip)."),
                    "per_track": { "type": "boolean", "description": "Also measure each track alone (default true)." },
                    "spectrogram": { "type": "boolean", "description": "Also return a spectrogram image (default false)." }
                }),
                &[],
            ),
            true,
        ),
        tool(
            "export_wav",
            "Render the song (or a region) to a 24-bit, 48 kHz WAV file at an absolute path.",
            object_schema(
                json!({
                    "path": { "type": "string", "description": "Absolute path ending in .wav." },
                    "start_beats": num("Start (default 0)."),
                    "end_beats": num("End (default: end of the last clip)."),
                    "tail_seconds": num("Extra seconds for reverb/echo tails (default 2).")
                }),
                &["path"],
            ),
            false,
        ),
        tool(
            "save_project",
            "Save the project. Without a path, saves to the file it was opened from or last saved to.",
            object_schema(
                json!({ "path": { "type": "string", "description": "Absolute path; .nptune is added if missing." } }),
                &[],
            ),
            false,
        ),
        tool(
            "open_project",
            "Open a .nptune project file, replacing the open song. Ask the user first if they may have unsaved work.",
            object_schema(json!({ "path": { "type": "string" } }), &["path"]),
            false,
        ),
        tool(
            "new_project",
            "Start a new, empty song with Keys, Bass, and Drums tracks. Ask the user first if they may have unsaved work.",
            object_schema(json!({}), &[]),
            false,
        ),
        tool(
            "import_audio",
            "Bring an audio file (wav, mp3, m4a/AAC from a phone, flac, ogg) into the song as an audio clip. The file is copied into the project's audio folder. Without track_id, a new audio track named after the file is added. One undo step.",
            object_schema(
                json!({
                    "path": { "type": "string", "description": "Absolute path of the audio file." },
                    "track_id": { "type": "integer", "description": "An existing audio track (instrument \"audio\"); omit to add a new track." },
                    "start_beats": num("Where the clip starts (default 0, the song start).")
                }),
                &["path"],
            ),
            false,
        ),
        tool(
            "record_audio",
            "Start recording the user's microphone onto an audio track. The song plays from the current position so they can play along; looping pauses while recording. Only do this when the user asks to record, then call stop_recording when they say they're done.",
            object_schema(
                json!({ "track_id": { "type": "integer", "description": "An audio track (instrument \"audio\")." } }),
                &["track_id"],
            ),
            false,
        ),
        tool(
            "stop_recording",
            "Stop recording audio. The take becomes a clip on the track, lined up with the beat the user heard while playing.",
            object_schema(json!({}), &[]),
            false,
        ),
    ]
}

/// Every tool, commands first.
pub fn all_tools() -> Vec<ToolDef> {
    let mut tools = command_tools();
    tools.extend(extra_tools());
    tools
}

/// Turns a tool call into a request for the app.
pub fn to_request(name: &str, args: Map<String, Value>) -> Result<Request, String> {
    let get_f64 = |k: &str| args.get(k).and_then(Value::as_f64);
    let get_bool = |k: &str| args.get(k).and_then(Value::as_bool);
    let get_str = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_owned);
    let get_id = |k: &str| {
        args.get(k)
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| format!("{k} must be a non-negative integer id"))
    };
    let request = match name {
        "get_song" => Request::GetSong,
        "get_track" => Request::GetTrack {
            track_id: get_id("track_id")?,
        },
        "get_clip" => Request::GetClip {
            clip_id: get_id("clip_id")?,
        },
        "describe_instruments" => Request::Describe,
        "undo" => Request::Undo,
        "redo" => Request::Redo,
        "play" => Request::Play {
            from_beats: get_f64("from_beats"),
        },
        "stop" => Request::Stop,
        "locate" => Request::Locate {
            beats: get_f64("beats").ok_or("beats is required")?,
        },
        "set_metronome" => Request::SetMetronome {
            on: get_bool("on").ok_or("on is required")?,
        },
        "transport_status" => Request::Status,
        "analyze_mix" => Request::Analyze {
            start_beats: get_f64("start_beats"),
            end_beats: get_f64("end_beats"),
            per_track: get_bool("per_track"),
            spectrogram: get_bool("spectrogram"),
        },
        "export_wav" => Request::ExportWav {
            path: get_str("path").ok_or("path is required")?,
            start_beats: get_f64("start_beats"),
            end_beats: get_f64("end_beats"),
            tail_seconds: get_f64("tail_seconds"),
        },
        "save_project" => Request::Save {
            path: get_str("path"),
        },
        "open_project" => Request::Open {
            path: get_str("path").ok_or("path is required")?,
        },
        "new_project" => Request::New,
        "import_audio" => Request::ImportAudio {
            path: get_str("path").ok_or("path is required")?,
            track_id: args
                .get("track_id")
                .map(|_| get_id("track_id"))
                .transpose()?,
            start_beats: get_f64("start_beats"),
        },
        "record_audio" => Request::RecordAudio {
            track_id: get_id("track_id")?,
        },
        "stop_recording" => Request::StopRecording,
        command => {
            if HIDDEN_COMMANDS.contains(&command) {
                return Err(format!("unknown tool {command}"));
            }
            let mut obj = args;
            obj.insert("command".into(), json!(command));
            let command: daw_model::Command = serde_json::from_value(Value::Object(obj))
                .map_err(|e| format!("invalid arguments for {name}: {e}"))?;
            Request::Execute { command }
        }
    };
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_visible_command_becomes_a_tool() {
        let tools = all_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        for expected in [
            "set_tempo",
            "create_clip",
            "add_notes",
            "add_effect",
            "batch",
            "get_song",
            "analyze_mix",
        ] {
            assert!(names.contains(&expected), "missing {expected}");
        }
        for hidden in HIDDEN_COMMANDS {
            assert!(!names.contains(hidden));
        }
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "duplicate tool names");
        for t in &tools {
            assert_eq!(t.input_schema["type"], "object", "{}", t.name);
            assert!(!t.description.is_empty(), "{} has no description", t.name);
            let text = serde_json::to_string(&t.input_schema).expect("json");
            assert!(
                !text.contains("\"#\""),
                "{} has a dangling root ref",
                t.name
            );
            if text.contains("$ref") {
                assert!(
                    t.input_schema.contains_key("$defs"),
                    "{} lacks $defs",
                    t.name
                );
            }
        }
    }

    #[test]
    fn command_tool_calls_become_execute_requests() {
        let args = json!({ "bpm": 128.0 }).as_object().cloned().expect("obj");
        let r = to_request("set_tempo", args).expect("ok");
        assert_eq!(
            r,
            Request::Execute {
                command: daw_model::Command::SetTempo { bpm: 128.0 }
            }
        );
    }

    #[test]
    fn batch_accepts_nested_commands() {
        let args = json!({
            "commands": [
                { "command": "set_tempo", "bpm": 90 },
                { "command": "add_track", "name": "Lead", "instrument": "synth", "preset": "Bright Lead", "index": null }
            ]
        })
        .as_object()
        .cloned()
        .expect("obj");
        let Request::Execute {
            command: daw_model::Command::Batch { commands },
        } = to_request("batch", args).expect("ok")
        else {
            panic!("not a batch");
        };
        assert_eq!(commands.len(), 2);
    }

    #[test]
    fn bad_arguments_explain_themselves() {
        let args = json!({ "bpm": "fast" }).as_object().cloned().expect("obj");
        let err = to_request("set_tempo", args).expect_err("bad");
        assert!(err.contains("invalid arguments for set_tempo"), "{err}");
        assert!(to_request("restore_track", Map::new()).is_err());
        assert!(to_request("get_clip", Map::new()).is_err());
    }
}
