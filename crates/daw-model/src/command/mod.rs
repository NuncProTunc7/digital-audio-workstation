//! Every edit to a project is a [`Command`]. Applying one returns the
//! Command that undoes it, which is how undo/redo works.

mod audio;
mod automation;
mod buses;
mod chords;
mod clips;
mod effects;
mod instruments;
mod links;
pub use buses::MAX_BUSES;
mod markers;
mod notes;
mod snapshots;
pub use snapshots::MAX_SNAPSHOTS;
mod song;
mod tracks;

#[cfg(test)]
mod tests;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::effect::{Effect, EffectKind};
use crate::instrument::{Instrument, InstrumentKind};
use crate::project::{
    AudioRegion, AutomationLane, AutomationPoint, AutomationTarget, Clip, ClipId, EffectId, Id,
    LaneId, MAX_TEMPO_BPM, MIN_TEMPO_BPM, NoteId, Project, Track, TrackId,
};

pub use notes::{NoteEdit, NoteInput};

/// Instrument validation shared with file loading.
pub(crate) fn complete_instrument_pub(i: Instrument) -> Result<Instrument, CommandError> {
    instruments::complete_instrument(i)
}

/// Clip/track kind validation shared with file loading.
pub(crate) fn check_clip_fits_pub(kind: InstrumentKind, clip: &Clip) -> Result<(), CommandError> {
    audio::check_clip_fits(kind, clip)
}

/// Note validation shared with file loading.
pub(crate) fn validate_note_pub(n: &crate::project::Note) -> Result<(), CommandError> {
    notes::validate_input(&NoteInput {
        chance: n.chance,
        pitch: n.pitch,
        start_beats: n.start_beats,
        length_beats: n.length_beats,
        velocity: n.velocity,
        id: None,
    })
}

