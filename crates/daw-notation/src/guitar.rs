//! Guitar tab: which string and fret each note of a guitar track is played
//! on, found by replaying the track the way the guitar hears it.

use daw_model::Track;
use daw_model::instrument::{guitar_reach, guitar_string_for, guitar_strings, strings_for};

/// Where one note sits on the neck.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabNote {
    /// Written start, in beats from the song start (not swung).
    pub start_beats: f64,
    pub pitch: u8,
    /// MusicXML numbering: 1 is the highest (thinnest) string, 6 the lowest.
    pub string: u8,
    /// Frets above the open string; with a capo, counted from the capo.
    pub fret: u8,
}

/// The guitar's open strings for `track` with the capo, and the low string
/// without it.
pub fn open_strings(track: &Track) -> ([u8; 6], u8) {
    let tuning = track.instrument.value("string.tuning").unwrap_or(0.0);
    (
        guitar_strings(&track.instrument),
        strings_for(tuning, 0.0)[0],
    )
}

/// Every note of a guitar track the guitar can reach, on the string the
/// engine plays it on. The notes are replayed in the sequencer's order (by
/// played time, note-offs before note-ons at the same moment, then as
/// stored) so busy strings are the ones the engine sees. Notes out of the
/// tuning's reach are left out.
pub fn guitar_frets(track: &Track) -> Vec<TabNote> {
    let (open, low) = open_strings(track);
    let reach = guitar_reach(open, low);
    // (played beat, is note-on, index into `notes`).
    let mut notes = Vec::new();
    let mut events = Vec::new();
    for clip in track.clips.iter().filter(|c| !c.muted && c.audio.is_none()) {
        for n in &clip.notes {
            if n.start_beats >= clip.length_beats {
                continue;
            }
            let on = clip.played_song_beats(n.start_beats);
            let off =
                clip.played_song_beats((n.start_beats + n.length_beats).min(clip.length_beats));
            let k = notes.len();
            notes.push((clip.start_beats + n.start_beats, n.pitch.min(127)));
            events.push((on, true, k, n.velocity));
            events.push((off, false, k, 0));
        }
    }
    // As the sequencer sorts: by beat, then velocity (offs carry 0).
    events.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.3.cmp(&b.3)));

    let mut busy: [Option<u8>; 6] = [None; 6];
    // Which note each string is holding (index into `notes`).
    let mut holder: [Option<usize>; 6] = [None; 6];
    let mut placed: Vec<Option<usize>> = vec![None; notes.len()];
    for (_, on, k, _) in events {
        let pitch = notes[k].1;
        if on {
            let i = guitar_string_for(pitch, open, low, busy);
            busy[i] = Some(pitch);
            holder[i] = Some(k);
            placed[k] = Some(i);
        } else if let Some(i) = (0..6).find(|&i| holder[i] == Some(k)) {
            busy[i] = None;
            holder[i] = None;
        }
    }
    notes
        .iter()
        .zip(placed)
        .filter_map(|(&(start_beats, pitch), i)| {
            let i = i?;
            let fret = pitch.checked_sub(open[i]).filter(|&f| f <= reach)?;
            Some(TabNote {
                start_beats,
                pitch,
                string: 6 - i as u8,
                fret,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_model::{Command, InstrumentKind, NoteInput, Project};

    fn guitar(notes: &[(u8, f64, f64)], tuning: f64, capo: f64) -> Track {
        let mut p = Project::default();
        Command::AddTrack {
            name: "Guitar".into(),
            instrument: InstrumentKind::Guitar,
            preset: None,
            index: None,
        }
        .apply(&mut p)
        .expect("guitar");
        let track_id = p.tracks.last().expect("track").id;
        for (id, v) in [("string.tuning", tuning), ("string.capo", capo)] {
            Command::SetInstrumentParam {
                track_id,
                param: id.into(),
                value: v,
            }
            .apply(&mut p)
            .expect("param");
        }
        Command::CreateClip {
            track_id,
            start_beats: 0.0,
            length_beats: 16.0,
            name: None,
            notes: notes
                .iter()
                .map(|&(pitch, start_beats, length_beats)| NoteInput {
                    chance: 100,
                    pitch,
                    start_beats,
                    length_beats,
                    velocity: 100,
                    id: None,
                })
                .collect(),
        }
        .apply(&mut p)
        .expect("clip");
        p.tracks.last().expect("track").clone()
    }

    fn frets(t: &Track) -> Vec<(u8, u8, u8)> {
        guitar_frets(t)
            .iter()
            .map(|n| (n.pitch, n.string, n.fret))
            .collect()
    }

    #[test]
    fn an_open_e_chord_lands_one_note_per_string() {
        let chord: Vec<_> = [40, 47, 52, 56, 59, 64]
            .iter()
            .map(|&p| (p, 0.0, 2.0))
            .collect();
        let mut got = frets(&guitar(&chord, 0.0, 0.0));
        got.sort_by_key(|n| n.1);
        assert_eq!(
            got,
            [
                (64, 1, 0),
                (59, 2, 0),
                (56, 3, 1),
                (52, 4, 2),
                (47, 5, 2),
                (40, 6, 0)
            ]
        );
    }

    #[test]
    fn single_notes_take_the_highest_string_that_reaches() {
        // A2 alone: the open A string, not the 5th fret of low E.
        assert_eq!(frets(&guitar(&[(45, 0.0, 1.0)], 0.0, 0.0)), [(45, 5, 0)]);
        // A held note keeps its string; the next goes to the next one down.
        assert_eq!(
            frets(&guitar(&[(64, 0.0, 2.0), (66, 1.0, 1.0)], 0.0, 0.0)),
            [(64, 1, 0), (66, 2, 7)]
        );
        // Once let go, the high string is free again.
        assert_eq!(
            frets(&guitar(&[(64, 0.0, 1.0), (66, 1.0, 1.0)], 0.0, 0.0)),
            [(64, 1, 0), (66, 1, 2)]
        );
    }

    #[test]
    fn frets_follow_the_tuning_and_count_from_the_capo() {
        // Drop D: the low D is the open sixth string.
        assert_eq!(frets(&guitar(&[(38, 0.0, 1.0)], 1.0, 0.0)), [(38, 6, 0)]);
        // Capo 2: F#4 on the high string is "0" (the capo).
        assert_eq!(frets(&guitar(&[(66, 0.0, 1.0)], 0.0, 2.0)), [(66, 1, 0)]);
    }

    #[test]
    fn notes_the_guitar_cannot_reach_are_left_out() {
        assert_eq!(
            frets(&guitar(&[(30, 0.0, 1.0), (100, 1.0, 1.0)], 0.0, 0.0)),
            []
        );
    }
}
