//! Turns notes captured while recording into a clip.

use daw_model::NoteInput;

use crate::RecordedEvent;

/// A clip ready to be created with `Command::CreateClip`.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedClip {
    pub start_beats: f64,
    pub length_beats: f64,
    pub notes: Vec<NoteInput>,
}

/// Shortest note a quick tap can produce, in beats.
const MIN_NOTE_BEATS: f64 = 1.0 / 32.0;

/// Pairs note-ons with note-offs. Notes still held when recording stopped
/// end at `stop_beats`. The clip starts on the bar where the first note was
/// played and lasts a whole number of bars.
pub fn clip_from_recording(
    events: &[RecordedEvent],
    stop_beats: f64,
    beats_per_bar: f64,
) -> Option<RecordedClip> {
    let mut held: [Option<(f64, u8)>; 128] = [None; 128];
    let mut notes: Vec<(u8, f64, f64, u8)> = Vec::new();
    let finish = |notes: &mut Vec<_>, pitch: u8, start: f64, end: f64, vel: u8| {
        notes.push((pitch, start, (end - start).max(MIN_NOTE_BEATS), vel));
    };
    for e in events {
        let pitch = e.note.min(127);
        let slot = &mut held[usize::from(pitch)];
        if e.velocity > 0.0 {
            // Retrigger without a note-off: close the old note first.
            if let Some((start, vel)) = slot.take() {
                finish(&mut notes, pitch, start, e.beat, vel);
            }
            let vel = (e.velocity * 127.0).round().clamp(1.0, 127.0) as u8;
            *slot = Some((e.beat, vel));
        } else if let Some((start, vel)) = slot.take() {
            finish(&mut notes, pitch, start, e.beat, vel);
        }
    }
    for (pitch, slot) in held.iter().enumerate() {
        if let Some((start, vel)) = slot {
            finish(
                &mut notes,
                pitch as u8,
                *start,
                stop_beats.max(*start),
                *vel,
            );
        }
    }
    if notes.is_empty() {
        return None;
    }
    let bar = beats_per_bar.max(1.0);
    let first = notes.iter().map(|n| n.1).fold(f64::INFINITY, f64::min);
    let last = notes.iter().map(|n| n.1 + n.2).fold(0.0, f64::max);
    let start_beats = (first / bar).floor().max(0.0) * bar;
    let length_beats = (((last - start_beats) / bar).ceil() * bar).max(bar);
    notes.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    Some(RecordedClip {
        start_beats,
        length_beats,
        notes: notes
            .into_iter()
            .map(|(pitch, start, len, velocity)| NoteInput {
                chance: 100,
                pitch,
                start_beats: start - start_beats,
                length_beats: len,
                velocity,
                id: None,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(beat: f64, note: u8, velocity: f32) -> RecordedEvent {
        RecordedEvent {
            beat,
            note,
            velocity,
        }
    }

    #[test]
    fn pairs_notes_and_snaps_clip_to_bars() {
        let c = clip_from_recording(
            &[
                ev(5.0, 60, 1.0),
                ev(5.5, 64, 0.5),
                ev(6.0, 60, 0.0),
                ev(9.5, 64, 0.0),
            ],
            12.0,
            4.0,
        )
        .expect("clip");
        assert_eq!(c.start_beats, 4.0);
        assert_eq!(c.length_beats, 8.0);
        assert_eq!(c.notes.len(), 2);
        assert_eq!(
            (
                c.notes[0].pitch,
                c.notes[0].start_beats,
                c.notes[0].length_beats
            ),
            (60, 1.0, 1.0)
        );
        assert_eq!(c.notes[0].velocity, 127);
        assert_eq!((c.notes[1].pitch, c.notes[1].length_beats), (64, 4.0));
        assert_eq!(c.notes[1].velocity, 64);
    }

    #[test]
    fn held_notes_end_when_recording_stops() {
        let c = clip_from_recording(&[ev(1.0, 48, 0.8)], 3.0, 4.0).expect("clip");
        assert_eq!(c.notes[0].length_beats, 2.0);
        assert_eq!(c.length_beats, 4.0);
    }

    #[test]
    fn nothing_played_means_no_clip() {
        assert!(clip_from_recording(&[], 8.0, 4.0).is_none());
        assert!(clip_from_recording(&[ev(1.0, 60, 0.0)], 8.0, 4.0).is_none());
    }

    #[test]
    fn instant_taps_get_a_minimum_length() {
        let c = clip_from_recording(&[ev(0.0, 36, 1.0), ev(0.0, 36, 0.0)], 1.0, 4.0).expect("clip");
        assert!(c.notes[0].length_beats >= MIN_NOTE_BEATS);
    }
}