/// An edit to a project.
///
/// Doc comments on each variant become the tool descriptions Claude reads
/// over MCP, so write them for someone who has never seen the code. Times are
/// in beats (1 beat = one quarter note in 4/4); bars = beats / beats-per-bar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    // ---- Song ----
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
    /// Set the loop region and/or turn looping on or off. Omitted fields keep
    /// their current value. Useful for seamless game-music loops.
    SetLoop {
        enabled: Option<bool>,
        /// Loop start in beats.
        start_beats: Option<f64>,
        /// Loop end in beats; must be after the start.
        end_beats: Option<f64>,
    },

    // ---- Tracks ----
    /// Add a new instrument track.
    AddTrack {
        /// Track name, such as "Lead" or "Strings".
        name: String,
        /// `synth` (keys, pads, leads, basses) or `drums`.
        instrument: InstrumentKind,
        /// Optional factory preset; defaults to the first preset.
        preset: Option<String>,
        /// Position from the top (0 = first). Omit to add at the bottom.
        index: Option<usize>,
    },
    /// Delete a track and everything on it.
    RemoveTrack { track_id: TrackId },
    /// Put back a previously removed track exactly as it was (used by undo).
    RestoreTrack {
        /// Position from the top.
        index: usize,
        track: Track,
    },
    /// Rename a track.
    RenameTrack { track_id: TrackId, name: String },
    /// Move a track to a new position (0 = top).
    MoveTrack { track_id: TrackId, index: usize },
    /// Change a track's channel strip. Omitted fields keep their value.
    SetTrackMixer {
        track_id: TrackId,
        /// Fader level in dB, -60 (silent) to +6.
        volume_db: Option<f64>,
        /// Stereo position, -1.0 (left) to 1.0 (right).
        pan: Option<f64>,
        mute: Option<bool>,
        /// When any track is soloed, only soloed tracks are heard.
        solo: Option<bool>,
    },
    /// Set the master volume in dB (-60 to +6).
    SetMasterVolume { volume_db: f64 },
    // ---- Chord track ----
    /// Put a chord on the chord track at `start_beats`; it lasts until the
    /// next chord. root and bass are pitch classes (0 = C ... 11 = B);
    /// bass makes a slash chord (C/E). The chord track makes no sound: it
    /// guides writing and is shown above the piano roll. One per beat.
    AddChord {
        start_beats: f64,
        root: u8,
        quality: crate::music::ChordQuality,
        bass: Option<u8>,
    },
    /// Change a chord (all three fields are set).
    SetChord {
        chord_id: Id,
        root: u8,
        quality: crate::music::ChordQuality,
        bass: Option<u8>,
    },
    /// Move a chord to another beat.
    MoveChord { chord_id: Id, start_beats: f64 },
    /// Delete a chord; the one before lasts longer.
    RemoveChord { chord_id: Id },
    /// Put back a deleted chord exactly as it was (used by undo).
    RestoreChord { chord: crate::music::Chord },

    /// Set the song's key (tonic 0 = C ... 11 = B, and a mode such as
    /// minor or dorian), or null for none. It drives scale highlighting,
    /// key signatures in MIDI and MusicXML, and get_song's scale notes.
    /// It doesn't move any notes.
    SetKey { key: Option<crate::music::Key> },

    // ---- Buses ----
    /// Add a group bus (e.g. "Drums", "Reverb") at the end of the mixer.
    /// Tracks play into it with set_track_output or feed it with set_send;
    /// add effects to it with add_effect using the bus id as track_id.
    /// At most 16.
    AddBus { name: String },
    /// Delete a bus; tracks that played into it play into the master, and
    /// sends to it are removed.
    RemoveBus { bus_id: Id },
    /// Put back a deleted bus with its routing (used by undo).
    RestoreBus {
        bus: crate::project::Bus,
        index: usize,
        /// Tracks that played into it.
        outputs: Vec<TrackId>,
        /// Sends that fed it.
        sends: Vec<(TrackId, crate::project::Send)>,
    },
    /// Rename a bus.
    RenameBus { bus_id: Id, name: String },
    /// Set a bus's level (dB, -60 to +6), pan (-1 to 1), or mute. Omitted
    /// fields keep their value.
    SetBusMixer {
        bus_id: Id,
        volume_db: Option<f64>,
        pan: Option<f64>,
        mute: Option<bool>,
    },
    /// Use a rendering of the track instead of its instrument and effects
    /// (to save CPU). Made by the freeze_track tool, which renders the
    /// file first; call that rather than this.
    FreezeTrack {
        track_id: TrackId,
        frozen: crate::project::Frozen,
    },
    /// Play the track live again (its instrument and effects).
    UnfreezeTrack { track_id: TrackId },
    /// Choose where a track plays: a bus id, or null for the master.
    SetTrackOutput {
        track_id: TrackId,
        bus_id: Option<Id>,
    },
    /// Send some of a track's sound to a bus as well (e.g. a shared
    /// reverb), or change an existing send. New sends start at -12 dB,
    /// after the fader. `pre_fader: true` ignores the track's fader.
    SetSend {
        track_id: TrackId,
        bus_id: Id,
        /// Send level in dB, -60 to +6.
        level_db: Option<f64>,
        pre_fader: Option<bool>,
    },
    /// Stop sending a track to a bus.
    RemoveSend { track_id: TrackId, bus_id: Id },

    // ---- Instruments ----
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

    // ---- Effects ----
    /// Add an installed plugin effect (from plugins) to a chain: a track's,
    /// a bus's (its id as track_id), or the master's (track_id omitted).
    /// Use the add_plugin_effect tool, which fills in the plugin.
    AddPluginEffect {
        track_id: Option<TrackId>,
        plugin: Box<crate::plugin::PluginRef>,
        /// Position in the chain (default: last).
        index: Option<usize>,
    },
    /// Add an effect to a track's chain, or to the master bus if `track_id`
    /// is omitted. Use `describe_effects` for kinds and parameters.
    AddEffect {
        track_id: Option<TrackId>,
        kind: EffectKind,
        /// Position in the chain (0 = first). Omit to add at the end.
        index: Option<usize>,
    },
    /// Remove an effect from a track (or the master if `track_id` is omitted).
    RemoveEffect {
        track_id: Option<TrackId>,
        effect_id: EffectId,
    },
    /// Put back a previously removed effect (used by undo).
    RestoreEffect {
        track_id: Option<TrackId>,
        index: usize,
        effect: Effect,
    },
    /// Change one effect parameter. Omit `track_id` for master effects.
    SetEffectParam {
        track_id: Option<TrackId>,
        effect_id: EffectId,
        /// Parameter id, for example `mix` or `threshold_db`.
        param: String,
        value: f64,
    },
    /// Turn an effect on or bypass it. Omit `track_id` for master effects.
    SetEffectEnabled {
        track_id: Option<TrackId>,
        effect_id: EffectId,
        enabled: bool,
    },
    /// Sidechain a compressor: it reacts to another track's sound instead of
    /// its own, so this track ducks under it (classic: a compressor on the
    /// bass or pad keyed by the kick track, threshold around -30 dB, ratio
    /// 4-8, release 0.1-0.2 s). `source: null` turns the sidechain off.
    /// `track_id` is the track or bus holding the compressor (omit for the
    /// master). Only compressors can be sidechained.
    SetEffectSidechain {
        track_id: Option<TrackId>,
        effect_id: EffectId,
        /// The track to listen to.
        source: Option<TrackId>,
    },

    // ---- Clips ----
    /// Create a MIDI clip on a track, optionally filled with notes.
    CreateClip {
        track_id: TrackId,
        /// Start on the timeline, in beats from the song start (bar 1 = 0).
        start_beats: f64,
        /// Length in beats, for example 16 for four bars of 4/4.
        length_beats: f64,
        /// Optional name; defaults to the track name.
        name: Option<String>,
        /// Notes to put in the clip, with times relative to the clip start.
        #[serde(default)]
        notes: Vec<NoteInput>,
    },
    /// Delete a clip.
    DeleteClip { clip_id: ClipId },
    /// Put back a previously deleted clip exactly as it was (used by undo).
    RestoreClip { track_id: TrackId, clip: Clip },
    /// Move a clip in time and/or to another track. Omitted fields keep
    /// their value.
    MoveClip {
        clip_id: ClipId,
        start_beats: Option<f64>,
        track_id: Option<TrackId>,
    },
    /// Change a clip's length in beats. Notes past the new end stay in the
    /// clip but are not played.
    ResizeClip { clip_id: ClipId, length_beats: f64 },
    /// Rename a clip.
    RenameClip { clip_id: ClipId, name: String },

    // ---- Section markers ----
    /// Put a named section marker on the timeline, e.g. "Explore" at beat
    /// 0 and "Combat" at beat 32. A section runs from its marker to the
    /// next; Godot export can turn sections into an AudioStreamInteractive.
    /// One marker per beat.
    AddMarker { name: String, start_beats: f64 },
    /// Move a marker to another beat.
    MoveMarker { marker_id: Id, start_beats: f64 },
    /// Rename a marker (its section).
    RenameMarker { marker_id: Id, name: String },
    /// Delete a marker; its section joins the one before.
    RemoveMarker { marker_id: Id },
    /// Put back a deleted marker exactly as it was (used by undo).
    RestoreMarker { marker: crate::project::Marker },

    // ---- Saved versions ----
    /// Save the song as it is now as a named version (kept in the song
    /// file), so it can be loaded or compared later. Take one before big
    /// changes, e.g. "Before Claude's changes" or "Calm verse". Returns
    /// nothing; read the id with get_song.
    TakeSnapshot { name: String },
    /// Replace the song's music (tracks, mixer, tempo, meter, loop) with a
    /// saved version. The song's name and its versions stay. Undoable.
    LoadSnapshot { snapshot_id: Id },
    /// Rename a saved version.
    RenameSnapshot { snapshot_id: Id, name: String },
    /// Delete a saved version.
    DeleteSnapshot { snapshot_id: Id },
    /// Put back a deleted version exactly as it was (used by undo).
    RestoreSnapshot {
        snapshot: crate::project::Snapshot,
        index: usize,
    },
    /// Replace the song's music wholesale (used by undo of load_snapshot).
    SetSongState {
        song: Box<crate::project::SongState>,
    },
    /// Silence a clip without deleting it (or bring it back).
    SetClipMuted { clip_id: ClipId, muted: bool },
    /// Comping: make this clip the one heard over its stretch of time. Other
    /// clips on the same track that overlap it are muted (kept as takes),
    /// this one is unmuted, and audio clips get short fades at their edges
    /// so the joins between takes are smooth. Split takes at the same points
    /// first (split_clip) to choose the best part of each.
    CompTake { clip_id: ClipId },
    /// Swing (shuffle) a note clip: every second step plays late. Notes
    /// keep their written positions; playback, exports and MIDI files hear
    /// the swing. `swing: null` makes the clip straight again. Errors on
    /// audio clips.
    SetClipSwing {
        clip_id: ClipId,
        swing: Option<crate::project::Swing>,
    },
    /// Copy a clip, by default placing the copy right after the original.
    DuplicateClip {
        clip_id: ClipId,
        /// Where the copy starts; defaults to the original's end.
        start_beats: Option<f64>,
        /// Note clips: keep the copy linked to the original, so editing the
        /// notes of either changes both (default false: an independent copy).
        #[serde(default)]
        linked: bool,
    },
    /// Link note clips so they keep the same notes and swing: they all take
    /// the first clip's notes now, and editing any of them later changes all
    /// (e.g. one melody used in the quiet, normal and boss sections).
    LinkClips { clip_ids: Vec<ClipId> },
    /// Make a linked clip independent again (its notes stay as they are).
    UnlinkClip { clip_id: ClipId },
    /// Set a clip's link group directly (used by undo).
    SetClipLink { clip_id: ClipId, link: Option<Id> },
    /// Replace a clip's notes and swing wholesale (used by undo of linked
    /// edits).
    SetClipNotes {
        clip_id: ClipId,
        notes: Vec<crate::project::Note>,
        swing: Option<crate::project::Swing>,
    },

    /// Split a clip in two at a point on the timeline. Works for note and
    /// audio clips; notes go with the half they start in.
    SplitClip {
        clip_id: ClipId,
        /// Where to cut, in beats from the song start (inside the clip).
        at_beats: f64,
    },
    /// Move a clip's start while keeping its end in place, trimming (or
    /// revealing) the beginning. For audio clips this skips into the
    /// recording; for note clips, notes before the new start are removed.
    TrimClipStart {
        clip_id: ClipId,
        /// New start, in beats from the song start.
        start_beats: f64,
    },

    // ---- Audio ----
    /// Place audio that is already in the project's audio folder on an audio
    /// track. To bring in a new file (wav, mp3, m4a from a phone...), use the
    /// import_audio tool instead; it copies the file and calls this.
    AddAudioClip {
        /// An audio track (instrument "audio").
        track_id: TrackId,
        /// Start on the timeline, in beats from the song start.
        start_beats: f64,
        /// Which audio to play; copy it from an existing clip's `audio`.
        audio: AudioRegion,
        /// Length in beats; defaults to the rest of the file at the current tempo.
        length_beats: Option<f64>,
        /// Optional name; defaults to the file name.
        name: Option<String>,
    },
    /// Make an audio clip follow the song's tempo, stretching it without
    /// changing its pitch. `source_bpm` is the tempo it was recorded at
    /// (usually the song's current tempo, so nothing changes until the
    /// tempo does); null plays it at its own speed again.
    SetClipTempo {
        clip_id: ClipId,
        source_bpm: Option<f64>,
    },
    /// Change an audio clip's volume and fades. Omitted fields keep their value.
    SetAudioClip {
        clip_id: ClipId,
        /// Clip volume in dB (-60 to +24; 0 = as recorded).
        gain_db: Option<f64>,
        /// Fade-in length in seconds.
        fade_in_seconds: Option<f64>,
        /// Fade-out length in seconds.
        fade_out_seconds: Option<f64>,
    },

    /// Choose the SFZ sample pack a sampler track plays (a downloaded piano,
    /// bass, ...): the absolute path of its .sfz file, or null to unload.
    LoadSamplePack {
        track_id: TrackId,
        path: Option<String>,
    },

    /// Set parameters of a plugin, by the parameter ids plugin_params
    /// lists: a track's plugin instrument (track_id, no effect_id) or a
    /// plugin effect (effect_id, with track_id = the track or bus it's on;
    /// leave track_id out for the master). Values are 0–1 on the plugin's
    /// own scale (plugin_params shows what each means, e.g. 0.5 = "-6 dB").
    /// Fails if there's no such plugin or an id isn't one of its parameters.
    SetPluginParams {
        track_id: Option<TrackId>,
        effect_id: Option<EffectId>,
        /// Parameter id → value (0–1).
        #[serde(deserialize_with = "crate::plugin::id_map::deserialize")]
        params: std::collections::BTreeMap<u32, f64>,
    },

    // ---- Automation ----
    /// Add an automation lane that moves one of a track's settings over
    /// time: volume (dB), pan (-1..1), an instrument parameter, or a
    /// parameter of an effect on the track (ids from get_track). Points are
    /// (beats, value) pairs; values change in straight lines between them.
    /// Example: a filter sweep is two points on instrument param
    /// "filter.cutoff_hz", e.g. (0, 300) and (16, 6000).
    AddAutomationLane {
        track_id: TrackId,
        target: AutomationTarget,
        #[serde(default)]
        points: Vec<AutomationPoint>,
    },
    /// Delete an automation lane; the setting goes back to its slider value.
    RemoveAutomationLane { track_id: TrackId, lane_id: LaneId },
    /// Put back a deleted automation lane exactly as it was (used by undo).
    RestoreAutomationLane {
        track_id: TrackId,
        index: usize,
        lane: AutomationLane,
    },
    /// Replace all points of an automation lane.
    SetAutomationPoints {
        track_id: TrackId,
        lane_id: LaneId,
        points: Vec<AutomationPoint>,
    },
    /// Turn an automation lane on or off without deleting it.
    SetAutomationEnabled {
        track_id: TrackId,
        lane_id: LaneId,
        enabled: bool,
    },

    // ---- Notes ----
    /// Add notes to a clip. Times are relative to the clip start.
    AddNotes {
        clip_id: ClipId,
        notes: Vec<NoteInput>,
    },
    /// Remove notes from a clip by id.
    RemoveNotes {
        clip_id: ClipId,
        note_ids: Vec<NoteId>,
    },
    /// Change existing notes. Omitted fields keep their value.
    EditNotes {
        clip_id: ClipId,
        edits: Vec<NoteEdit>,
    },
    /// Snap note starts (and optionally lengths) to a grid, for example
    /// 0.25 beats for 1/16 notes in 4/4. Applies to all notes in the clip
    /// unless `note_ids` is given.
    QuantizeNotes {
        clip_id: ClipId,
        /// Grid size in beats: 1 = quarter, 0.5 = eighth, 0.25 = sixteenth.
        grid_beats: f64,
        /// How far to move toward the grid, 0.0–1.0 (default 1.0 = fully).
        strength: Option<f64>,
        /// Also snap note lengths to the grid (default false).
        #[serde(default)]
        lengths: bool,
        note_ids: Option<Vec<NoteId>>,
    },
    /// Shift notes up or down in pitch. Applies to all notes in the clip
    /// unless `note_ids` is given.
    TransposeNotes {
        clip_id: ClipId,
        /// Semitones to shift; 12 = one octave up, -12 = one octave down.
        semitones: i32,
        note_ids: Option<Vec<NoteId>>,
    },
    /// Make programmed notes sound played: nudge each note's start by up to
    /// ±`timing_beats` and its velocity by up to ±`velocity`, at random.
    /// The same `seed` always gives the same result. Applies to all notes
    /// in the clip unless `note_ids` is given. Typical: timing 0.01–0.03
    /// beats, velocity 5–15.
    HumanizeNotes {
        clip_id: ClipId,
        /// Largest timing shift, in beats (0–0.25).
        timing_beats: f64,
        /// Largest velocity change (0–64).
        velocity: u8,
        /// Any number; changing it gives a different variation.
        seed: u64,
        note_ids: Option<Vec<NoteId>>,
    },

    // ---- Grouping ----
    /// Apply several commands in order as ONE undoable step. If any command
    /// fails, none of them are applied. Use this for multi-step edits such as
    /// "add a track, give it a preset, and write a clip".
    Batch { commands: Vec<Command> },
}

