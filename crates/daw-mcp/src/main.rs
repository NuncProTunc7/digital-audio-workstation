//! `npt-mcp`: the MCP server Claude Desktop and Claude Code launch.
//!
//! It speaks MCP over stdin/stdout and forwards each tool call to the
//! running Nunc Pro Tune app over its local control channel. It holds no
//! state of its own, so the app can be restarted at any time.

mod guide;
mod tools;

use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::Value;

use daw_control::ControlClient;

const INSTRUCTIONS: &str = "\
You are controlling Nunc Pro Tune, a music app (DAW) the user is running. Changes \
appear in the app immediately and every edit is undoable.

How music is represented:
- Time is in beats from the song start (bar 1 beat 1 = 0). In 4/4, one bar = 4 beats; \
0.5 = an eighth note, 0.25 = a sixteenth.
- Notes: MIDI pitch (60 = middle C, 69 = A 440), start_beats relative to the clip, \
length_beats, velocity 1-127.
- Drum tracks use General MIDI notes: 36 kick, 37 rim, 38 snare, 39 clap, 42 closed hat, \
44 pedal hat, 46 open hat, 41/43/45/47/48/50 toms (low to high), 49 crash, 51 ride.
- Ids: tracks, clips, notes, and effects have numeric ids; read them with get_song, \
get_track, and get_clip.
- Audio tracks (instrument \"audio\") hold recorded or imported audio clips instead of \
notes. Bring in files with import_audio; shape clips with set_audio_clip (gain, fades), \
split_clip, trim_clip_start, move_clip, and resize_clip. Audio keeps its own speed when \
the tempo changes.

Working well:
- Before composing, read_guide(\"composing\") for the workflow, game-music recipes, and mix \
targets. When the user wants to learn, read_guide(\"lessons\") and teach one step at a time; \
the other pages explain every button, so use them to answer how-to questions accurately.
- Start with get_song. Use describe_instruments before changing sounds (parameter ids, \
ranges, presets).
- Use batch to make a multi-step change one undo step (e.g. add a track, load a preset, \
create a clip with notes).
- You cannot hear audio: after changes, call analyze_mix (and spectrogram=true when \
judging tone) to check loudness, balance, and peaks. Game music usually sits around \
-16 to -20 LUFS with peaks below -1 dBTP.
- Use play/stop so the user can listen, and ask them how it sounds.
- Sheet music: export_musicxml / import_musicxml (MuseScore and other notation apps); \
to turn a photo or PDF of a score into tracks, read it yourself, write MusicXML, and pass \
it to import_musicxml as text. MIDI files: import_midi / export_midi.
- Automation: add_automation_lane moves volume, pan, or any instrument/effect setting \
over time (fade-ins, filter sweeps, swelling reverb); points are (beats, value).
- Sampler tracks (instrument \"sampler\") play an SFZ sample pack: real pianos, \
strings, brass, woodwinds. sample_library lists free ones the app can download \
(download_sample_pack); load one with load_sample_pack and check sample_pack_status \
in get_track.
- Plugins: the user's own VST3 instruments (plugins, load_plugin). Their controls are \
0-1 values by id (plugin_params, set_plugin_params); always check the display text. Audio clips can follow tempo changes with set_clip_tempo.
- Games: export_godot writes seamless loops (plus optional stems and adaptive-music \
resources) straight into the user's Godot project, already set to loop.
- Recording timing: if the user's takes land late (common with Bluetooth headsets), run \
calibrate_recording (explain the clapping first) or set recording_delay.
- Don't open or start a new project without asking if the user may have unsaved work.";

#[derive(Clone)]
struct Bridge {
    tools: Arc<Vec<tools::ToolDef>>,
}

impl Bridge {
    fn new() -> Self {
        Self {
            tools: Arc::new(tools::all_tools()),
        }
    }

    /// Runs one tool call against the app. Never fails at the MCP level:
    /// problems come back as readable tool errors Claude can act on.
    fn call_blocking(name: &str, args: serde_json::Map<String, Value>) -> CallToolResult {
        // The guide is built in, so it works even when the app isn't running.
        if name == tools::READ_GUIDE {
            let page = args.get("page").and_then(Value::as_str);
            return match guide::read(page) {
                Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
                Err(e) => CallToolResult::error(vec![ContentBlock::text(e)]),
            };
        }
        let request = match tools::to_request(name, args) {
            Ok(r) => r,
            Err(e) => return CallToolResult::error(vec![ContentBlock::text(e)]),
        };
        // Discover on every call so restarting the app just works.
        let client = match ControlClient::discover() {
            Ok(c) => c,
            Err(e) => return CallToolResult::error(vec![ContentBlock::text(e.to_string())]),
        };
        match client.call(&request) {
            Err(e) => CallToolResult::error(vec![ContentBlock::text(e.to_string())]),
            Ok(Err(app_error)) => CallToolResult::error(vec![ContentBlock::text(app_error)]),
            Ok(Ok(mut value)) => {
                let mut content = Vec::new();
                // Spectrograms go back as images Claude can look at.
                if let Some(png) = value
                    .as_object_mut()
                    .and_then(|o| o.remove("spectrogram_png_base64"))
                    .and_then(|v| v.as_str().map(str::to_owned))
                {
                    content.push(ContentBlock::image(png, "image/png"));
                }
                let text = serde_json::to_string_pretty(&value).unwrap_or_default();
                content.insert(0, ContentBlock::text(text));
                CallToolResult::success(content)
            }
        }
    }
}

impl ServerHandler for Bridge {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "nunc-pro-tune",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let tools = self
            .tools
            .iter()
            .map(|t| {
                Tool::new(
                    t.name.clone(),
                    t.description.clone(),
                    t.input_schema.clone(),
                )
                .with_annotations(ToolAnnotations::new().read_only(t.read_only))
            })
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.to_string();
        let args = request.arguments.unwrap_or_default();
        let result = tokio::task::spawn_blocking(move || Bridge::call_blocking(&name, args))
            .await
            .unwrap_or_else(|e| CallToolResult::error(vec![ContentBlock::text(e.to_string())]));
        Ok(result.into())
    }
}

#[tokio::main]
async fn main() {
    if std::env::args().any(|a| a == "--version") {
        println!("npt-mcp {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let service = match Bridge::new().serve(rmcp::transport::stdio()).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("npt-mcp: could not start: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = service.waiting().await {
        eprintln!("npt-mcp: stopped: {e}");
    }
}
