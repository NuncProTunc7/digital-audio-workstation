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
    /// Saved versions of the song ("Before Claude's changes", "Darker
    /// mix") to go back to or compare with.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub snapshots: Vec<Snapshot>,
    /// Section markers, in time order. A section runs from its marker to
    /// the next one (or the end of the song).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
    /// Group buses (drums, music, ambience, a shared reverb), played into
    /// the master after the tracks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buses: Vec<Bus>,
    /// The song's key, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<crate::music::Key>,
    /// The chord track, in time order: each chord lasts until the next.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chords: Vec<crate::music::Chord>,
}

/// A group bus: tracks play into it (or send some of their sound to it),
/// it applies its own effects and level, and plays into the master.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Bus {
    pub id: Id,
    pub name: String,
    /// Level, pan, mute, and effects (solo is not used on buses).
    #[serde(default)]
    pub mixer: Mixer,
}

/// A track's sound rendered to audio (instrument and effects, before the
/// fader and pan), from the song start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Frozen {
    /// The audio file in the project's audio folder.
    pub file: String,
    /// Fingerprint of everything that shaped the sound; when it no longer
    /// matches (an edit), the track plays live again.
    pub fingerprint: u64,
}

/// Some of a track's sound sent to a bus as well as its own output, e.g.
/// to a shared reverb.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Send {
    pub bus_id: Id,
    /// How much is sent, in dB (-60 to +6).
    pub level_db: f64,
    /// Taken before the track's fader (the send ignores the fader and pan).
    #[serde(default)]
    pub pre_fader: bool,
}

impl Send {
    /// Level of a new send.
    pub const DEFAULT_LEVEL_DB: f64 = -6.0;
}

/// A named point on the timeline where a section starts ("Explore",
/// "Combat").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Marker {
    pub id: Id,
    pub name: String,
    /// Where the section starts, in beats from the song start.
    pub start_beats: f64,
}

/// A part of the song between two markers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SongSection {
    pub name: String,
    pub start_beats: f64,
    pub end_beats: f64,
}

/// A named, saved version of the song.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Snapshot {
    pub id: Id,
    pub name: String,
    pub song: SongState,
}

/// Everything about the music that a saved version keeps: the song without
/// its name and its other versions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SongState {
    pub tempo_bpm: f64,
    pub time_signature: TimeSignature,
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub master: MasterBus,
    #[serde(default)]
    pub loop_region: LoopRegion,
    #[serde(default)]
    pub markers: Vec<Marker>,
    #[serde(default)]
    pub buses: Vec<Bus>,
    #[serde(default)]
    pub key: Option<crate::music::Key>,
    #[serde(default)]
    pub chords: Vec<crate::music::Chord>,
}

fn default_format_version() -> u32 {
    FORMAT_VERSION
}

impl Project {
    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    /// The music as it is now, for saving as a version.
    pub fn song_state(&self) -> SongState {
        SongState {
            tempo_bpm: self.tempo_bpm,
            time_signature: self.time_signature,
            tracks: self.tracks.clone(),
            master: self.master.clone(),
            loop_region: self.loop_region,
            markers: self.markers.clone(),
            buses: self.buses.clone(),
            key: self.key,
            chords: self.chords.clone(),
        }
    }