/// Why a Command was rejected. A rejected Command leaves the project unchanged.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum CommandError {
    #[error("name must not be empty")]
    EmptyName,
    #[error("tempo must be between {MIN_TEMPO_BPM} and {MAX_TEMPO_BPM} BPM, got {0}")]
    TempoOutOfRange(f64),
    #[error("time signature {0}/{1} is not supported")]
    InvalidTimeSignature(u8, u8),
    #[error("there is no track with id {0}")]
    UnknownTrack(TrackId),
    #[error("there is no clip with id {0}")]
    UnknownClip(ClipId),
    #[error("clip has no note with id {0}")]
    UnknownNote(NoteId),
    #[error("there is no effect with id {0} there")]
    UnknownEffect(EffectId),
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
    #[error("{what} {reason}")]
    Invalid { what: String, reason: String },
    #[error("id {0} is already used in this project")]
    IdInUse(Id),
    #[error("a project can have at most {0} tracks")]
    TooManyTracks(usize),
    #[error("step {} ({command}) failed: {source}; nothing was changed", index + 1)]
    BatchStep {
        index: usize,
        command: String,
        source: Box<CommandError>,
    },
}

pub(crate) fn invalid(what: &str, reason: impl Into<String>) -> CommandError {
    CommandError::Invalid {
        what: what.to_owned(),
        reason: reason.into(),
    }
}

