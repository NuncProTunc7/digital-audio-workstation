use daw_instruments::InstrumentProcessor;
use daw_model::TrackId;

/// One track's instrument, as the audio thread sees it.
pub struct TrackSlot {
    pub id: TrackId,
    pub instrument: Box<dyn InstrumentProcessor>,
}

/// Everything the audio thread can be told. Sent through a lock-free queue.
pub enum EngineMessage {
    NoteOn {
        track_id: TrackId,
        note: u8,
        /// 0.0–1.0.
        velocity: f32,
    },
    NoteOff {
        track_id: TrackId,
        note: u8,
    },
    ControlChange {
        track_id: TrackId,
        controller: u8,
        value: u8,
    },
    PitchBend {
        track_id: TrackId,
        semitones: f32,
    },
    /// Silences every instrument (panic button, focus loss).
    AllNotesOff,
    SetParam {
        track_id: TrackId,
        index: usize,
        value: f32,
    },
    Play,
    Stop,
    /// Moves the playhead, in beats from the start.
    Locate(f64),
    SetTempo(f64),
    SetTimeSignature {
        numerator: u8,
        denominator: u8,
    },
    SetMetronome(bool),
    /// Swaps in a new set of tracks. The old set is sent back to be freed
    /// off the audio thread.
    ReplaceTracks(Box<[TrackSlot]>),
}
