//! Turns a track's clips into the time-ordered note events the audio
//! thread plays.

use daw_model::Track;

use crate::message::{SeqEvent, Sequence};

/// Flattens all clips on a track. Notes are cut at their clip's end; at the
/// same beat, note-offs come before note-ons so repeated notes retrigger.
pub fn build_sequence(track: &Track) -> Sequence {
    let mut events = Vec::new();
    for clip in &track.clips {
        let clip_end = clip.start_beats + clip.length_beats;
        for n in &clip.notes {
            if n.start_beats >= clip.length_beats {
                continue;
            }
            let on = clip.start_beats + n.start_beats;
            let off = (on + n.length_beats).min(clip_end);
            events.push(SeqEvent {
                beat: on,
                note: n.pitch.min(127),
                velocity: (f32::from(n.velocity) / 127.0).clamp(1.0 / 127.0, 1.0),
            });
            events.push(SeqEvent {
                beat: off,
                note: n.pitch.min(127),
                velocity: 0.0,
            });
        }
    }
    events.sort_by(|a, b| {
        a.beat
            .total_cmp(&b.beat)
            .then_with(|| a.velocity.total_cmp(&b.velocity))
    });
    Sequence { events }
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_model::{Clip, Note, Project};

    fn track_with(clips: Vec<Clip>) -> Track {
        let mut t = Project::default().tracks[0].clone();
        t.clips = clips;
        t
    }

    fn note(id: u32, pitch: u8, start: f64, len: f64) -> Note {
        Note {
            id,
            pitch,
            start_beats: start,
            length_beats: len,
            velocity: 127,
        }
    }

    #[test]
    fn offsets_by_clip_start_and_cuts_at_clip_end() {
        let t = track_with(vec![Clip {
            id: 10,
            name: "c".into(),
            start_beats: 4.0,
            length_beats: 2.0,
            notes: vec![
                note(1, 60, 0.0, 1.0),
                note(2, 62, 1.5, 4.0),
                note(3, 64, 3.0, 1.0),
            ],
        }]);
        let s = build_sequence(&t);
        let summary: Vec<(f64, u8, bool)> = s
            .events
            .iter()
            .map(|e| (e.beat, e.note, e.velocity > 0.0))
            .collect();
        assert_eq!(
            summary,
            vec![
                (4.0, 60, true),
                (5.0, 60, false),
                (5.5, 62, true),
                (6.0, 62, false),
            ]
        );
    }

    #[test]
    fn note_off_sorts_before_note_on_at_the_same_beat() {
        let t = track_with(vec![Clip {
            id: 10,
            name: "c".into(),
            start_beats: 0.0,
            length_beats: 4.0,
            notes: vec![note(1, 60, 0.0, 1.0), note(2, 60, 1.0, 1.0)],
        }]);
        let s = build_sequence(&t);
        assert_eq!(s.events[1].beat, 1.0);
        assert_eq!(s.events[1].velocity, 0.0);
        assert!(s.events[2].velocity > 0.0);
    }
}