impl Command {
    /// Applies the edit and returns the Command that reverses it.
    pub fn apply(self, project: &mut Project) -> Result<Command, CommandError> {
        let edited = links::note_target(&self);
        let cut = links::cut_target(&self)
            .filter(|id| project.clip(*id).is_some_and(|(_, c)| c.link.is_some()));
        let first_new_id = project.next_id;
        let inverse = self.apply_one(project)?;
        // Linked clips follow the one that was edited. Undo puts the
        // edited clip back first (which re-copies), then the partners
        // exactly as they were.
        if let Some(clip_id) = edited {
            let mut undo = links::propagate(project, clip_id);
            if !undo.is_empty() {
                undo.insert(0, inverse);
                return Ok(Command::Batch { commands: undo });
            }
        }
        // A linked clip that is split or trimmed becomes its own part (both
        // halves, after a split).
        if let Some(clip_id) = cut {
            // The cut clip, and the new half a split made.
            let pieces: Vec<ClipId> = project
                .tracks
                .iter()
                .flat_map(|t| &t.clips)
                .filter(|c| c.link.is_some() && (c.id == clip_id || c.id >= first_new_id))
                .map(|c| c.id)
                .collect();
            let mut undo = vec![inverse];
            for id in pieces {
                undo.insert(0, links::set_link(project, id, None)?);
            }
            return Ok(Command::Batch { commands: undo });
        }
        Ok(inverse)
    }

