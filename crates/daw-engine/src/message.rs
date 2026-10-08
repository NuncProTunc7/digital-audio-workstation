use std::sync::Arc;

use daw_audio::AudioBuffer;
use daw_effects::EffectProcessor;
use daw_instruments::InstrumentProcessor;
use daw_model::{EffectId, TrackId};

use crate::processor::MAX_BLOCK_FRAMES;

/// A note event in a track's playback sequence, at an absolute beat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeqEvent {
    pub beat: f64,
    pub note: u8,
    /// 0.0 means note-off; otherwise velocity 0.0–1.0.
    pub velocity: f32,
}

/// One audio clip, as the audio thread plays it.
#[derive(Debug, Clone)]
pub struct AudioRegionPlay {
    pub start_beats: f64,
    pub end_beats: f64,
    /// Audio at the engine's sample rate.
    pub buffer: Arc<AudioBuffer>,
    /// Frame of `buffer` heard at `start_beats`.
    pub offset_frames: f64,
    /// Linear clip gain.
    pub gain: f32,
    pub fade_in_frames: f64,
    pub fade_out_frames: f64,
}

/// What an automation curve drives, resolved to engine indices.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AutoTarget {
    /// Fader level in dB.
    Volume,
    Pan,
    /// Instrument parameter index.
    Instrument(usize),
    /// Effect (by id) parameter index.
    Effect {
        effect_id: EffectId,
        index: usize,
    },
}

/// One automation lane, as the audio thread plays it.
#[derive(Debug, Clone)]
pub struct AutoCurve {
    pub target: AutoTarget,
    /// (beats, value in the target's units), sorted by beats.
    pub points: Vec<(f64, f32)>,
    /// Last value applied, so unchanged values aren't re-sent.
    pub(crate) last: f32,
}

impl AutoCurve {
    pub fn new(target: AutoTarget, points: Vec<(f64, f32)>) -> Self {
        Self {
            target,
            points,
            last: f32::NAN,
        }
    }

    /// Value at `beats`: straight lines between points, ends held.
    // RT-SAFE
    pub fn value_at(&self, beats: f64) -> Option<f32> {
        let first = self.points.first()?;
        let i = self.points.partition_point(|p| p.0 <= beats);
        if i == 0 {
            return Some(first.1);
        }
        let (ab, av) = self.points[i - 1];
        let Some(&(bb, bv)) = self.points.get(i) else {
            return Some(av);
        };
        if bb <= ab {
            return Some(bv);
        }
        Some(av + (bv - av) * ((beats - ab) / (bb - ab)) as f32)
    }
}

/// All of a track's clips: notes flattened into time-ordered events, and
/// audio clips in timeline order; plus its automation.
#[derive(Debug, Clone, Default)]
pub struct Sequence {
    pub events: Vec<SeqEvent>,
    pub audio: Vec<AudioRegionPlay>,
    pub automation: Vec<AutoCurve>,
}

/// One effect in a chain, as the audio thread sees it.
pub struct EffectSlot {
    pub id: EffectId,
    pub enabled: bool,
    pub processor: Box<dyn EffectProcessor>,
}

/// An ordered effect chain. Boxed so a whole chain can be swapped in one
/// pointer move on the audio thread.
#[derive(Default)]
pub struct EffectChain {
    pub effects: Vec<EffectSlot>,
}

/// Fader, pan, mute, and solo, as linear values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripSettings {
    pub gain: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
}

/// One track, as the audio thread sees it.
pub struct TrackSlot {
    pub id: TrackId,
    pub instrument: Box<dyn InstrumentProcessor>,
    pub effects: Box<EffectChain>,
    pub sequence: Box<Sequence>,
    pub strip: StripSettings,
    // Audio-thread state, preallocated off the audio thread.
    pub(crate) buf_left: Box<[f32]>,
    pub(crate) buf_right: Box<[f32]>,
    pub(crate) gain_left: daw_dsp::Smoother,
    pub(crate) gain_right: daw_dsp::Smoother,
    pub(crate) cursor: usize,
    /// Notes started by the sequence and not yet stopped (bit per pitch).
    pub(crate) sounding: u128,
}

impl TrackSlot {
    pub fn new(
        id: TrackId,
        instrument: Box<dyn InstrumentProcessor>,
        effects: Box<EffectChain>,
        sequence: Box<Sequence>,
        strip: StripSettings,
        sample_rate_hz: f32,
    ) -> Self {
        let (l, r) = strip.pan_gains();
        let audible = if strip.mute { 0.0 } else { strip.gain };
        Self {
            id,
            instrument,
            effects,
            sequence,
            strip,
            buf_left: vec![0.0; MAX_BLOCK_FRAMES].into_boxed_slice(),
            buf_right: vec![0.0; MAX_BLOCK_FRAMES].into_boxed_slice(),
            gain_left: daw_dsp::Smoother::new(audible * l, 0.01, sample_rate_hz),
            gain_right: daw_dsp::Smoother::new(audible * r, 0.01, sample_rate_hz),
            cursor: 0,
            sounding: 0,
        }
    }
}

impl StripSettings {
    /// Equal-power pan, normalized so center is unity on both sides.
    pub fn pan_gains(&self) -> (f32, f32) {
        let angle = (self.pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
        (
            angle.cos() * std::f32::consts::SQRT_2,
            angle.sin() * std::f32::consts::SQRT_2,
        )
    }
}

/// A note played live on the record track, timestamped by the audio thread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordedEvent {
    pub beat: f64,
    pub note: u8,
    /// 0.0 means note-off.
    pub velocity: f32,
}

/// Things the audio thread hands back to be freed on another thread.
pub enum Garbage {
    Tracks(#[allow(dead_code)] Box<[TrackSlot]>),
    Effects(#[allow(dead_code)] Box<EffectChain>),
    Sequence(#[allow(dead_code)] Box<Sequence>),
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
    SetStrip {
        track_id: TrackId,
        strip: StripSettings,
    },
    SetMasterGain(f32),
    /// `track_id` None targets the master chain.
    SetEffectParam {
        track_id: Option<TrackId>,
        effect_id: EffectId,
        index: usize,
        value: f32,
    },
    SetEffectEnabled {
        track_id: Option<TrackId>,
        effect_id: EffectId,
        enabled: bool,
    },
    ReplaceEffects {
        track_id: Option<TrackId>,
        chain: Box<EffectChain>,
    },
    ReplaceSequence {
        track_id: TrackId,
        sequence: Box<Sequence>,
    },
    /// Swaps in a new set of tracks. The old set is sent back to be freed
    /// off the audio thread.
    ReplaceTracks(Box<[TrackSlot]>),
    Play,
    /// Starts playback after this many beats of metronome clicks (a
    /// count-in), so the song starts where the playhead is on the beat after
    /// the last click. Clicks play even with the metronome off; the song is
    /// silent until the count-in ends. Ignored while already playing.
    CountIn(f64),
    Stop,
    /// Moves the playhead, in beats from the start.
    Locate(f64),
    SetTempo(f64),
    SetTimeSignature {
        numerator: u8,
        denominator: u8,
    },
    SetMetronome(bool),
    SetLoop {
        enabled: bool,
        start_beats: f64,
        end_beats: f64,
    },
    /// Capture live notes on this track (None stops capturing).
    SetRecordTrack(Option<TrackId>),
}
