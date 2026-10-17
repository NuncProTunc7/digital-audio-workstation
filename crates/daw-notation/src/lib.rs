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
    /// Starting tempo, in beats per minute of the time signature's beat.
    pub tempo_bpm: Option<f64>,
    /// Later tempo changes as (beats from the song start, bpm), in order,
    /// all after beat 0.
    pub tempo_changes: Vec<(f64, f64)>,
    pub time_signature: Option<TimeSignature>,
    pub parts: Vec<ImportedPart>,
}

/// One instrument part: becomes a track with one clip.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedPart {
    pub name: String,
    /// Drum parts use General MIDI drum notes and go on a drum track.
    pub drums: bool,
    /// Suggested preset for the part's sound (None for drums): a synth
    /// preset, or a guitar one for guitar parts.
    pub preset: Option<&'static str>,
    /// Notes with times in beats from the song start.
    pub notes: Vec<NoteInput>,
    /// Clip length in beats (whole bars).
    pub length_beats: f64,
}

/// Splits tempo marks `(beats, bpm)` found in a file into a starting tempo
/// and later changes: sorted, the last mark on a beat wins, and marks that
/// don't change the tempo are dropped. Before the first mark the tempo is
/// `before_first` (MIDI's 120) or, if None, the first mark's.
pub(crate) fn split_tempo_marks(
    mut marks: Vec<(f64, f64)>,
    before_first: Option<f64>,
) -> (Option<f64>, Vec<(f64, f64)>) {
    marks.retain(|&(b, bpm)| b.is_finite() && b >= 0.0 && bpm.is_finite() && bpm > 0.0);
    // Stable: marks on one beat keep their order, so the last one wins.
    marks.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (b, bpm) in marks {
        let bpm = bpm.clamp(daw_model::MIN_TEMPO_BPM, daw_model::MAX_TEMPO_BPM);
        match merged.last_mut() {
            Some(last) if (last.0 - b).abs() < 1e-6 => last.1 = bpm,
            _ => merged.push((b, bpm)),
        }
    }
    let start = match merged.first() {
        Some(&(b, bpm)) if b < 1e-6 => Some(bpm),
        Some(&(_, bpm)) => before_first.or(Some(bpm)),
        None => None,
    };
    let mut changes = Vec::new();
    let mut current = start;
    for (b, bpm) in merged.into_iter().filter(|&(b, _)| b >= 1e-6) {
        if current.is_none_or(|c| (c - bpm).abs() > 1e-3) {
            changes.push((b, bpm));
            current = Some(bpm);
        }
    }
    changes.truncate(daw_model::MAX_TEMPO_CHANGES);
    (start, changes)
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
    for clip in track.clips.iter().filter(|c| !c.muted) {
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