    fn apply_one(self, project: &mut Project) -> Result<Command, CommandError> {
        use Command as C;
        match self {
            C::RenameProject { name } => song::rename(project, name),
            C::SetTempo { bpm } => song::set_tempo(project, bpm),
            C::SetTimeSignature {
                numerator,
                denominator,
            } => song::set_time_signature(project, numerator, denominator),
            C::SetLoop {
                enabled,
                start_beats,
                end_beats,
            } => song::set_loop(project, enabled, start_beats, end_beats),

            C::AddTrack {
                name,
                instrument,
                preset,
                index,
            } => tracks::add(project, name, instrument, preset, index),
            C::RemoveTrack { track_id } => tracks::remove(project, track_id),
            C::RestoreTrack { index, track } => tracks::restore(project, index, track),
            C::RenameTrack { track_id, name } => tracks::rename(project, track_id, name),
            C::MoveTrack { track_id, index } => tracks::move_to(project, track_id, index),
            C::SetTrackMixer {
                track_id,
                volume_db,
                pan,
                mute,
                solo,
            } => tracks::set_mixer(project, track_id, volume_db, pan, mute, solo),
            C::SetMasterVolume { volume_db } => tracks::set_master_volume(project, volume_db),
            C::AddChord {
                start_beats,
                root,
                quality,
                bass,
            } => chords::add(project, start_beats, root, quality, bass),
            C::SetChord {
                chord_id,
                root,
                quality,
                bass,
            } => chords::set(project, chord_id, root, quality, bass),
            C::MoveChord {
                chord_id,
                start_beats,
            } => chords::move_to(project, chord_id, start_beats),
            C::RemoveChord { chord_id } => chords::remove(project, chord_id),
            C::RestoreChord { chord } => chords::restore(project, chord),
            C::SetKey { key } => {
                if let Some(k) = key
                    && k.tonic > 11
                {
                    return Err(invalid(
                        "key",
                        format!("tonic must be 0–11, got {}", k.tonic),
                    ));
                }
                let old = std::mem::replace(&mut project.key, key);
                Ok(C::SetKey { key: old })
            }
            C::AddBus { name } => buses::add(project, name),
            C::RemoveBus { bus_id } => buses::remove(project, bus_id),
            C::RestoreBus {
                bus,
                index,
                outputs,
                sends,
            } => buses::restore(project, bus, index, outputs, sends),
            C::RenameBus { bus_id, name } => buses::rename(project, bus_id, name),
            C::SetBusMixer {
                bus_id,
                volume_db,
                pan,
                mute,
            } => buses::set_mixer(project, bus_id, volume_db, pan, mute),
            C::SetTrackOutput { track_id, bus_id } => buses::set_output(project, track_id, bus_id),
            C::FreezeTrack { track_id, frozen } => {
                audio::check_file_name(&frozen.file)?;
                let t = project
                    .track_mut(track_id)
                    .ok_or(CommandError::UnknownTrack(track_id))?;
                if t.instrument.kind.is_audio() {
                    return Err(invalid(
                        "track",
                        "audio tracks are already audio; there's nothing to freeze",
                    ));
                }
                Ok(match t.frozen.replace(frozen) {
                    Some(old) => C::FreezeTrack {
                        track_id,
                        frozen: old,
                    },
                    None => C::UnfreezeTrack { track_id },
                })
            }
            C::UnfreezeTrack { track_id } => {
                let t = project
                    .track_mut(track_id)
                    .ok_or(CommandError::UnknownTrack(track_id))?;
                let old = t
                    .frozen
                    .take()
                    .ok_or_else(|| invalid("track", format!("\"{}\" isn't frozen", t.name)))?;
                Ok(C::FreezeTrack {
                    track_id,
                    frozen: old,
                })
            }
            C::SetSend {
                track_id,
                bus_id,
                level_db,
                pre_fader,
            } => buses::set_send(project, track_id, bus_id, level_db, pre_fader),
            C::RemoveSend { track_id, bus_id } => buses::remove_send(project, track_id, bus_id),

            C::SetInstrumentParam {
                track_id,
                param,
                value,
            } => instruments::set_param(project, track_id, param, value),
            C::LoadPreset { track_id, preset } => {
                instruments::load_preset(project, track_id, preset)
            }
            C::SetInstrument {
                track_id,
                instrument,
            } => instruments::set_instrument(project, track_id, instrument),

            C::AddEffect {
                track_id,
                kind,
                index,
            } => effects::add(project, track_id, kind, index),
            C::AddPluginEffect {
                track_id,
                plugin,
                index,
            } => effects::add_plugin(project, track_id, *plugin, index),
            C::RemoveEffect {
                track_id,
                effect_id,
            } => effects::remove(project, track_id, effect_id),
            C::RestoreEffect {
                track_id,
                index,
                effect,
            } => effects::restore(project, track_id, index, effect),
            C::SetEffectParam {
                track_id,
                effect_id,
                param,
                value,
            } => effects::set_param(project, track_id, effect_id, param, value),
            C::SetEffectEnabled {
                track_id,
                effect_id,
                enabled,
            } => effects::set_enabled(project, track_id, effect_id, enabled),
            C::SetEffectSidechain {
                track_id,
                effect_id,
                source,
            } => effects::set_sidechain(project, track_id, effect_id, source),

            C::CreateClip {
                track_id,
                start_beats,
                length_beats,
                name,
                notes,
            } => clips::create(project, track_id, start_beats, length_beats, name, notes),
            C::DeleteClip { clip_id } => clips::delete(project, clip_id),
            C::RestoreClip { track_id, clip } => clips::restore(project, track_id, clip),
            C::MoveClip {
                clip_id,
                start_beats,
                track_id,
            } => clips::move_clip(project, clip_id, start_beats, track_id),
            C::ResizeClip {
                clip_id,
                length_beats,
            } => clips::resize(project, clip_id, length_beats),
            C::RenameClip { clip_id, name } => clips::rename(project, clip_id, name),
            C::AddMarker { name, start_beats } => markers::add(project, name, start_beats),
            C::MoveMarker {
                marker_id,
                start_beats,
            } => markers::move_to(project, marker_id, start_beats),
            C::RenameMarker { marker_id, name } => markers::rename(project, marker_id, name),
            C::RemoveMarker { marker_id } => markers::remove(project, marker_id),
            C::RestoreMarker { marker } => markers::restore(project, marker),
            C::TakeSnapshot { name } => snapshots::take(project, name),
            C::LoadSnapshot { snapshot_id } => snapshots::load(project, snapshot_id),
            C::RenameSnapshot { snapshot_id, name } => {
                snapshots::rename(project, snapshot_id, name)
            }
            C::DeleteSnapshot { snapshot_id } => snapshots::delete(project, snapshot_id),
            C::RestoreSnapshot { snapshot, index } => snapshots::restore(project, snapshot, index),
            C::SetSongState { song } => snapshots::set_state(project, *song),
            C::SetClipSwing { clip_id, swing } => clips::set_swing(project, clip_id, swing),
            C::SetClipMuted { clip_id, muted } => {
                let clip = project
                    .clip_mut(clip_id)
                    .ok_or(CommandError::UnknownClip(clip_id))?;
                let old = std::mem::replace(&mut clip.muted, muted);
                Ok(C::SetClipMuted {
                    clip_id,
                    muted: old,
                })
            }
            C::CompTake { clip_id } => {
                let commands = comp_take(project, clip_id)?;
                batch(project, commands)
            }
            C::DuplicateClip {
                clip_id,
                start_beats,
                linked,
            } => {
                let before = project.next_id;
                let inverse = clips::duplicate(project, clip_id, start_beats)?;
                if !linked || project.clip(clip_id).is_some_and(|(_, c)| c.is_audio()) {
                    return Ok(inverse);
                }
                // The copy is the newest clip; link the two.
                let copy = project
                    .tracks
                    .iter()
                    .flat_map(|t| &t.clips)
                    .filter(|c| c.id >= before)
                    .map(|c| c.id)
                    .max()
                    .ok_or(CommandError::UnknownClip(clip_id))?;
                match links::link(project, vec![clip_id, copy]) {
                    Ok(Command::Batch { commands }) => {
                        let mut undo = commands;
                        undo.push(inverse);
                        Ok(C::Batch { commands: undo })
                    }
                    Ok(other) => Ok(C::Batch {
                        commands: vec![other, inverse],
                    }),
                    Err(e) => {
                        let _ = inverse.apply(project);
                        Err(e)
                    }
                }
            }
            C::LinkClips { clip_ids } => links::link(project, clip_ids),
            C::UnlinkClip { clip_id } => {
                if project.clip(clip_id).is_some_and(|(_, c)| c.link.is_none()) {
                    return Err(invalid("clip", "isn't linked"));
                }
                links::set_link(project, clip_id, None)
            }
            C::SetClipLink { clip_id, link } => links::set_link(project, clip_id, link),
            C::SetClipNotes {
                clip_id,
                notes,
                swing,
            } => links::set_notes(project, clip_id, notes, swing),

            C::SplitClip { clip_id, at_beats } => audio::split(project, clip_id, at_beats),
            C::TrimClipStart {
                clip_id,
                start_beats,
            } => audio::trim_start(project, clip_id, start_beats),
            C::AddAudioClip {
                track_id,
                start_beats,
                audio,
                length_beats,
                name,
            } => audio::add(project, track_id, start_beats, audio, length_beats, name),
            C::SetClipTempo {
                clip_id,
                source_bpm,
            } => audio::set_clip_tempo(project, clip_id, source_bpm),
            C::SetAudioClip {
                clip_id,
                gain_db,
                fade_in_seconds,
                fade_out_seconds,
            } => audio::set(project, clip_id, gain_db, fade_in_seconds, fade_out_seconds),
            C::LoadSamplePack { track_id, path } => {
                instruments::load_sample_pack(project, track_id, path)
            }
            C::SetPluginParams {
                track_id,
                effect_id,
                params,
            } => instruments::set_plugin_params(project, track_id, effect_id, params),
            C::AddAutomationLane {
                track_id,
                target,
                points,
            } => automation::add(project, track_id, target, points),
            C::RemoveAutomationLane { track_id, lane_id } => {
                automation::remove(project, track_id, lane_id)
            }
            C::RestoreAutomationLane {
                track_id,
                index,
                lane,
            } => automation::restore(project, track_id, index, lane),
            C::SetAutomationPoints {
                track_id,
                lane_id,
                points,
            } => automation::set_points(project, track_id, lane_id, points),
            C::SetAutomationEnabled {
                track_id,
                lane_id,
                enabled,
            } => automation::set_enabled(project, track_id, lane_id, enabled),
            C::AddNotes { clip_id, notes } => notes::add(project, clip_id, notes),
            C::RemoveNotes { clip_id, note_ids } => notes::remove(project, clip_id, note_ids),
            C::EditNotes { clip_id, edits } => notes::edit(project, clip_id, edits),
            C::QuantizeNotes {
                clip_id,
                grid_beats,
                strength,
                lengths,
                note_ids,
            } => notes::quantize(project, clip_id, grid_beats, strength, lengths, note_ids),
            C::TransposeNotes {
                clip_id,
                semitones,
                note_ids,
            } => notes::transpose(project, clip_id, semitones, note_ids),
            C::HumanizeNotes {
                clip_id,
                timing_beats,
                velocity,
                seed,
                note_ids,
            } => notes::humanize(project, clip_id, timing_beats, velocity, seed, note_ids),
            C::Batch { commands } => batch(project, commands),
        }
    }

