//! Tempo changes within a song, and the one conversion between beats and
//! seconds that everything (playback, recording, export, analysis, MIDI)
//! uses.
//!
//! A song starts at [`Project::tempo_bpm`](crate::Project::tempo_bpm). Each
//! [`TempoChange`] sets a new tempo from its beat on. A change with `ramp`
//! glides from its own tempo to the next change's, arriving exactly on it
//! (BPM changes in a straight line over beats).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::project::Id;

/// Most tempo changes a song may have.
pub const MAX_TEMPO_CHANGES: usize = 1000;

/// From `beats` on, the song plays at `bpm`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TempoChange {
    pub id: Id,
    /// Where the new tempo takes over, in beats from the song start
    /// (always after beat 0; the starting tempo is the song's tempo).
    pub beats: f64,
    /// Tempo from here, in beats per minute (20–999).
    pub bpm: f64,
    /// Glide: from here the tempo changes smoothly, reaching the next
    /// change's tempo exactly at the next change. Without a later change it
    /// stays steady.
    #[serde(default, skip_serializing_if = "is_false")]
    pub ramp: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// One stretch between tempo changes, with its start already converted.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Segment {
    beats: f64,
    seconds: f64,
    bpm: f64,
    /// BPM gained per beat (0 = steady).
    slope: f64,
}

impl Segment {
    // RT-SAFE
    fn bpm_at(&self, beats: f64) -> f64 {
        self.bpm + self.slope * (beats - self.beats)
    }

    // RT-SAFE
    fn seconds_at(&self, beats: f64) -> f64 {
        let db = beats - self.beats;
        if self.slope == 0.0 {
            return self.seconds + db * 60.0 / self.bpm;
        }
        // dt = 60 db / bpm(b) integrates to a logarithm.
        self.seconds + 60.0 / self.slope * (self.bpm_at(beats) / self.bpm).ln()
    }

    // RT-SAFE
    fn beats_at(&self, seconds: f64) -> f64 {
        let dt = seconds - self.seconds;
        if self.slope == 0.0 {
            return self.beats + dt * self.bpm / 60.0;
        }
        self.beats + self.bpm * ((self.slope * dt / 60.0).exp() - 1.0) / self.slope
    }
}

/// Beats ↔ seconds for a song. Build it with
/// [`Project::tempo_map`](crate::Project::tempo_map) (or [`TempoMap::new`]);
/// queries never allocate, so the audio thread uses it too. Before beat 0 the
/// starting tempo continues backwards (count-ins).
#[derive(Debug, Clone, PartialEq)]
pub struct TempoMap {
    /// Never empty; the first starts at beat 0, second 0.
    segments: Vec<Segment>,
}

impl Default for TempoMap {
    fn default() -> Self {
        Self::steady(120.0)
    }
}

/// Keeps a tempo usable even if bad data slipped through.
fn sane_bpm(bpm: f64) -> f64 {
    if bpm.is_finite() {
        bpm.clamp(crate::project::MIN_TEMPO_BPM, crate::project::MAX_TEMPO_BPM)
    } else {
        120.0
    }
}

impl TempoMap {
    /// One tempo throughout.
    pub fn steady(bpm: f64) -> Self {
        Self::new(bpm, &[])
    }

    /// A map starting at `start_bpm` with `changes` (in any order; changes
    /// at or before beat 0, or on the same beat as an earlier one, are
    /// ignored).
    pub fn new(start_bpm: f64, changes: &[TempoChange]) -> Self {
        let mut sorted: Vec<TempoChange> = changes
            .iter()
            .copied()
            .filter(|c| c.beats.is_finite() && c.beats > 0.0)
            .collect();
        sorted.sort_by(|a, b| a.beats.total_cmp(&b.beats));
        sorted.dedup_by(|b, a| (b.beats - a.beats).abs() < 1e-9);
        // (beats, bpm, ramp) for every stretch, the start included.
        let points: Vec<(f64, f64, bool)> = std::iter::once((0.0, sane_bpm(start_bpm), false))
            .chain(sorted.iter().map(|c| (c.beats, sane_bpm(c.bpm), c.ramp)))
            .collect();
        let mut segments: Vec<Segment> = Vec::with_capacity(points.len());
        for (i, &(beats, bpm, ramp)) in points.iter().enumerate() {
            let slope = match points.get(i + 1) {
                Some(&(next_beats, next_bpm, _)) if ramp => (next_bpm - bpm) / (next_beats - beats),
                _ => 0.0,
            };
            let seconds = segments.last().map_or(0.0, |s| s.seconds_at(beats));
            segments.push(Segment {
                beats,
                seconds,
                bpm,
                slope,
            });
        }
        Self { segments }
    }

    /// The stretch playing at `beats`.
    // RT-SAFE
    fn segment_at_beats(&self, beats: f64) -> &Segment {
        let i = self.segments.partition_point(|s| s.beats <= beats);
        &self.segments[i.saturating_sub(1).min(self.segments.len() - 1)]
    }

    // RT-SAFE
    fn segment_at_seconds(&self, seconds: f64) -> &Segment {
        let i = self.segments.partition_point(|s| s.seconds <= seconds);
        &self.segments[i.saturating_sub(1).min(self.segments.len() - 1)]
    }

    /// Seconds from the song start to `beats`.
    // RT-SAFE
    pub fn seconds_at(&self, beats: f64) -> f64 {
        self.segment_at_beats(beats).seconds_at(beats)
    }

    /// The beat playing `seconds` after the song start.
    // RT-SAFE
    pub fn beats_at(&self, seconds: f64) -> f64 {
        self.segment_at_seconds(seconds).beats_at(seconds)
    }

