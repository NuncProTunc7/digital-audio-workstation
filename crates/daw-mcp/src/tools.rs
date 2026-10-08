//! The tool list Claude sees. Project-editing tools are generated from the
//! `Command` JSON Schema, so the UI and Claude can never drift apart; a few
//! more tools cover reading the song, the transport, files, and analysis.

use serde_json::{Map, Value, json};

use daw_control::Request;

/// Commands that exist only so undo can restore things exactly.
const HIDDEN_COMMANDS: &[&str] = &[
    "restore_track",
    "restore_clip",
    "restore_effect",
    "restore_automation_lane",
    "restore_snapshot",
    "restore_marker",
    "restore_bus",
    "restore_chord",
    "set_song_state",
    "set_clip_link",
    "set_clip_notes",
];

/// Commands whose tool does more than the Command (freeze_track renders
/// the track first), so the generated tool of that name is left out.
const REPLACED_COMMANDS: &[&str] = &["freeze_track"];

/// The tool that reads the built-in user guide (answered by the bridge itself).
pub const READ_GUIDE: &str = "read_guide";

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
        if HIDDEN_COMMANDS.contains(&name.as_str()) || REPLACED_COMMANDS.contains(&name.as_str()) {
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
            "diagnostic_report",
            "Plain-text report for troubleshooting: app and Windows version, audio output and input devices, sample rate, buffer and latency, recording delay, CPU load and peak, overloads (each one is likely an audible crackle), MIDI keyboards, sample packs, the song's size, recent errors, and the last 50 log lines. Use it when the user reports a problem (crackles, silence, late recordings, a failed action) before guessing at causes. It contains no folder paths.",
            object_schema(json!({}), &[]),
            true,
        ),
        tool(
            "save_preset",
            "Save a track's current instrument settings as the user's own preset, under a name, so any song can use it (kept on this computer, listed after the built-in presets). Can't reuse a built-in preset's name; saving an existing name replaces it.",
            object_schema(
                json!({
                    "track_id": { "type": "integer" },
                    "name": { "type": "string", "description": "Up to 40 characters, e.g. \"Dungeon Pad\"." }
                }),
                &["track_id", "name"],
            ),
            false,
        ),
        tool(
            "user_presets",
            "List the user's own saved presets (name, instrument kind, parameter values). Built-in presets are in describe_instruments.",
            object_schema(json!({}), &[]),
            true,
        ),
        tool(
            "load_user_preset",
            "Give a track one of the user's own saved presets (one undo step). The preset must be for the track's instrument kind; for built-in presets use load_preset.",
            object_schema(
                json!({
                    "track_id": { "type": "integer" },
                    "name": { "type": "string" }
                }),
                &["track_id", "name"],
            ),
            false,
        ),
        tool(
            "set_count_in",
            "Set how many bars of metronome clicks play before recording starts (0 = none, 1 or 2 bars), so the performer can come in on the first beat. Applies to recording audio and notes, by the user or with record_audio. The song stays silent during the count-in. Saved on this computer, not in the song.",
            object_schema(
                json!({ "bars": { "type": "integer", "minimum": 0, "maximum": 2 } }),
                &["bars"],
            ),
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
            "Start recording the user's microphone onto an audio track. After the count-in (see set_count_in), the song plays from the current position so they can play along; looping pauses while recording. Only do this when the user asks to record, then call stop_recording when they say they're done.",
            object_schema(
                json!({ "track_id": { "type": "integer", "description": "An audio track (instrument \"audio\")." } }),
                &["track_id"],
            ),
            false,
        ),
        tool(
            "import_midi",
            "Read a Standard MIDI File (.mid) into the song: each instrument part becomes a new track with one clip (channel 10 becomes a drum track). An empty song also takes the file's tempo and time signature. One undo step.",
            object_schema(
                json!({ "path": { "type": "string", "description": "Absolute path of the .mid file." } }),
                &["path"],
            ),
            false,
        ),
        tool(
            "export_midi",
            "Write the song's instrument tracks as a type-1 MIDI file (drums on channel 10), for other music apps or notation programs.",
            object_schema(
                json!({ "path": { "type": "string", "description": "Absolute path ending in .mid." } }),
                &["path"],
            ),
            false,
        ),
        tool(
            "freeze_track",
            "Freeze a track: render its instrument and effects to audio and play that instead, so it costs almost no CPU (use when the user hears crackles or the CPU meter is high). Fader, pan, mute, sends and volume/pan automation still work. Any edit to the track's notes, sound, effects, or the tempo makes it play live again until frozen again; unfreeze_track goes back to live. One undo step.",
            object_schema(json!({ "track_id": { "type": "integer" } }), &["track_id"]),
            false,
        ),
        tool(
            "inspect_export",
            "Check a Godot export before writing it, with the same options as export_godot (project_dir not needed): renders exactly what would be exported and reports, in plain words, missing recordings, clipping or hot peaks, silence, a click or level jump at the loop point, loops that aren't whole beats or bars (Godot can't beat-sync them), and stems of different lengths. Returns findings (level problem/warning/ok), loudness, peak, and length. Run it before export_godot and fix problems first.",
            object_schema(godot_options_properties(), &[]),
            true,
        ),
        tool(
            "export_godot",
            "Export music straight into the user's Godot project as seamless loops. Writes OGG (or WAV) files plus Godot .import settings so they loop as soon as Godot imports them (with BPM/beat info for beat-synced transitions). Region: the loop region if looping is on, else the whole song; with intro=true the file starts at the song start and only the region loops. Options: stems (one file per track) with an AudioStreamSynchronized layers resource for adaptive mixing; sections (named regions, e.g. explore/combat) become an AudioStreamInteractive that switches on the next bar. Loudness is normalized to -16 LUFS unless told otherwise. Ask the user for their Godot project folder if you don't know it.",
            object_schema(godot_options_properties(), &["project_dir"]),
            false,
        ),
        tool(
            "import_musicxml",
            "Turn sheet music into tracks. Give either the path of a MusicXML file (.musicxml, .xml, or compressed .mxl, as saved by MuseScore and most notation apps) or MusicXML text in `xml`. To import a photo or PDF of printed music: read the notes from the image yourself, write them as a partwise MusicXML score (one <part> per instrument; include divisions, time signature, and <sound tempo>), and pass it as `xml`; check the result with get_clip. Each part becomes a new track; an empty song also takes the tempo and meter. One undo step.",
            object_schema(
                json!({
                    "path": { "type": "string", "description": "Absolute path of a .musicxml, .xml, or .mxl file." },
                    "xml": { "type": "string", "description": "A complete MusicXML partwise score, as text." }
                }),
                &[],
            ),
            false,
        ),
        tool(
            "export_musicxml",
            "Write the song as sheet music (MusicXML 4.0) for MuseScore or other notation apps. Notes are snapped to sixteenths. With `path`, writes a file; without, returns the MusicXML text.",
            object_schema(
                json!({
                    "path": { "type": "string", "description": "Absolute path ending in .musicxml (optional)." },
                    "track_ids": { "type": "array", "items": { "type": "integer" }, "description": "Only these tracks (default: every instrument track)." }
                }),
                &[],
            ),
            false,
        ),
        tool(
            "recording_delay",
            "Read or set how late the current microphone's recordings arrive, in ms (-500 to 500). Takes are moved this much earlier so they line up with the beat. Bluetooth headsets typically need 100-250 ms; prefer calibrate_recording over guessing. Returns the device, offset_ms, and whether it looks like Bluetooth.",
            object_schema(
                json!({ "ms": num("New delay in ms; omit to just read it.") }),
                &[],
            ),
            false,
        ),
        tool(
            "calibrate_recording",
            "Measure the microphone's recording delay. Tell the user first: put the headset on, and when the clicks start, listen to 4 clicks, then clap on each of the next 8. Takes about 10 seconds and silences the song meanwhile. Returns offset_ms, claps heard, spread_ms, and saved (true when steady enough and now in use). If not saved, ask the user to try again, clapping more clearly.",
            object_schema(json!({}), &[]),
            false,
        ),
        tool(
            READ_GUIDE,
            "Read the Nunc Pro Tune user guide. Pages: README (contents), lessons (a course to teach the user), basics, writing-music, audio, mixing, sheet-music, godot, claude, composing (Claude's playbook: workflow, game-music recipes, mix targets), troubleshooting. Works without the app running.",
            object_schema(
                json!({ "page": { "type": "string", "description": "Page name, e.g. \"composing\". Omit for the contents." } }),
                &[],
            ),
            true,
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
        "diagnostic_report" => Request::DiagnosticReport,
        "save_preset" => Request::SavePreset {
            track_id: get_id("track_id")?,
            name: get_str("name").ok_or("name is required")?,
        },
        "user_presets" => Request::UserPresets,
        "load_user_preset" => Request::LoadUserPreset {
            track_id: get_id("track_id")?,
            name: get_str("name").ok_or("name is required")?,
        },
        "set_count_in" => Request::SetCountIn {
            bars: args
                .get("bars")
                .and_then(Value::as_u64)
                .map(|b| b.min(2) as u32)
                .ok_or("bars must be 0, 1, or 2")?,
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
        "recording_delay" => Request::RecordingDelay { ms: get_f64("ms") },
        "calibrate_recording" => Request::CalibrateRecording,
        "import_midi" => Request::ImportMidi {
            path: get_str("path").ok_or("path is required")?,
        },
        "freeze_track" => Request::FreezeTrack {
            track_id: get_id("track_id")?,
        },
        "inspect_export" => Request::InspectExport(
            serde_json::from_value(Value::Object(args))
                .map_err(|e| format!("invalid arguments for inspect_export: {e}"))?,
        ),
        "export_godot" => Request::ExportGodot(
            serde_json::from_value(Value::Object(args))
                .map_err(|e| format!("invalid arguments for export_godot: {e}"))?,
        ),
        "import_musicxml" => Request::ImportMusicXml {
            path: get_str("path"),
            xml: get_str("xml"),
        },
        "export_musicxml" => Request::ExportMusicXml {
            path: get_str("path"),
            track_ids: args
                .get("track_ids")
                .map(|v| {
                    serde_json::from_value::<Vec<u32>>(v.clone())
                        .map_err(|_| "track_ids must be a list of track ids".to_owned())
                })
                .transpose()?,
        },
        "export_midi" => Request::ExportMidi {
            path: get_str("path").ok_or("path is required")?,
        },
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

/// The options of export_godot (and inspect_export).
fn godot_options_properties() -> Value {
    let num = |d: &str| json!({ "type": "number", "description": d });
    json!({
        "project_dir": { "type": "string", "description": "The Godot project folder (contains project.godot)." },
        "folder": { "type": "string", "description": "Folder inside the project (default \"music\")." },
        "name": { "type": "string", "description": "Base file name (default: the song name)." },
        "format": { "type": "string", "enum": ["ogg", "wav"], "description": "Default ogg." },
        "start_beats": num("Region start (default: loop region or song start)."),
        "end_beats": num("Region end."),
        "looped": { "type": "boolean", "description": "Seamless loop (default true). False: plays once with a ring-out." },
        "intro": { "type": "boolean", "description": "With looped: the file starts at the song start and Godot loops only the region (default: the loop region), so everything before it is an intro that plays once. Default false." },
        "stems": { "type": "boolean", "description": "Also export each track separately (default false)." },
        "bus_stems": { "type": "boolean", "description": "With stems: one stem per bus (the tracks playing into it) plus an \"other\" stem for tracks playing straight into the master, instead of one per track. Default false." },
        "layers": { "type": "boolean", "description": "With stems, write an AudioStreamSynchronized .tres (default true)." },
        "sections": { "type": "array", "description": "Named sections for an AudioStreamInteractive .tres.", "items": { "type": "object", "properties": { "name": { "type": "string" }, "start_beats": { "type": "number" }, "end_beats": { "type": "number" } }, "required": ["name", "start_beats", "end_beats"] } },
        "sections_from_markers": { "type": "boolean", "description": "Use the song's section markers (see add_marker and get_song's sections) as the sections. Ignored when sections is given." },
        "target_lufs": num("Loudness target (default -16)."),
        "normalize": { "type": "boolean", "description": "False keeps the mix level as is." }
    })
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