    /// The command's snake_case name, as used in JSON and MCP tool names.
    pub fn name(&self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.get("command").and_then(|c| c.as_str()).map(str::to_owned))
            .unwrap_or_default()
    }

    /// True when `next` edits the same thing as `self` in the same way, so a
    /// drag (of a slider, clip, or note) becomes a single undo step.
    pub(crate) fn coalesces_with(&self, next: &Command) -> bool {
        use Command as C;
        match (self, next) {
            (
                C::SetInstrumentParam {
                    track_id: a,
                    param: pa,
                    ..
                },
                C::SetInstrumentParam {
                    track_id: b,
                    param: pb,
                    ..
                },
            ) => a == b && pa == pb,
            (
                C::SetTrackMixer {
                    track_id: a,
                    volume_db: va,
                    pan: pa,
                    mute: ma,
                    solo: sa,
                },
                C::SetTrackMixer {
                    track_id: b,
                    volume_db: vb,
                    pan: pb,
                    mute: mb,
                    solo: sb,
                },
            ) => {
                a == b
                    && va.is_some() == vb.is_some()
                    && pa.is_some() == pb.is_some()
                    && ma.is_none()
                    && mb.is_none()
                    && sa.is_none()
                    && sb.is_none()
            }
            (C::SetMasterVolume { .. }, C::SetMasterVolume { .. }) => true,
            (
                C::SetPluginParams {
                    track_id: a,
                    effect_id: ea,
                    params: pa,
                },
                C::SetPluginParams {
                    track_id: b,
                    effect_id: eb,
                    params: pb,
                },
            ) => a == b && ea == eb && pa.keys().eq(pb.keys()),
            (
                C::SetEffectParam {
                    track_id: ta,
                    effect_id: ea,
                    param: pa,
                    ..
                },
                C::SetEffectParam {
                    track_id: tb,
                    effect_id: eb,
                    param: pb,
                    ..
                },
            ) => ta == tb && ea == eb && pa == pb,
            (
                C::MoveClip {
                    clip_id: a,
                    start_beats: sa,
                    track_id: ta,
                },
                C::MoveClip {
                    clip_id: b,
                    start_beats: sb,
                    track_id: tb,
                },
            ) => a == b && sa.is_some() == sb.is_some() && ta.is_some() == tb.is_some(),
            (C::ResizeClip { clip_id: a, .. }, C::ResizeClip { clip_id: b, .. }) => a == b,
            (C::SetClipSwing { clip_id: a, .. }, C::SetClipSwing { clip_id: b, .. }) => a == b,
            (
                C::SetBusMixer {
                    bus_id: a,
                    volume_db: va,
                    pan: pa,
                    mute: ma,
                },
                C::SetBusMixer {
                    bus_id: b,
                    volume_db: vb,
                    pan: pb,
                    mute: mb,
                },
            ) => {
                a == b
                    && va.is_some() == vb.is_some()
                    && pa.is_some() == pb.is_some()
                    && ma.is_some() == mb.is_some()
            }
            (
                C::SetSend {
                    track_id: ta,
                    bus_id: ba,
                    level_db: la,
                    pre_fader: pa,
                },
                C::SetSend {
                    track_id: tb,
                    bus_id: bb,
                    level_db: lb,
                    pre_fader: pb,
                },
            ) => {
                ta == tb && ba == bb && la.is_some() == lb.is_some() && pa.is_some() == pb.is_some()
            }
            (C::MoveMarker { marker_id: a, .. }, C::MoveMarker { marker_id: b, .. }) => a == b,
            (C::MoveChord { chord_id: a, .. }, C::MoveChord { chord_id: b, .. }) => a == b,
            (C::TrimClipStart { clip_id: a, .. }, C::TrimClipStart { clip_id: b, .. }) => a == b,
            (
                C::SetAutomationPoints { lane_id: a, .. },
                C::SetAutomationPoints { lane_id: b, .. },
            ) => a == b,
            (
                C::SetAudioClip {
                    clip_id: a,
                    gain_db: ga,
                    fade_in_seconds: ia,
                    fade_out_seconds: oa,
                },
                C::SetAudioClip {
                    clip_id: b,
                    gain_db: gb,
                    fade_in_seconds: ib,
                    fade_out_seconds: ob,
                },
            ) => {
                a == b
                    && ga.is_some() == gb.is_some()
                    && ia.is_some() == ib.is_some()
                    && oa.is_some() == ob.is_some()
            }
            (
                C::EditNotes {
                    clip_id: a,
                    edits: ea,
                },
                C::EditNotes {
                    clip_id: b,
                    edits: eb,
                },
            ) => a == b && ea.len() == eb.len() && ea.iter().zip(eb).all(|(x, y)| x.same_shape(y)),
            _ => false,
        }
    }
}

