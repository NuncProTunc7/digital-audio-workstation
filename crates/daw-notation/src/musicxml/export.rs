//! Writing a project as a MusicXML score.
//!
//! Notes are snapped to a sixteenth-note grid. Each track becomes one part
//! on one staff with a single voice: notes that start together form a
//! chord, and a note is cut where the next one starts. That keeps the score
//! readable; the exact timing stays in the project and in MIDI exports.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use daw_model::{InstrumentKind, Project, TimeSignature, Track, TrackId};

use super::{DIVISIONS, drum_position};
use crate::{played_notes, quarters_per_beat};

/// Written note values in divisions (sixteenths), longest first, with
/// their MusicXML type and whether they are dotted.
const VALUES: &[(i64, &str, bool)] = &[
    (24, "whole", true),
    (16, "whole", false),
    (12, "half", true),
    (8, "half", false),
    (6, "quarter", true),
    (4, "quarter", false),
    (3, "eighth", true),
    (2, "eighth", false),
    (1, "16th", false),
];

const SHARP_NAMES: [(char, i32); 12] = [
    ('C', 0),
    ('C', 1),
    ('D', 0),
    ('D', 1),
    ('E', 0),
    ('F', 0),
    ('F', 1),
    ('G', 0),
    ('G', 1),
    ('A', 0),
    ('A', 1),
    ('B', 0),
];

/// A run of the single voice: a chord (or a rest when `pitches` is empty).
#[derive(Debug, Clone, PartialEq)]
struct Event {
    start: i64,
    duration: i64,
    pitches: Vec<u8>,
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn measure_divisions(ts: TimeSignature) -> i64 {
    // numerator beats of 1/denominator each, in sixteenths.
    (i64::from(ts.numerator.max(1)) * 16 / i64::from(ts.denominator.max(1))).max(1)
}

/// The track's notes as non-overlapping chords and rests, from 0 to `end`.
fn voice(track: &Track, ts: TimeSignature, end: i64) -> Vec<Event> {
    let to_div = |beats: f64| (beats * quarters_per_beat(ts) * DIVISIONS as f64).round() as i64;
    // Onset -> (pitches, shortest end).
    let mut chords: BTreeMap<i64, (Vec<u8>, i64)> = BTreeMap::new();
    // Sheet music shows notes as written; swing is a playing style.
    for (s, e, pitch, _) in played_notes(track, false) {
        let start = to_div(s).max(0);
        if start >= end {
            continue;
        }
        let stop = to_div(e).max(start + 1);
        let entry = chords.entry(start).or_insert((Vec::new(), stop));
        if !entry.0.contains(&pitch) {
            entry.0.push(pitch);
        }
        entry.1 = entry.1.min(stop);
    }
    let onsets: Vec<i64> = chords.keys().copied().collect();
    let mut events = Vec::new();
    let mut at = 0;
    for (i, &start) in onsets.iter().enumerate() {
        let (pitches, stop) = &chords[&start];
        if start > at {
            events.push(Event {
                start: at,
                duration: start - at,
                pitches: Vec::new(),
            });
        }
        let next = onsets.get(i + 1).copied().unwrap_or(end);
        let stop = (*stop).min(next).min(end);
        let mut pitches = pitches.clone();
        pitches.sort_unstable();
        events.push(Event {
            start,
            duration: stop - start,
            pitches,
        });
        at = stop;
    }
    if at < end {
        events.push(Event {
            start: at,
            duration: end - at,
            pitches: Vec::new(),
        });
    }
    events
}

/// Splits a duration into written values, greedily.
fn pieces(mut duration: i64) -> Vec<(i64, &'static str, bool)> {
    let mut out = Vec::new();
    while duration > 0 {
        let v = VALUES
            .iter()
            .find(|(d, ..)| *d <= duration)
            .copied()
            .unwrap_or((1, "16th", false));
        out.push(v);
        duration -= v.0;
    }
    out
}

struct PartWriter<'a> {
    xml: &'a mut String,
    drums: bool,
}

