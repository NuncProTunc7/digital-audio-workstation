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
    /// Read a .mid file into new tracks.
    ImportMidi {
        path: String,
    },
    /// Write the instrument tracks as a .mid file.
    ExportMidi {
        path: String,
    },
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