    /// Seconds between two song positions (negative if `to` is earlier).
    // RT-SAFE
    pub fn seconds_between(&self, from_beats: f64, to_beats: f64) -> f64 {
        self.seconds_at(to_beats) - self.seconds_at(from_beats)
    }

    /// The tempo at `beats`, in BPM (on a change's beat, the new tempo).
    // RT-SAFE
    pub fn bpm_at(&self, beats: f64) -> f64 {
        self.segment_at_beats(beats).bpm_at(beats)
    }

    /// The tempo the song starts at.
    pub fn start_bpm(&self) -> f64 {
        self.segments[0].bpm
    }

    /// The first change strictly after `beats`, if any.
    // RT-SAFE
    pub fn next_change_after(&self, beats: f64) -> Option<f64> {
        let i = self.segments.partition_point(|s| s.beats <= beats);
        self.segments.get(i).map(|s| s.beats)
    }

    /// Whether the tempo is gliding at `beats`.
    // RT-SAFE
    pub fn ramping_at(&self, beats: f64) -> bool {
        self.segment_at_beats(beats).slope != 0.0
    }

    /// Whether the tempo never changes.
    pub fn is_steady(&self) -> bool {
        self.segments.len() == 1
    }

    /// Whether the tempo is the same all the way from `from_beats` to
    /// `to_beats`.
    pub fn steady_between(&self, from_beats: f64, to_beats: f64) -> bool {
        !self.ramping_at(from_beats)
            && self
                .next_change_after(from_beats)
                .is_none_or(|next| next >= to_beats - 1e-9)
    }

    /// Beats where the tempo changes, after beat 0, in order.
    pub fn change_beats(&self) -> impl Iterator<Item = f64> + '_ {
        self.segments.iter().skip(1).map(|s| s.beats)
    }

    /// The average tempo over a stretch: beats per minute of wall-clock time.
    pub fn average_bpm(&self, from_beats: f64, to_beats: f64) -> f64 {
        let seconds = self.seconds_between(from_beats, to_beats);
        if seconds > 0.0 {
            (to_beats - from_beats) * 60.0 / seconds
        } else {
            self.bpm_at(from_beats)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(beats: f64, bpm: f64, ramp: bool) -> TempoChange {
        TempoChange {
            id: 1,
            beats,
            bpm,
            ramp,
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn steady_tempo_is_sixty_over_bpm() {
        let m = TempoMap::steady(120.0);
        assert!(close(m.seconds_at(8.0), 4.0));
        assert!(close(m.beats_at(4.0), 8.0));
        assert!(close(m.seconds_at(-2.0), -1.0), "count-ins run backwards");
        assert!(m.is_steady());
        assert_eq!(m.next_change_after(0.0), None);
    }

    #[test]
    fn a_sudden_change_takes_over_on_its_beat() {
        // 4 beats at 120 (2 s), then 60 BPM: one beat per second.
        let m = TempoMap::new(120.0, &[change(4.0, 60.0, false)]);
        assert!(close(m.seconds_at(4.0), 2.0));
        assert!(close(m.seconds_at(6.0), 4.0));
        assert!(close(m.beats_at(4.0), 6.0));
        assert!(close(m.beats_at(1.0), 2.0));
        assert_eq!(m.bpm_at(3.999), 120.0);
        assert_eq!(m.bpm_at(4.0), 60.0);
        assert_eq!(m.next_change_after(0.0), Some(4.0));
        assert_eq!(m.next_change_after(4.0), None);
        assert!(m.steady_between(0.0, 4.0));
        assert!(!m.steady_between(0.0, 5.0));
    }

    #[test]
    fn a_ramp_glides_to_the_next_tempo() {
        // 60 -> 120 BPM over beats 4..8, then steady 120.
        let m = TempoMap::new(60.0, &[change(4.0, 60.0, true), change(8.0, 120.0, false)]);
        assert!(close(m.bpm_at(6.0), 90.0));
        assert!(m.ramping_at(5.0));
        assert!(!m.ramping_at(8.0));
        // 60/slope * ln(120/60) with slope 15 BPM per beat.
        let ramp_seconds = 4.0 * 2f64.ln();
        assert!(close(m.seconds_at(8.0), 4.0 + ramp_seconds));
        assert!(close(m.seconds_at(10.0), 4.0 + ramp_seconds + 1.0));
        // Faster than the slow tempo, slower than the fast one.
        assert!(ramp_seconds < 4.0 && ramp_seconds > 2.0);
        for b in [0.5, 4.0, 5.3, 7.9, 8.0, 12.0] {
            assert!(close(m.beats_at(m.seconds_at(b)), b), "round trip at {b}");
        }
    }

    #[test]
    fn a_ramp_without_a_later_change_is_steady() {
        let m = TempoMap::new(100.0, &[change(4.0, 80.0, true)]);
        assert!(!m.ramping_at(6.0));
        assert_eq!(m.bpm_at(100.0), 80.0);
    }

    #[test]
    fn unusable_changes_are_ignored() {
        let m = TempoMap::new(
            120.0,
            &[
                change(0.0, 60.0, false),
                change(-4.0, 60.0, false),
                change(f64::NAN, 60.0, false),
                change(4.0, 90.0, false),
                change(4.0, 50.0, false),
            ],
        );
        assert_eq!(m.change_beats().collect::<Vec<_>>(), vec![4.0]);
        assert_eq!(m.start_bpm(), 120.0);
    }

    #[test]
    fn average_bpm_matches_the_time_taken() {
        let m = TempoMap::new(120.0, &[change(4.0, 60.0, false)]);
        // 8 beats in 2 + 4 = 6 s.
        assert!(close(m.average_bpm(0.0, 8.0), 80.0));
        assert!(close(m.average_bpm(0.0, 4.0), 120.0));
    }
}
