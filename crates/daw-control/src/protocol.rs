use daw_model::{ClipId, Command, TrackId};
use serde::{Deserialize, Serialize};

/// Everything the bridge can ask the app to do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    /// Check the app is there.
    Ping,
    /// Apply a project edit (use `Command::Batch` for several at once).
    Execute {
        command: Command,
    },
    Undo,
    Redo,
    /// Compact overview of the song.
    GetSong,
    /// One track in full.
    GetTrack {
        track_id: TrackId,
    },
    /// One clip with every note.
    GetClip {
        clip_id: ClipId,
    },
    /// Instruments, presets, effects, and drum pads.
    Describe,
    Play {
        from_beats: Option<f64>,
    },
    Stop,
    Locate {
        beats: f64,
    },
    SetMetronome {
        on: bool,
    },
    /// Bars of clicks before recording starts (0-2).
    SetCountIn {
        bars: u32,
    },
    /// Plain-text report on devices, load, the song, and recent log lines.
    DiagnosticReport,
    /// Check a Godot export without writing it (project_dir is ignored).
    InspectExport(GodotOptions),
    /// Save a track's instrument settings as the user's own preset.
    SavePreset {
        track_id: TrackId,
        name: String,
    },
    /// The user's own presets.
    UserPresets,
    /// Give a track one of the user's own presets (one undo step).
    LoadUserPreset {
        track_id: TrackId,
        name: String,
    },
    /// Transport position and levels.
    Status,
    Save {
        path: Option<String>,
    },
    Open {
        path: String,
    },
    New,
    /// Render offline and measure the mix.
    Analyze {
        start_beats: Option<f64>,
        end_beats: Option<f64>,
        per_track: Option<bool>,
        spectrogram: Option<bool>,
    },
    /// Render to a 24-bit WAV file.
    ExportWav {
        path: String,
        start_beats: Option<f64>,
        end_beats: Option<f64>,
        tail_seconds: Option<f64>,
    },
    /// Bring an audio file (wav, mp3, m4a...) into the project as a clip.
    ImportAudio {
        path: String,
        /// Audio track to put it on; None makes a new track.
        track_id: Option<TrackId>,
        start_beats: Option<f64>,
    },
    /// Start recording the microphone onto an audio track (plays the song).
    RecordAudio {
        track_id: TrackId,
    },
    /// Stop recording; the take becomes a clip.
    StopRecording,
    /// Read (or set, with `ms`) how late the current microphone's
    /// recordings arrive.
    RecordingDelay {
        ms: Option<f64>,
    },
    /// Measure the recording delay: the user claps along with clicks.
    CalibrateRecording,
    /// Read a .mid file into new tracks.
    ImportMidi {
        path: String,
    },
    /// Write the instrument tracks as a .mid file.
    ExportMidi {
        path: String,
    },
    /// Read sheet music into new tracks, from a file or from MusicXML text.
    ImportMusicXml {
        path: Option<String>,
        xml: Option<String>,
    },
    /// Render seamless loops (and stems, adaptive-music resources) into a
    /// Godot project.
    ExportGodot(GodotOptions),
    /// The song (or some tracks) as MusicXML; written to `path` if given.
    ExportMusicXml {
        path: Option<String>,
        track_ids: Option<Vec<TrackId>>,
    },
}

/// Options for [`Request::ExportGodot`]; omitted fields use sensible defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GodotOptions {
    /// The Godot project folder (the one containing project.godot).
    #[serde(default)]
    pub project_dir: String,
    /// Folder inside the project (default "music").
    #[serde(default)]
    pub folder: Option<String>,
    /// Base file name (default: the song name).
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub format: Option<daw_export::AudioFormat>,
    #[serde(default)]
    pub start_beats: Option<f64>,
    #[serde(default)]
    pub end_beats: Option<f64>,
    /// Seamless loop (default true).
    #[serde(default)]
    pub looped: Option<bool>,
    /// With looped: the file starts at the song start and only the region
    /// (default: the loop region) loops; what comes before plays once as an
    /// intro (default false).
    #[serde(default)]
    pub intro: Option<bool>,
    #[serde(default)]
    pub stems: Option<bool>,
    /// With stems: one stem per bus instead of per track (default false).
    #[serde(default)]
    pub bus_stems: Option<bool>,
    /// With stems, an AudioStreamSynchronized of them (default true).
    #[serde(default)]
    pub layers: Option<bool>,
    #[serde(default)]
    pub sections: Option<Vec<daw_export::Section>>,
    /// Use the song's section markers as the sections (when `sections` is
    /// not given).
    #[serde(default)]
    pub sections_from_markers: Option<bool>,
    /// Loudness target in LUFS (default -16); `normalize: false` turns it off.
    #[serde(default)]
    pub target_lufs: Option<f64>,
    #[serde(default)]
    pub normalize: Option<bool>,
}

/// Reply to a [`Request`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok(result: serde_json::Value) -> Self {
        Self {
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(message.into()),
        }
    }

    pub fn into_result(self) -> Result<serde_json::Value, String> {
        if self.ok {
            Ok(self.result.unwrap_or(serde_json::Value::Null))
        } else {
            Err(self.error.unwrap_or_else(|| "unknown error".into()))
        }
    }
}

impl From<Result<serde_json::Value, String>> for Response {
    fn from(r: Result<serde_json::Value, String>) -> Self {
        match r {
            Ok(v) => Response::ok(v),
            Err(e) => Response::error(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_use_method_and_params() {
        let r = Request::Execute {
            command: Command::SetTempo { bpm: 100.0 },
        };
        let v = serde_json::to_value(&r).expect("json");
        assert_eq!(v["method"], "execute");
        assert_eq!(v["params"]["command"]["command"], "set_tempo");
        let back: Request = serde_json::from_value(v).expect("parse");
        assert_eq!(back, r);
        let ping: Request = serde_json::from_str(r#"{"method":"ping"}"#).expect("ping");
        assert_eq!(ping, Request::Ping);
    }
}
