//! Notation for Nunc Pro Tune: Standard MIDI Files and MusicXML.
//!
//! Exports read a [`Project`](daw_model::Project). Imports produce an
//! [`ImportedSong`], a plain description of parts and notes that the app
//! turns into ordinary Commands (so an import is one undo step).
//!
//! Beats follow the project convention: one beat is one time-signature
//! beat (a quarter in 4/4, an eighth in 6/8). MIDI and MusicXML count in
//! quarter notes, so conversions go through [`quarters_per_beat`].

pub mod midi;
pub mod musicxml;

use daw_model::{NoteInput, TimeSignature, Track};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NotationError {
    #[error("this isn't a MIDI file this app can read: {0}")]
    BadMidi(String),
    #[error("this MIDI file times notes in SMPTE frames, which isn't supported yet")]
    SmpteTiming,
    #[error("this isn't a MusicXML score this app can read: {0}")]
    BadMusicXml(String),
    #[error("the file has no notes")]
    NoNotes,
}

/// A song read from a file, before it becomes Commands.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedSong {
    pub title: Option<String>,
    /// In beats per minute of the time signature's beat.
    pub tempo_bpm: Option<f64>,
    pub time_signature: Option<TimeSignature>,
    pub parts: Vec<ImportedPart>,
}

/// One instrument part: becomes a track with one clip.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedPart {
    pub name: String,
    /// Drum parts use General MIDI drum notes and go on a drum track.
    pub drums: bool,
    /// Suggested synth preset for the part's sound (None for drums).
    pub preset: Option<&'static str>,
    /// Notes with times in beats from the song start.
    pub notes: Vec<NoteInput>,
    /// Clip length in beats (whole bars).
    pub length_beats: f64,
}

/// How many quarter notes one beat of `ts` is (1 in 4/4, 0.5 in 6/8).
pub fn quarters_per_beat(ts: TimeSignature) -> f64 {
    4.0 / f64::from(ts.denominator.max(1))
}

/// Rounds a length up to whole bars (at least one).
pub(crate) fn whole_bars(beats: f64, beats_per_bar: f64) -> f64 {
    ((beats / beats_per_bar - 1e-9).ceil()).max(1.0) * beats_per_bar
}

/// Every note a track plays, cut at clip ends (as the engine plays them):
/// `(start_beats, end_beats, pitch, velocity)` from the song start.
pub(crate) fn played_notes(track: &Track, swung: bool) -> Vec<(f64, f64, u8, u8)> {
    let mut out = Vec::new();
    for clip in &track.clips {
        for n in &clip.notes {
            if n.start_beats >= clip.length_beats {
                continue;
            }
            let written = |beats: f64| clip.start_beats + beats;
            let at = |beats: f64| {
                if swung {
                    clip.played_song_beats(beats)
                } else {
                    written(beats)
                }
            };
            let start = at(n.start_beats);
            let stop = at((n.start_beats + n.length_beats).min(clip.length_beats));
            out.push((start, stop, n.pitch, n.velocity));
        }
    }
    out
}
