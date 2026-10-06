use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::effect::Effect;
use crate::instrument::{Instrument, InstrumentKind};

/// Slowest tempo a project accepts, in beats per minute.
pub const MIN_TEMPO_BPM: f64 = 20.0;
/// Fastest tempo a project accepts, in beats per minute.
pub const MAX_TEMPO_BPM: f64 = 999.0;
/// Most tracks a project may have (keeps the engine's meters fixed-size).
pub const MAX_TRACKS: usize = 64;
/// Latest beat anything may start at (about 2 hours at 120 BPM).
pub const MAX_BEATS: f64 = 16_384.0;
/// Shortest note or clip, in beats (a 1/256 note).
pub const MIN_LENGTH_BEATS: f64 = 1.0 / 64.0;
/// Project file format version written by this build.
pub const FORMAT_VERSION: u32 = 1;
/// Longest audio file a project accepts, in seconds (one hour).
pub const MAX_AUDIO_SECONDS: f64 = 3600.0;

/// Identifier of a track, clip, note, or effect. Unique within a project.
pub type Id = u32;
pub type TrackId = Id;
pub type ClipId = Id;
pub type NoteId = Id;
pub type EffectId = Id;

/// A song: everything saved in a project file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Project {
    /// File format version; see [`FORMAT_VERSION`].
    #[serde(default = "default_format_version")]
    pub format_version: u32,
    /// Display name shown in the title bar and used as the default export name.
    pub name: String,
    /// Tempo in beats per minute.
    pub tempo_bpm: f64,
    /// Meter, such as 4/4 or 6/8.
    pub time_signature: TimeSignature,
    /// Instrument tracks, top to bottom.
    pub tracks: Vec<Track>,
    /// The master bus every track feeds into.
    #[serde(default)]
    pub master: MasterBus,
    /// Region that playback repeats when looping is on.
    #[serde(default)]
    pub loop_region: LoopRegion,
    /// Next free id. Ids are never reused within a project.
    #[serde(default)]
    pub next_id: Id,
}

fn default_format_version() -> u32 {
    FORMAT_VERSION
}

impl Project {
    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn track_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    pub fn track_index(&self, id: TrackId) -> Option<usize> {
        self.tracks.iter().position(|t| t.id == id)
    }

    /// The clip and the id of the track that holds it.
    pub fn clip(&self, id: ClipId) -> Option<(TrackId, &Clip)> {
        self.tracks
            .iter()
            .find_map(|t| t.clips.iter().find(|c| c.id == id).map(|c| (t.id, c)))
    }

    pub fn clip_mut(&mut self, id: ClipId) -> Option<&mut Clip> {
        self.tracks
            .iter_mut()
            .find_map(|t| t.clips.iter_mut().find(|c| c.id == id))
    }

    /// Hands out a fresh id.
    pub fn allocate_id(&mut self) -> Id {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    /// Makes sure `id` will never be handed out again.
    pub fn reserve_id(&mut self, id: Id) {
        self.next_id = self.next_id.max(id.saturating_add(1));
    }

    /// Whether any track, clip, note, or effect already uses `id`.
    pub fn id_in_use(&self, id: Id) -> bool {
        self.master.effects.iter().any(|e| e.id == id)
            || self.tracks.iter().any(|t| {
                t.id == id
                    || t.mixer.effects.iter().any(|e| e.id == id)
                    || t.clips
                        .iter()
                        .any(|c| c.id == id || c.notes.iter().any(|n| n.id == id))
            })
    }

    /// Every id in the project, for validation.
    pub(crate) fn all_ids(&self) -> Vec<Id> {
        let mut ids: Vec<Id> = self.master.effects.iter().map(|e| e.id).collect();
        for t in &self.tracks {
            ids.push(t.id);
            ids.extend(t.mixer.effects.iter().map(|e| e.id));
            for c in &t.clips {
                ids.push(c.id);
                ids.extend(c.notes.iter().map(|n| n.id));
            }
        }
        ids
    }

    /// Length of the song: the end of the last clip, in beats.
    pub fn end_beats(&self) -> f64 {
        self.tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .map(|c| c.start_beats + c.length_beats)
            .fold(0.0, f64::max)
    }

    /// Beats per bar.
    pub fn beats_per_bar(&self) -> f64 {
        f64::from(self.time_signature.numerator.max(1))
    }
}

impl Default for Project {
    /// A new song with the three instruments most game tracks start from.
    fn default() -> Self {
        let track = |id, name: &str, kind, preset| Track {
            id,
            name: name.to_owned(),
            instrument: Instrument::from_preset(kind, preset)
                .unwrap_or_else(|| unreachable!("factory preset {preset} exists")),
            mixer: Mixer::default(),
            clips: Vec::new(),
        };
        Self {
            format_version: FORMAT_VERSION,
            name: "Untitled".to_owned(),
            tempo_bpm: 120.0,
            time_signature: TimeSignature::default(),
            tracks: vec![
                track(1, "Keys", InstrumentKind::Synth, "Warm Keys"),
                track(2, "Bass", InstrumentKind::Synth, "Fat Bass"),
                track(3, "Drums", InstrumentKind::Drums, "Classic Kit"),
            ],
            master: MasterBus::default(),
            loop_region: LoopRegion::default(),
            next_id: 4,
        }
    }
}

/// One instrument lane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Track {
    pub id: TrackId,
    pub name: String,
    pub instrument: Instrument,
    /// Volume, pan, mute/solo, and effects.
    #[serde(default)]
    pub mixer: Mixer,
    /// MIDI clips on the timeline.
    #[serde(default)]
    pub clips: Vec<Clip>,
}