impl PartWriter<'_> {
    /// One written note (or rest) of `duration`, with ties as given.
    fn note(&mut self, pitches: &[u8], value: (i64, &str, bool), tie_stop: bool, tie_start: bool) {
        if pitches.is_empty() {
            let _ = write!(
                self.xml,
                "<note><rest/><duration>{}</duration><voice>1</voice><type>{}</type>{}</note>",
                value.0,
                value.1,
                if value.2 { "<dot/>" } else { "" }
            );
            return;
        }
        for (i, &p) in pitches.iter().enumerate() {
            let mut n = String::from("<note>");
            if i > 0 {
                n.push_str("<chord/>");
            }
            let head;
            if self.drums {
                let (step, octave, x) = drum_position(p);
                head = x;
                let _ = write!(
                    n,
                    "<unpitched><display-step>{step}</display-step><display-octave>{octave}</display-octave></unpitched>"
                );
            } else {
                head = false;
                let (step, alter) = SHARP_NAMES[usize::from(p % 12)];
                let octave = i32::from(p / 12) - 1;
                let _ = write!(n, "<pitch><step>{step}</step>");
                if alter != 0 {
                    let _ = write!(n, "<alter>{alter}</alter>");
                }
                let _ = write!(n, "<octave>{octave}</octave></pitch>");
            }
            let _ = write!(n, "<duration>{}</duration>", value.0);
            if tie_stop {
                n.push_str("<tie type=\"stop\"/>");
            }
            if tie_start {
                n.push_str("<tie type=\"start\"/>");
            }
            let _ = write!(n, "<voice>1</voice><type>{}</type>", value.1);
            if value.2 {
                n.push_str("<dot/>");
            }
            if self.drums {
                n.push_str("<stem>up</stem>");
            }
            if head {
                n.push_str("<notehead>x</notehead>");
            }
            if tie_stop || tie_start {
                n.push_str("<notations>");
                if tie_stop {
                    n.push_str("<tied type=\"stop\"/>");
                }
                if tie_start {
                    n.push_str("<tied type=\"start\"/>");
                }
                n.push_str("</notations>");
            }
            n.push_str("</note>");
            self.xml.push_str(&n);
        }
    }
}