    /// Fingerprint of everything that shapes `track`'s sound before its
    /// fader: instrument, notes and audio, effects, automation of those,
    /// tempo and meter. Fader, pan, mute, routing and volume/pan automation
    /// aren't part of it; they still work live on a frozen track.
    pub fn freeze_fingerprint(&self, track: &Track) -> u64 {
        let shaping: Vec<&AutomationLane> = track
            .automation
            .iter()
            .filter(|l| !matches!(l.target, AutomationTarget::Volume | AutomationTarget::Pan))
            .collect();
        let text = serde_json::json!({
            "instrument": track.instrument,
            "clips": track.clips,
            "effects": track.mixer.effects,
            "automation": shaping,
            "tempo": self.tempo_bpm,
            "meter": self.time_signature,
        })
        .to_string();
        // FNV-1a: stable across runs and Rust versions (it's saved).
        text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    /// Whether `track` should play its frozen rendering (it has one and
    /// nothing has changed since).
    pub fn frozen_is_current(&self, track: &Track) -> bool {
        track
            .frozen
            .as_ref()
            .is_some_and(|f| f.fingerprint == self.freeze_fingerprint(track))
    }

    /// The chord playing at `beats`, if any.
    pub fn chord_at(&self, beats: f64) -> Option<&crate::music::Chord> {
        self.chords
            .iter()
            .rev()
            .find(|c| c.start_beats <= beats + 1e-9)
    }

    /// The sections the markers divide the song into: each runs from its
    /// marker to the next, the last to the end of the song (rounded up to
    /// whole bars). Sections with nothing in them are left out.
    pub fn sections(&self) -> Vec<SongSection> {
        let bpb = self.beats_per_bar();
        let song_end = (self.end_beats() / bpb).ceil() * bpb;
        self.markers
            .iter()
            .enumerate()
            .map(|(i, m)| SongSection {
                name: m.name.clone(),
                start_beats: m.start_beats,
                end_beats: self
                    .markers
                    .get(i + 1)
                    .map_or(song_end, |next| next.start_beats),
            })
            .filter(|s| s.end_beats > s.start_beats)
            .collect()
    }

    /// Replaces the music with `song` (keeping the name and the saved
    /// versions); returns what was there.
    pub fn set_song_state(&mut self, song: SongState) -> SongState {
        let old = self.song_state();
        self.tempo_bpm = song.tempo_bpm;
        self.time_signature = song.time_signature;
        self.tracks = song.tracks;
        self.master = song.master;
        self.loop_region = song.loop_region;
        self.markers = song.markers;
        self.buses = song.buses;
        self.key = song.key;
        self.chords = song.chords;
        if let Some(max) = self.all_ids().into_iter().max() {
            self.reserve_id(max);
        }
        old
    }

    /// The song as saved version `snapshot_id` has it, ready to play or
    /// render.
    pub fn snapshot_song(&self, snapshot_id: Id) -> Option<Project> {
        let snapshot = self.snapshots.iter().find(|s| s.id == snapshot_id)?;
        let mut p = self.clone();
        p.snapshots.clear();
        p.set_song_state(snapshot.song.clone());
        Some(p)
    }

    /// Every audio file the song or any of its saved versions plays,
    /// including frozen tracks' renderings.
    pub fn audio_files(&self) -> Vec<String> {
        let tracks = || {
            self.tracks
                .iter()
                .chain(self.snapshots.iter().flat_map(|s| &s.song.tracks))
        };
        let mut files: Vec<String> = tracks()
            .flat_map(|t| &t.clips)
            .filter_map(|c| c.audio.as_ref().map(|a| a.file.clone()))
            .chain(tracks().filter_map(|t| t.frozen.as_ref().map(|f| f.file.clone())))
            .collect();
        files.sort();
        files.dedup();
        files
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
        self.chords.iter().any(|c| c.id == id)
            || self.snapshots.iter().any(|s| s.id == id)
            || self.markers.iter().any(|m| m.id == id)
            || self
                .buses
                .iter()
                .any(|b| b.id == id || b.mixer.effects.iter().any(|e| e.id == id))
            || self.master.effects.iter().any(|e| e.id == id)
            || self.tracks.iter().any(|t| {
                t.id == id
                    || t.mixer.effects.iter().any(|e| e.id == id)
                    || t.automation.iter().any(|l| l.id == id)
                    || t.clips
                        .iter()
                        .any(|c| c.id == id || c.notes.iter().any(|n| n.id == id))
            })
    }

    /// Every id in the project, for validation.
    pub(crate) fn all_ids(&self) -> Vec<Id> {
        let mut ids: Vec<Id> = self.master.effects.iter().map(|e| e.id).collect();
        ids.extend(self.snapshots.iter().map(|s| s.id));
        ids.extend(self.markers.iter().map(|m| m.id));
        ids.extend(self.chords.iter().map(|c| c.id));
        for b in &self.buses {
            ids.push(b.id);
            ids.extend(b.mixer.effects.iter().map(|e| e.id));
        }
        for t in &self.tracks {
            ids.push(t.id);
            ids.extend(t.mixer.effects.iter().map(|e| e.id));
            ids.extend(t.automation.iter().map(|l| l.id));
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
            frozen: None,
            output: None,
            sends: Vec::new(),
            id,
            name: name.to_owned(),
            instrument: Instrument::from_preset(kind, preset)
                .unwrap_or_else(|| unreachable!("factory preset {preset} exists")),
            mixer: Mixer::default(),
            clips: Vec::new(),
            automation: Vec::new(),
        };
        Self {
            chords: Vec::new(),
            key: None,
            buses: Vec::new(),
            markers: Vec::new(),
            snapshots: Vec::new(),
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
    /// Settings that change over time (volume rides, filter sweeps, ...).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub automation: Vec<AutomationLane>,
    /// A rendering of the track's sound, played instead of its instrument
    /// and effects to save CPU, while it is up to date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frozen: Option<Frozen>,
    /// The bus this track plays into (None = the master).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Id>,
    /// Extra feeds into buses (e.g. a shared reverb).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sends: Vec<Send>,
}

/// Id of an automation lane.
pub type LaneId = Id;

/// What an automation lane moves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationTarget {
    /// Track volume in dB (-60 to +6).
    Volume,
    /// Track pan, -1 (left) to 1 (right).
    Pan,
    /// An instrument parameter by id (see describe_instruments), in its own units.
    InstrumentParam { param: String },
    /// A parameter of an effect on this track, in its own units.
    EffectParam { effect_id: EffectId, param: String },
}

/// One point on an automation curve. Values change in straight lines
/// between points; before the first point and after the last, the nearest
/// point's value holds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AutomationPoint {
    /// Position in beats from the song start.
    pub beats: f64,
    /// Value in the target's units (dB for volume, Hz for a cutoff, ...).
    pub value: f64,
}

/// A curve that moves one setting over time. While a lane is enabled and
/// has points, it overrides that setting's slider during playback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AutomationLane {
    pub id: LaneId,
    pub target: AutomationTarget,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    /// Points sorted by `beats`.
    #[serde(default)]
    pub points: Vec<AutomationPoint>,
}