/// Shortest fade at a join between takes, in seconds.
const TAKE_JOIN_FADE_SECONDS: f64 = 0.01;

/// The edits that make `clip_id` the heard take over its span.
fn comp_take(project: &Project, clip_id: ClipId) -> Result<Vec<Command>, CommandError> {
    let (track_id, clip) = project
        .clip(clip_id)
        .ok_or(CommandError::UnknownClip(clip_id))?;
    let (start, end) = (clip.start_beats, clip.start_beats + clip.length_beats);
    let track = project
        .track(track_id)
        .ok_or(CommandError::UnknownTrack(track_id))?;
    let mut commands: Vec<Command> = track
        .clips
        .iter()
        .filter(|c| {
            c.id != clip_id
                && !c.muted
                && c.start_beats < end - 1e-9
                && c.start_beats + c.length_beats > start + 1e-9
        })
        .map(|c| Command::SetClipMuted {
            clip_id: c.id,
            muted: true,
        })
        .collect();
    if clip.muted {
        commands.push(Command::SetClipMuted {
            clip_id,
            muted: false,
        });
    }
    if let Some(a) = &clip.audio {
        let fade = |f: f64| (f < TAKE_JOIN_FADE_SECONDS).then_some(TAKE_JOIN_FADE_SECONDS);
        let (fade_in, fade_out) = (fade(a.fade_in_seconds), fade(a.fade_out_seconds));
        if fade_in.is_some() || fade_out.is_some() {
            commands.push(Command::SetAudioClip {
                clip_id,
                gain_db: None,
                fade_in_seconds: fade_in,
                fade_out_seconds: fade_out,
            });
        }
    }
    Ok(commands)
}