/// The project (or the given tracks) as a MusicXML 4.0 partwise score.
/// Audio tracks are left out.
pub fn export_musicxml(project: &Project, track_ids: Option<&[TrackId]>) -> String {
    let ts = project.time_signature;
    let measure = measure_divisions(ts);
    let tracks: Vec<&Track> = project
        .tracks
        .iter()
        .filter(|t| !t.instrument.kind.is_audio())
        .filter(|t| track_ids.is_none_or(|ids| ids.contains(&t.id)))
        .collect();
    let end_beats = tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.start_beats + c.length_beats))
        .fold(0.0, f64::max);
    let end_div = (end_beats * quarters_per_beat(ts) * DIVISIONS as f64).round() as i64;
    let measures = ((end_div + measure - 1) / measure).max(1);
    let end = measures * measure;
    let quarter_bpm = project.tempo_bpm * quarters_per_beat(ts);

    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n");
    xml.push_str("<!DOCTYPE score-partwise PUBLIC \"-//Recordare//DTD MusicXML 4.0 Partwise//EN\" \"http://www.musicxml.org/dtds/partwise.dtd\">\n");
    xml.push_str("<score-partwise version=\"4.0\">\n");
    let _ = writeln!(
        xml,
        "<work><work-title>{}</work-title></work>",
        escape(&project.name)
    );
    xml.push_str("<identification><encoding><software>Nunc Pro Tune</software></encoding></identification>\n<part-list>\n");
    for (i, t) in tracks.iter().enumerate() {
        let _ = writeln!(
            xml,
            "<score-part id=\"P{}\"><part-name>{}</part-name></score-part>",
            i + 1,
            escape(&t.name)
        );
    }
    xml.push_str("</part-list>\n");

    for (i, t) in tracks.iter().enumerate() {
        let drums = t.instrument.kind == InstrumentKind::Drums;
        let events = voice(t, ts, end);
        let mean_pitch = {
            let p: Vec<f64> = events
                .iter()
                .flat_map(|e| e.pitches.iter().map(|&p| f64::from(p)))
                .collect();
            if p.is_empty() {
                60.0
            } else {
                p.iter().sum::<f64>() / p.len() as f64
            }
        };
        let clef = if drums {
            "<clef><sign>percussion</sign></clef>"
        } else if mean_pitch < 57.0 {
            "<clef><sign>F</sign><line>4</line></clef>"
        } else {
            "<clef><sign>G</sign><line>2</line></clef>"
        };
        let _ = writeln!(xml, "<part id=\"P{}\">", i + 1);
        let mut w = PartWriter {
            xml: &mut xml,
            drums,
        };
        let mut next_event = 0;
        // Part of the current event still to write, when it crosses a barline.
        let mut carry: Option<(Vec<u8>, i64)> = None;
        for m in 0..measures {
            let _ = write!(w.xml, "<measure number=\"{}\">", m + 1);
            if m == 0 {
                let _ = write!(
                    w.xml,
                    "<attributes><divisions>{DIVISIONS}</divisions><key><fifths>0</fifths></key><time><beats>{}</beats><beat-type>{}</beat-type></time>{clef}</attributes>",
                    ts.numerator, ts.denominator
                );
                if i == 0 {
                    let _ = write!(
                        w.xml,
                        "<direction placement=\"above\"><direction-type><metronome><beat-unit>quarter</beat-unit><per-minute>{}</per-minute></metronome></direction-type><sound tempo=\"{}\"/></direction>",
                        quarter_bpm.round(),
                        quarter_bpm
                    );
                }
            }
            let bar_end = (m + 1) * measure;
            let mut at = m * measure;
            while at < bar_end {
                let (pitches, remaining, continued) = match carry.take() {
                    Some((p, r)) => (p, r, true),
                    None => {
                        let Some(e) = events.get(next_event) else {
                            break;
                        };
                        next_event += 1;
                        (e.pitches.clone(), e.duration, false)
                    }
                };
                // A bar of silence is one whole-measure rest.
                if pitches.is_empty() && at == m * measure && remaining >= measure {
                    let _ = write!(
                        w.xml,
                        "<note><rest measure=\"yes\"/><duration>{measure}</duration><voice>1</voice></note>"
                    );
                    if remaining > measure {
                        carry = Some((pitches, remaining - measure));
                    }
                    at = bar_end;
                    continue;
                }
                let here = remaining.min(bar_end - at);
                let rest_of_event = remaining - here;
                let parts = pieces(here);
                for (k, value) in parts.iter().enumerate() {
                    let tie_stop = !pitches.is_empty() && (k > 0 || continued);
                    let tie_start =
                        !pitches.is_empty() && (k + 1 < parts.len() || rest_of_event > 0);
                    w.note(&pitches, *value, tie_stop, tie_start);
                }
                at += here;
                if rest_of_event > 0 {
                    carry = Some((pitches, rest_of_event));
                }
            }
            w.xml.push_str("</measure>\n");
        }
        xml.push_str("</part>\n");
    }
    xml.push_str("</score-partwise>\n");
    xml
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_model::{Command, NoteInput};

    fn note(pitch: u8, start: f64, len: f64) -> NoteInput {
        NoteInput {
            chance: 100,
            pitch,
            start_beats: start,
            length_beats: len,
            velocity: 100,
            id: None,
        }
    }

    fn project_with(track: u32, notes: Vec<NoteInput>, length: f64) -> Project {
        let mut p = Project::default();
        Command::CreateClip {
            track_id: track,
            start_beats: 0.0,
            length_beats: length,
            name: None,
            notes,
        }
        .apply(&mut p)
        .expect("clip");
        p
    }

    /// Sum of durations per measure, ignoring chord notes.
    fn measure_lengths(xml: &str) -> Vec<i64> {
        let doc = super::super::parse_xml(xml).expect("well-formed XML");
        doc.descendants()
            .filter(|n| n.has_tag_name("measure"))
            .map(|m| {
                m.children()
                    .filter(|n| {
                        n.has_tag_name("note") && !n.children().any(|c| c.has_tag_name("chord"))
                    })
                    .filter_map(|n| n.children().find(|c| c.has_tag_name("duration")))
                    .filter_map(|d| d.text()?.parse::<i64>().ok())
                    .sum()
            })
            .collect()
    }

    #[test]
    fn every_measure_is_full_and_well_formed() {
        let p = project_with(
            1,
            vec![
                note(60, 0.0, 1.0),
                note(64, 0.0, 1.0),
                note(67, 1.5, 3.0),
                note(72, 6.25, 0.5),
            ],
            8.0,
        );
        let xml = export_musicxml(&p, None);
        // Only Keys has notes, but every instrument track gets a part.
        assert_eq!(xml.matches("<part id=").count(), 3);
        let lengths = measure_lengths(&xml);
        assert!(lengths.iter().all(|&l| l == 16), "{lengths:?}");
    }

    #[test]
    fn chords_ties_and_pitch_spelling() {
        let p = project_with(
            1,
            vec![note(60, 0.0, 1.0), note(64, 0.0, 1.0), note(61, 3.0, 2.0)],
            8.0,
        );
        let xml = export_musicxml(&p, Some(&[1]));
        assert_eq!(xml.matches("<part id=").count(), 1);
        // E4 is written as a chord tone on C4.
        assert!(xml.contains("<note><chord/><pitch><step>E</step><octave>4</octave></pitch>"));
        // C#4 crosses the barline: tied quarter + quarter.
        assert!(xml.contains("<step>C</step><alter>1</alter><octave>4</octave></pitch><duration>4</duration><tie type=\"start\"/>"));
        assert!(xml.contains("<tie type=\"stop\"/>"));
        assert!(xml.contains("<sound tempo=\"120\"/>"));
        assert!(xml.contains("<clef><sign>G</sign><line>2</line></clef>"));
    }

    #[test]
    fn bass_and_drums_get_their_clefs() {
        let mut p = project_with(2, vec![note(36, 0.0, 2.0)], 4.0);
        Command::CreateClip {
            track_id: 3,
            start_beats: 0.0,
            length_beats: 4.0,
            name: None,
            notes: vec![
                note(36, 0.0, 0.25),
                note(42, 0.0, 0.25),
                note(38, 1.0, 0.25),
            ],
        }
        .apply(&mut p)
        .expect("drums");
        let xml = export_musicxml(&p, Some(&[2, 3]));
        assert!(xml.contains("<clef><sign>F</sign><line>4</line></clef>"));
        assert!(xml.contains("<clef><sign>percussion</sign></clef>"));
        assert!(xml.contains("<display-step>G</display-step><display-octave>5</display-octave>"));
        assert!(xml.contains("<notehead>x</notehead>"));
        assert!(measure_lengths(&xml).iter().all(|&l| l == 16));
    }

    #[test]
    fn off_grid_notes_snap_to_sixteenths_and_compound_meter_fills_bars() {
        let mut p = project_with(1, vec![note(60, 0.13, 0.4), note(62, 2.9, 0.3)], 8.0);
        Command::SetTimeSignature {
            numerator: 6,
            denominator: 8,
        }
        .apply(&mut p)
        .expect("6/8");
        let xml = export_musicxml(&p, Some(&[1]));
        // 6/8 = 12 sixteenths per bar.
        let lengths = measure_lengths(&xml);
        assert!(lengths.iter().all(|&l| l == 12), "{lengths:?}");
        assert!(xml.contains("<beats>6</beats><beat-type>8</beat-type>"));
    }

    #[test]
    fn names_are_escaped() {
        let mut p = Project::default();
        p.tracks[0].name = "Lead <A&B>".into();
        let xml = export_musicxml(&p, Some(&[1]));
        assert!(xml.contains("Lead &lt;A&amp;B&gt;"));
        super::super::parse_xml(&xml).expect("parses");
    }
}