/// A track's channel strip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Mixer {
    /// Fader level in decibels (-60 to +6; -60 is silent).
    pub volume_db: f64,
    /// Stereo position: -1.0 left, 0.0 center, 1.0 right.
    pub pan: f64,
    pub mute: bool,
    /// When any track is soloed, only soloed tracks are heard.
    pub solo: bool,
    /// Effects, applied top to bottom.
    #[serde(default)]
    pub effects: Vec<Effect>,
}

impl Default for Mixer {
    fn default() -> Self {
        Self {
            volume_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            effects: Vec::new(),
        }
    }
}

pub const MIN_VOLUME_DB: f64 = -60.0;
pub const MAX_VOLUME_DB: f64 = 6.0;

/// The master bus: everything you hear passes through here last.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MasterBus {
    /// Master level in decibels (-60 to +6).
    pub volume_db: f64,
    #[serde(default)]
    pub effects: Vec<Effect>,
}

impl Default for MasterBus {
    fn default() -> Self {
        Self {
            volume_db: 0.0,
            effects: Vec::new(),
        }
    }
}

/// Region that playback repeats when `enabled`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoopRegion {
    pub enabled: bool,
    pub start_beats: f64,
    pub end_beats: f64,
}

impl Default for LoopRegion {
    fn default() -> Self {
        Self {
            enabled: false,
            start_beats: 0.0,
            end_beats: 16.0,
        }
    }
}

/// A block of notes (on instrument tracks) or audio (on audio tracks) on a
/// track's timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Clip {
    pub id: ClipId,
    pub name: String,
    /// Where the clip starts on the timeline, in beats from the song start.
    pub start_beats: f64,
    /// Clip length in beats. Notes past the end are not played; audio past
    /// the end of its file is silent.
    pub length_beats: f64,
    /// Notes, with times relative to the clip start. Empty for audio clips.
    #[serde(default)]
    pub notes: Vec<Note>,
    /// The audio this clip plays, on audio tracks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioRegion>,
}

impl Clip {
    pub fn is_audio(&self) -> bool {
        self.audio.is_some()
    }
}

/// Loudest clip gain, in decibels.
pub const MAX_CLIP_GAIN_DB: f64 = 24.0;

/// The part of an audio file an audio clip plays, and how.
///
/// Audio keeps its own speed: changing the tempo moves where clips start
/// (they stay on their beat) but does not stretch the audio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AudioRegion {
    /// File name inside the project's audio folder ("<Song> Audio").
    pub file: String,
    /// Length of the whole file, in seconds.
    pub file_seconds: f64,
    /// Where in the file the clip starts playing, in seconds.
    #[serde(default)]
    pub offset_seconds: f64,
    /// Clip volume in decibels (-60 to +24; 0 = as recorded).
    #[serde(default)]
    pub gain_db: f64,
    /// Fade-in length at the clip start, in seconds.
    #[serde(default)]
    pub fade_in_seconds: f64,
    /// Fade-out length at the clip end, in seconds.
    #[serde(default)]
    pub fade_out_seconds: f64,
}

impl AudioRegion {
    /// Seconds of audio left after `offset_seconds`.
    pub fn remaining_seconds(&self) -> f64 {
        (self.file_seconds - self.offset_seconds).max(0.0)
    }
}

/// One MIDI note inside a clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Note {
    pub id: NoteId,
    /// MIDI note number: 60 is middle C (C4). Drums use 36 kick, 38 snare,
    /// 42 closed hat, 46 open hat, 49 crash (General MIDI).
    pub pitch: u8,
    /// Start, in beats from the clip start.
    pub start_beats: f64,
    /// Length in beats.
    pub length_beats: f64,
    /// Loudness, 1–127.
    pub velocity: u8,
}

/// Musical meter: `numerator` beats per bar, each a 1/`denominator` note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TimeSignature {
    /// Beats per bar (1–32).
    pub numerator: u8,
    /// Note value of one beat: 1, 2, 4, 8, 16, or 32.
    pub denominator: u8,
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self {
            numerator: 4,
            denominator: 4,
        }
    }
}