fn enabled_default() -> bool {
    true
}

impl AutomationLane {
    /// The curve's value at `beats` (None without points).
    pub fn value_at(&self, beats: f64) -> Option<f64> {
        value_at(&self.points, beats)
    }
}

/// Linear interpolation over sorted points, holding the ends.
pub fn value_at(points: &[AutomationPoint], beats: f64) -> Option<f64> {
    let first = points.first()?;
    let i = points.partition_point(|p| p.beats <= beats);
    if i == 0 {
        return Some(first.value);
    }
    let a = points[i - 1];
    let Some(b) = points.get(i) else {
        return Some(a.value);
    };
    let span = b.beats - a.beats;
    if span <= 0.0 {
        return Some(b.value);
    }
    Some(a.value + (b.value - a.value) * (beats - a.beats) / span)
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
    /// Shuffle applied to the clip's notes when they play (None = straight).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swing: Option<Swing>,
    /// Kept but silent (an unused take, or a part switched off).
    #[serde(default, skip_serializing_if = "is_false")]
    pub muted: bool,
    /// Linked clips share this group id and keep the same notes: editing
    /// one edits them all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<Id>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Clip {
    pub fn is_audio(&self) -> bool {
        self.audio.is_some()
    }

    /// Where on the song timeline a note time (beats from the clip start)
    /// is heard, after swing.
    pub fn played_song_beats(&self, beats: f64) -> f64 {
        let song = self.start_beats + beats;
        self.swing.map_or(song, |s| s.warp(song))
    }
}

