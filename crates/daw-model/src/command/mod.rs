//! Every edit to a project is a [`Command`]. Applying one returns the
//! Command that undoes it, which is how undo/redo works.

mod audio;
mod automation;
mod clips;
mod effects;
mod instruments;
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
            C::TakeSnapshot { name } => snapshots::take(project, name),
            C::LoadSnapshot { snapshot_id } => snapshots::load(project, snapshot_id),
            C::RenameSnapshot { snapshot_id, name } => {
                snapshots::rename(project, snapshot_id, name)
            }
            C::DeleteSnapshot { snapshot_id } => snapshots::delete(project, snapshot_id),
            C::RestoreSnapshot { snapshot, index } => snapshots::restore(project, snapshot, index),
            C::SetSongState { song } => snapshots::set_state(project, *song),
            C::SetClipSwing { clip_id, swing } => clips::set_swing(project, clip_id, swing),
            C::DuplicateClip {
                clip_id,
                start_beats,
            } => clips::duplicate(project, clip_id, start_beats),

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