/// Applies commands in order; on failure, rolls back the ones already applied.
fn batch(project: &mut Project, commands: Vec<Command>) -> Result<Command, CommandError> {
    let mut inverses = Vec::with_capacity(commands.len());
    for (index, command) in commands.into_iter().enumerate() {
        let name = command.name();
        match command.apply(project) {
            Ok(inverse) => inverses.push(inverse),
            Err(source) => {
                for inverse in inverses.into_iter().rev() {
                    // Inverses of successful applies are valid by construction.
                    let _ = inverse.apply(project);
                }
                return Err(CommandError::BatchStep {
                    index,
                    command: name,
                    source: Box::new(source),
                });
            }
        }
    }
    inverses.reverse();
    Ok(Command::Batch { commands: inverses })
}

/// JSON Schema for [`Command`]. The MCP server turns each variant into a tool.
pub fn command_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Command)).unwrap_or_default()
}

/// Checks a time in beats is a usable position.
pub(crate) fn check_position(what: &str, beats: f64) -> Result<(), CommandError> {
    if beats.is_finite() && (0.0..=crate::project::MAX_BEATS).contains(&beats) {
        Ok(())
    } else {
        Err(invalid(
            what,
            format!(
                "must be between 0 and {} beats, got {beats}",
                crate::project::MAX_BEATS
            ),
        ))
    }
}

/// Checks a length in beats is positive and not absurd.
pub(crate) fn check_length(what: &str, beats: f64) -> Result<(), CommandError> {
    let min = crate::project::MIN_LENGTH_BEATS;
    if beats.is_finite() && beats >= min && beats <= crate::project::MAX_BEATS {
        Ok(())
    } else {
        Err(invalid(
            what,
            format!(
                "must be between {min} and {} beats, got {beats}",
                crate::project::MAX_BEATS
            ),
        ))
    }
}

pub(crate) fn check_name(name: String) -> Result<String, CommandError> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        Err(CommandError::EmptyName)
    } else {
        Ok(name)
    }
}

pub(crate) fn check_value(
    spec: &crate::instrument::ParamSpec,
    value: f64,
) -> Result<(), CommandError> {
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