/// Shuffle: every second step of `grid_beats` is played late, giving a
/// swung, bouncy feel. Steps are counted on the song's beat grid, so a
/// clip swings the same wherever it is split or moved to. Notes keep their
/// written positions; only playback (and rendering, analysis, MIDI export)
/// hears the swing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Swing {
    /// 0 = straight, about 67 = triplet shuffle, 100 = hardest (the late
    /// step lands halfway into the next one).
    pub amount_percent: f64,
    /// The step that swings, in beats: 0.25 (sixteenths, usual for drums)
    /// or 0.5 (eighths).
    pub grid_beats: f64,
}

impl Swing {
    /// Maps a straight time to its swung time. Each pair of steps is
    /// stretched so the second step starts late; times between steps
    /// move proportionally, so note order never changes.
    pub fn warp(&self, beats: f64) -> f64 {
        let g = self.grid_beats;
        if g.is_nan() || g <= 0.0 || self.amount_percent <= 0.0 || !beats.is_finite() {
            return beats;
        }
        let pair = 2.0 * g;
        let off = g + self.amount_percent.clamp(0.0, 100.0) / 100.0 * g / 2.0;
        let k = (beats / pair).floor();
        let r = beats - k * pair;
        let swung = if r <= g {
            r * off / g
        } else {
            off + (r - g) * (pair - off) / g
        };
        k * pair + swung
    }
}

/// Loudest clip gain, in decibels.
pub const MAX_CLIP_GAIN_DB: f64 = 24.0;

/// The part of an audio file an audio clip plays, and how.
///
/// By default audio keeps its own speed: changing the tempo moves where
/// clips start (they stay on their beat) but does not stretch the audio.
/// With `source_bpm` set, the clip stretches to follow the tempo.
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
    /// When set, the clip follows the song's tempo: it was recorded at this
    /// tempo and is stretched (pitch unchanged) to match the current one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_bpm: Option<f64>,
}

impl AudioRegion {
    /// Seconds of audio left after `offset_seconds`.
    pub fn remaining_seconds(&self) -> f64 {
        (self.file_seconds - self.offset_seconds).max(0.0)
    }

    /// Seconds of the file that one beat on the timeline covers.
    pub fn file_seconds_per_beat(&self, tempo_bpm: f64) -> f64 {
        60.0 / self.source_bpm.unwrap_or(tempo_bpm)
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
    /// How often it plays, 1–100 % (100 = every time). Below 100 the note
    /// is left out on some passes, the same way every time the song plays
    /// or renders, so repeats vary without surprises.
    #[serde(default = "full_chance", skip_serializing_if = "is_full_chance")]
    pub chance: u8,
}

/// Notes play every time unless given a lower chance.
pub fn full_chance() -> u8 {
    100
}

fn is_full_chance(chance: &u8) -> bool {
    *chance >= 100
}

#[cfg(test)]
mod swing_tests {
    use super::Swing;

    #[test]
    fn swing_delays_every_second_step() {
        let s = Swing {
            amount_percent: 100.0,
            grid_beats: 0.25,
        };
        // Downbeats stay; the off sixteenth moves half a step late.
        assert_eq!(s.warp(0.0), 0.0);
        assert_eq!(s.warp(0.25), 0.375);
        assert_eq!(s.warp(0.5), 0.5);
        assert_eq!(s.warp(1.25), 1.375);
        // Times between steps keep their order.
        assert!(s.warp(0.3) > s.warp(0.25) && s.warp(0.3) < s.warp(0.5));
        // Triplet feel at about 67 %.
        let triplet = Swing {
            amount_percent: 200.0 / 3.0,
            grid_beats: 0.5,
        };
        assert!((triplet.warp(0.5) - 2.0 / 3.0).abs() < 1e-12);
        // No swing changes nothing.
        let straight = Swing {
            amount_percent: 0.0,
            grid_beats: 0.25,
        };
        assert_eq!(straight.warp(0.25), 0.25);
    }
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
