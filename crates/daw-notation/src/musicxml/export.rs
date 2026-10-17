//! Writing a project as a MusicXML score.
//!
//! Notes are snapped to a sixteenth-note grid. Each track becomes one part
//! on one staff with a single voice: notes that start together form a
//! chord, and a note is cut where the next one starts. That keeps the score
//! readable; the exact timing stays in the project and in MIDI exports.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use daw_model::music::{Chord, ChordQuality, NOTE_NAMES};
use daw_model::{InstrumentKind, Project, TimeSignature, Track, TrackId};

use super::{DIVISIONS, drum_position};
use crate::guitar::{Connection, TabNote};
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

/// How guitar tracks are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GuitarView {
    /// A notes staff with a tab staff under it (the usual guitar score).
    #[default]
    Both,
    /// The tab staff only.
    Tab,
    /// The notes staff only.
    Notes,
}

/// One written note, chord or rest in a bar, before it becomes XML.
#[derive(Debug, Clone)]
struct Written {
    /// Where its event started (the key for frets), in divisions.
    onset: i64,
    /// Where this piece starts, in divisions from the song start.
    at: i64,
    pitches: Vec<u8>,
    value: (i64, &'static str, bool),
    tie_stop: bool,
    tie_start: bool,
    /// A whole bar's rest.
    measure_rest: bool,
}

/// Splits the voice into bars of written notes (tied across barlines and
/// into note values that can be written).
fn layout(events: &[Event], measures: i64, measure: i64) -> Vec<Vec<Written>> {
    let mut bars = Vec::new();
    let mut next_event = 0;
    // Part of the current event still to write: (pitches, onset, remaining).
    let mut carry: Option<(Vec<u8>, i64, i64)> = None;
    for m in 0..measures {
        let mut bar = Vec::new();
        let bar_end = (m + 1) * measure;
        let mut at = m * measure;
        while at < bar_end {
            let (pitches, onset, remaining, continued) = match carry.take() {
                Some((p, o, r)) => (p, o, r, true),
                None => {
                    let Some(e) = events.get(next_event) else {
                        break;
                    };
                    next_event += 1;
                    (e.pitches.clone(), e.start, e.duration, false)
                }
            };
            // A bar of silence is one whole-measure rest.
            if pitches.is_empty() && at == m * measure && remaining >= measure {
                bar.push(Written {
                    onset,
                    at,
                    pitches: Vec::new(),
                    value: (measure, "", false),
                    tie_stop: false,
                    tie_start: false,
                    measure_rest: true,
                });
                if remaining > measure {
                    carry = Some((pitches, onset, remaining - measure));
                }
                at = bar_end;
                continue;
            }
            let here = remaining.min(bar_end - at);
            let rest_of_event = remaining - here;
            let parts = pieces(here);
            for (k, value) in parts.iter().enumerate() {
                bar.push(Written {
                    onset,
                    at,
                    pitches: pitches.clone(),
                    value: *value,
                    tie_stop: !pitches.is_empty() && (k > 0 || continued),
                    tie_start: !pitches.is_empty() && (k + 1 < parts.len() || rest_of_event > 0),
                    measure_rest: false,
                });
                at += value.0;
            }
            if rest_of_event > 0 {
                carry = Some((pitches, onset, rest_of_event));
            }
        }
        bars.push(bar);
    }
    bars
}

/// What a staff shows.
#[derive(Clone, Copy)]
enum StaffKind<'a> {
    Notes {
        drums: bool,
    },
    /// Fret numbers, from (onset, pitch) to (string, fret).
    Tab(&'a HashMap<(i64, u8), TabNote>),
}

/// Writes one staff's notes for a bar. `staff` is set when the part has two
/// staves; `voice` keeps the staves' voices apart. Chord names starting
/// during a note are written just before it.
fn write_staff(
    xml: &mut String,
    bar: &[Written],
    kind: StaffKind<'_>,
    staff: Option<u8>,
    voice: u8,
    chords: &[(i64, &Chord)],
) {
    let staff_tag = staff.map_or(String::new(), |s| format!("<staff>{s}</staff>"));
    let mut chords = chords.iter().peekable();
    for w in bar {
        while let Some((div, chord)) = chords.next_if(|(div, _)| *div < w.at + w.value.0) {
            write_harmony(xml, chord, div - w.at);
        }
        if w.measure_rest {
            let _ = write!(
                xml,
                "<note><rest measure=\"yes\"/><duration>{}</duration><voice>{voice}</voice>{staff_tag}</note>",
                w.value.0
            );
            continue;
        }
        let pitches: Vec<(u8, Option<TabNote>)> = match kind {
            StaffKind::Notes { .. } => w.pitches.iter().map(|&p| (p, None)).collect(),
            // The tab leaves out notes the guitar can't reach.
            StaffKind::Tab(frets) => w
                .pitches
                .iter()
                .filter_map(|&p| frets.get(&(w.onset, p)).map(|&t| (p, Some(t))))
                .collect(),
        };
        let dot = if w.value.2 { "<dot/>" } else { "" };
        if pitches.is_empty() {
            let _ = write!(
                xml,
                "<note><rest/><duration>{}</duration><voice>{voice}</voice><type>{}</type>{dot}{staff_tag}</note>",
                w.value.0, w.value.1
            );
            continue;
        }
        let drums = matches!(kind, StaffKind::Notes { drums: true });
        for (i, &(p, fret)) in pitches.iter().enumerate() {
            let mut n = String::from("<note>");
            if i > 0 {
                n.push_str("<chord/>");
            }
            let mut head = false;
            if drums {
                let (step, octave, x) = drum_position(p);
                head = x;
                let _ = write!(
                    n,
                    "<unpitched><display-step>{step}</display-step><display-octave>{octave}</display-octave></unpitched>"
                );
            } else {
                let (step, alter) = SHARP_NAMES[usize::from(p % 12)];
                let octave = i32::from(p / 12) - 1;
                let _ = write!(n, "<pitch><step>{step}</step>");
                if alter != 0 {
                    let _ = write!(n, "<alter>{alter}</alter>");
                }
                let _ = write!(n, "<octave>{octave}</octave></pitch>");
            }
            let _ = write!(n, "<duration>{}</duration>", w.value.0);
            if w.tie_stop {
                n.push_str("<tie type=\"stop\"/>");
            }
            if w.tie_start {
                n.push_str("<tie type=\"start\"/>");
            }
            let _ = write!(n, "<voice>{voice}</voice><type>{}</type>{dot}", w.value.1);
            if drums {
                n.push_str("<stem>up</stem>");
            }
            n.push_str(&staff_tag);
            if head {
                n.push_str("<notehead>x</notehead>");
            }
            if w.tie_stop || w.tie_start || fret.is_some() {
                n.push_str("<notations>");
                if w.tie_stop {
                    n.push_str("<tied type=\"stop\"/>");
                }
                if w.tie_start {
                    n.push_str("<tied type=\"start\"/>");
                }
                if let Some(t) = fret {
                    write_tab_marks(&mut n, &t, !w.tie_stop, !w.tie_start);
                }
                n.push_str("</notations>");
            }
            n.push_str("</note>");
            xml.push_str(&n);
        }
    }
}

/// A tab note's string and fret, and its techniques: a bend and the start
/// or end of a slide, hammer-on or pull-off. A note split across a barline
/// marks the first piece with what it receives and the last with what it
/// passes on.
fn write_tab_marks(n: &mut String, t: &TabNote, first: bool, last: bool) {
    let slide = |c: Option<Connection>| c == Some(Connection::Slide);
    if first && slide(t.from_previous) {
        n.push_str("<slide number=\"1\" type=\"stop\"/>");
    }
    if last && slide(t.into_next) {
        n.push_str("<slide number=\"1\" type=\"start\"/>");
    }
    let _ = write!(
        n,
        "<technical><string>{}</string><fret>{}</fret>",
        t.string, t.fret
    );
    let legato = |c: Connection, kind: &str| match c {
        Connection::HammerOn => format!("<hammer-on number=\"1\" type=\"{kind}\">H</hammer-on>"),
        Connection::PullOff => format!("<pull-off number=\"1\" type=\"{kind}\">P</pull-off>"),
        Connection::Slide => String::new(),
    };
    if first && let Some(c) = t.from_previous {
        n.push_str(&legato(c, "stop"));
    }
    if last && let Some(c) = t.into_next {
        n.push_str(&legato(c, "start"));
    }
    if first && let Some(b) = t.bend {
        let _ = write!(n, "<bend><bend-alter>{b}</bend-alter></bend>");
    }
    n.push_str("</technical>");
}

/// A chord name above the staff, `offset` divisions after the next note.
fn write_harmony(xml: &mut String, chord: &Chord, offset: i64) {
    let spell = |pc: u8| {
        let name = NOTE_NAMES[usize::from(pc % 12)];
        let mut chars = name.chars();
        let step = chars.next().unwrap_or('C');
        let alter = match chars.next() {
            Some('#') => 1,
            Some('b') => -1,
            _ => 0,
        };
        (step, alter)
    };
    let kind = match chord.quality {
        ChordQuality::Major => "major",
        ChordQuality::Minor => "minor",
        ChordQuality::Diminished => "diminished",
        ChordQuality::Augmented => "augmented",
        ChordQuality::Sus2 => "suspended-second",
        ChordQuality::Sus4 => "suspended-fourth",
        ChordQuality::Major7 => "major-seventh",
        ChordQuality::Minor7 => "minor-seventh",
        ChordQuality::Dominant7 => "dominant",
        ChordQuality::HalfDiminished7 => "half-diminished",
        ChordQuality::Power => "power",
    };
    let (step, alter) = spell(chord.root);
    let _ = write!(xml, "<harmony><root><root-step>{step}</root-step>");
    if alter != 0 {
        let _ = write!(xml, "<root-alter>{alter}</root-alter>");
    }
    let _ = write!(
        xml,
        "</root><kind text=\"{}\">{kind}</kind>",
        chord.quality.symbol()
    );
    if let Some(b) = chord.bass {
        let (step, alter) = spell(b);
        let _ = write!(xml, "<bass><bass-step>{step}</bass-step>");
        if alter != 0 {
            let _ = write!(xml, "<bass-alter>{alter}</bass-alter>");
        }
        xml.push_str("</bass>");
    }
    if offset > 0 {
        let _ = write!(xml, "<offset>{offset}</offset>");
    }
    xml.push_str("</harmony>");
}

/// The tab staff's six lines in the guitar's tuning (lowest first, without
/// the capo), and the capo.
fn tab_details(track: &Track, number: Option<u8>) -> String {
    let (open, low) = crate::guitar::open_strings(track);
    let capo = open[0].saturating_sub(low);
    let number = number.map_or(String::new(), |n| format!(" number=\"{n}\""));
    let mut s = format!("<staff-details{number}><staff-lines>6</staff-lines>");
    for (line, &note) in open.iter().enumerate() {
        let note = note - capo;
        let (step, alter) = SHARP_NAMES[usize::from(note % 12)];
        let _ = write!(
            s,
            "<staff-tuning line=\"{}\"><tuning-step>{step}</tuning-step>",
            line + 1
        );
        if alter != 0 {
            let _ = write!(s, "<tuning-alter>{alter}</tuning-alter>");
        }
        let _ = write!(
            s,
            "<tuning-octave>{}</tuning-octave></staff-tuning>",
            i32::from(note / 12) - 1
        );
    }
    if capo > 0 {
        let _ = write!(s, "<capo>{capo}</capo>");
    }
    s.push_str("</staff-details>");
    s
}

/// Guitar music is written an octave above where it sounds: a treble clef
/// with a small 8 below.
const GUITAR_CLEF: &str = "<sign>G</sign><line>2</line><clef-octave-change>-1</clef-octave-change>";

/// The project (or the given tracks) as a MusicXML 4.0 partwise score.
/// Audio tracks are left out; guitar tracks get a notes staff and a tab
/// staff.
pub fn export_musicxml(project: &Project, track_ids: Option<&[TrackId]>) -> String {
    export_musicxml_with(project, track_ids, GuitarView::Both)
}

/// [`export_musicxml`], choosing how guitar tracks are written. Chord
/// names from the chord track go above the top part.
pub fn export_musicxml_with(
    project: &Project,
    track_ids: Option<&[TrackId]>,
    guitar: GuitarView,
) -> String {
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
    let qpb = quarters_per_beat(ts);
    let to_div = |beats: f64| (beats * qpb * DIVISIONS as f64).round() as i64;
    let measures = ((to_div(end_beats) + measure - 1) / measure).max(1);
    let end = measures * measure;
    // Tempo marks as (division from the start, quarter-note tempo, words):
    // the start, then each change, with "accel." or "rit." where it glides.
    let mut tempo_marks: Vec<(i64, f64, &str)> = vec![(0, project.tempo_bpm * qpb, "")];
    for (k, c) in project.tempo_changes.iter().enumerate() {
        let words = match project.tempo_changes.get(k + 1) {
            Some(next) if c.ramp && next.bpm > c.bpm => "accel.",
            Some(next) if c.ramp && next.bpm < c.bpm => "rit.",
            _ => "",
        };
        tempo_marks.push((to_div(c.beats), c.bpm * qpb, words));
    }
    let mut chords: Vec<(i64, &Chord)> = project
        .chords
        .iter()
        .map(|c| (to_div(c.start_beats).max(0), c))
        .filter(|(div, _)| *div < end)
        .collect();
    chords.sort_by_key(|(div, _)| *div);

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
        let is_guitar = t.instrument.kind == InstrumentKind::Guitar;
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
        let frets: HashMap<(i64, u8), TabNote> = if is_guitar && guitar != GuitarView::Notes {
            crate::guitar::guitar_frets(t)
                .into_iter()
                .map(|n| ((to_div(n.start_beats).max(0), n.pitch), n))
                .collect()
        } else {
            HashMap::new()
        };
        // The staves: what each shows, and the attributes that set them up.
        let notes = StaffKind::Notes { drums };
        let tab = StaffKind::Tab(&frets);
        let (staves, clefs): (Vec<StaffKind<'_>>, String) = if is_guitar {
            match guitar {
                GuitarView::Both => (
                    vec![notes, tab],
                    format!(
                        "<staves>2</staves><clef number=\"1\">{GUITAR_CLEF}</clef><clef number=\"2\"><sign>TAB</sign><line>5</line></clef>{}",
                        tab_details(t, Some(2))
                    ),
                ),
                GuitarView::Tab => (
                    vec![tab],
                    format!(
                        "<clef><sign>TAB</sign><line>5</line></clef>{}",
                        tab_details(t, None)
                    ),
                ),
                GuitarView::Notes => (vec![notes], format!("<clef>{GUITAR_CLEF}</clef>")),
            }
        } else if drums {
            (vec![notes], "<clef><sign>percussion</sign></clef>".into())
        } else if mean_pitch < 57.0 {
            (
                vec![notes],
                "<clef><sign>F</sign><line>4</line></clef>".into(),
            )
        } else {
            (
                vec![notes],
                "<clef><sign>G</sign><line>2</line></clef>".into(),
            )
        };
        let bars = layout(&events, measures, measure);
        let _ = writeln!(xml, "<part id=\"P{}\">", i + 1);
        for (m, bar) in (0_i64..).zip(&bars) {
            let _ = write!(xml, "<measure number=\"{}\">", m + 1);
            if m == 0 {
                let _ = write!(
                    xml,
                    "<attributes><divisions>{DIVISIONS}</divisions><key><fifths>{}</fifths><mode>{}</mode></key><time><beats>{}</beats><beat-type>{}</beat-type></time>{clefs}</attributes>",
                    project.key.map_or(0, |k| k.fifths()),
                    if project.key.is_some_and(|k| k.mode.is_minor()) {
                        "minor"
                    } else {
                        "major"
                    },
                    ts.numerator,
                    ts.denominator
                );
            }
            let (bar_start, bar_end) = (m * measure, (m + 1) * measure);
            if i == 0 {
                // Tempo marks in this bar, placed from its start.
                for &(div, quarter_bpm, words) in tempo_marks
                    .iter()
                    .filter(|t| t.0 >= bar_start && t.0 < bar_end)
                {
                    let words = if words.is_empty() {
                        String::new()
                    } else {
                        format!("<direction-type><words>{words}</words></direction-type>")
                    };
                    let offset = div - bar_start;
                    let offset = if offset > 0 {
                        format!("<offset>{offset}</offset>")
                    } else {
                        String::new()
                    };
                    let _ = write!(
                        xml,
                        "<direction placement=\"above\"><direction-type><metronome><beat-unit>quarter</beat-unit><per-minute>{}</per-minute></metronome></direction-type>{words}{offset}<sound tempo=\"{}\"/></direction>",
                        quarter_bpm.round(),
                        quarter_bpm
                    );
                }
            }
            let two = staves.len() > 1;
            for (k, &kind) in (0_u8..).zip(&staves) {
                if k > 0 {
                    let _ = write!(xml, "<backup><duration>{measure}</duration></backup>");
                }
                // Chord names go above the top part's first staff.
                let bar_chords: Vec<(i64, &Chord)> = if i == 0 && k == 0 {
                    chords
                        .iter()
                        .copied()
                        .filter(|(div, _)| *div >= bar_start && *div < bar_end)
                        .collect()
                } else {
                    Vec::new()
                };
                write_staff(
                    &mut xml,
                    bar,
                    kind,
                    two.then_some(k + 1),
                    if k == 0 { 1 } else { 5 },
                    &bar_chords,
                );
            }
            xml.push_str("</measure>\n");
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
            guitar: None,
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
    fn tempo_changes_are_marked_where_they_happen_and_read_back() {
        let mut p = project_with(1, vec![note(60, 0.0, 1.0)], 16.0);
        // Mid-bar 2 (beat 6), a glide up to 150 at bar 4.
        for (beats, bpm, ramp) in [(6.0, 90.0, true), (12.0, 150.0, false)] {
            Command::AddTempoChange { beats, bpm, ramp }
                .apply(&mut p)
                .expect("tempo");
        }
        let xml = export_musicxml(&p, None);
        assert!(xml.contains("<sound tempo=\"120\"/>"));
        assert!(xml.contains("<words>accel.</words>"), "{xml}");
        assert!(xml.contains("<sound tempo=\"90\"/>"));
        assert!(xml.contains("<sound tempo=\"150\"/>"));
        assert!(xml.contains("<offset>"), "{xml}");
        assert!(
            measure_lengths(&xml)
                .iter()
                .all(|&l| l == measure_lengths(&xml)[0])
        );
        let back = crate::musicxml::import_musicxml(&xml).expect("read");
        assert_eq!(back.tempo_bpm, Some(120.0));
        assert_eq!(back.tempo_changes, vec![(6.0, 90.0), (12.0, 150.0)]);
    }

    #[test]
    fn the_key_signature_is_written() {
        let mut p = project_with(1, vec![note(62, 0.0, 1.0)], 4.0);
        Command::SetKey {
            key: Some(daw_model::music::Key {
                tonic: 2,
                mode: daw_model::music::Mode::Minor,
            }),
        }
        .apply(&mut p)
        .expect("key");
        let xml = export_musicxml(&p, None);
        assert!(
            xml.contains("<key><fifths>-1</fifths><mode>minor</mode></key>"),
            "{xml}"
        );
        let mid = crate::midi::export_midi(&p);
        // Key signature meta event: FF 59 02 sf mi (one flat, minor).
        assert!(mid.windows(5).any(|w| w == [0xFF, 0x59, 0x02, 0xFF, 0x01]));
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

    /// A song with one guitar track (id returned) playing `notes`.
    fn guitar_song(notes: Vec<NoteInput>, tuning: f64, capo: f64) -> (Project, u32) {
        let mut p = Project::default();
        Command::AddTrack {
            name: "Guitar".into(),
            instrument: InstrumentKind::Guitar,
            preset: None,
            index: None,
        }
        .apply(&mut p)
        .expect("guitar");
        let id = p.tracks.last().expect("track").id;
        for (param, v) in [("string.tuning", tuning), ("string.capo", capo)] {
            Command::SetInstrumentParam {
                track_id: id,
                param: param.into(),
                value: v,
            }
            .apply(&mut p)
            .expect("param");
        }
        Command::CreateClip {
            track_id: id,
            start_beats: 0.0,
            length_beats: 4.0,
            name: None,
            notes,
        }
        .apply(&mut p)
        .expect("clip");
        (p, id)
    }

    #[test]
    fn guitar_parts_get_a_notes_staff_and_a_tab_staff() {
        // An open E chord, then a high A (5th fret, high E string).
        let mut notes: Vec<NoteInput> = [40, 47, 52, 56, 59, 64]
            .iter()
            .map(|&p| note(p, 0.0, 2.0))
            .collect();
        notes.push(note(69, 2.0, 2.0));
        let (p, id) = guitar_song(notes, 0.0, 0.0);
        let xml = export_musicxml(&p, Some(&[id]));
        let doc = super::super::parse_xml(&xml).expect("well-formed");
        assert!(xml.contains("<staves>2</staves>"));
        assert!(xml.contains("<clef-octave-change>-1</clef-octave-change>"));
        assert!(xml.contains("<clef number=\"2\"><sign>TAB</sign><line>5</line></clef>"));
        assert!(xml.contains("<staff-lines>6</staff-lines>"));
        assert!(xml.contains(
            "<staff-tuning line=\"1\"><tuning-step>E</tuning-step><tuning-octave>2</tuning-octave></staff-tuning>"
        ));
        // Each staff fills its bar (the tab comes after a backup).
        let tab: Vec<(String, String)> = doc
            .descendants()
            .filter(|n| n.has_tag_name("technical"))
            .map(|t| {
                let text = |tag: &str| {
                    t.children()
                        .find(|c| c.has_tag_name(tag))
                        .and_then(|c| c.text())
                        .unwrap_or_default()
                        .to_owned()
                };
                (text("string"), text("fret"))
            })
            .collect();
        let mut chord: Vec<_> = tab[..6].to_vec();
        chord.sort();
        let pairs = |v: &[(&str, &str)]| -> Vec<(String, String)> {
            v.iter().map(|(a, b)| ((*a).into(), (*b).into())).collect()
        };
        assert_eq!(
            chord,
            pairs(&[
                ("1", "0"),
                ("2", "0"),
                ("3", "1"),
                ("4", "2"),
                ("5", "2"),
                ("6", "0")
            ])
        );
        assert_eq!(tab[6], ("1".into(), "5".into()));
        // Tab notes are on staff 2, in their own voice.
        let tab_notes = doc
            .descendants()
            .filter(|n| n.has_tag_name("note"))
            .filter(|n| n.descendants().any(|c| c.has_tag_name("fret")))
            .count();
        assert_eq!(tab_notes, 7);
        assert!(xml.contains("<voice>5</voice>"));
        assert!(xml.contains("<backup><duration>16</duration></backup>"));
    }

    #[test]
    fn the_tab_view_and_the_notes_view_have_one_staff() {
        let (p, id) = guitar_song(vec![note(64, 0.0, 1.0)], 0.0, 2.0);
        let tab = export_musicxml_with(&p, Some(&[id]), GuitarView::Tab);
        assert!(!tab.contains("<staves>"));
        assert!(tab.contains("<clef><sign>TAB</sign><line>5</line></clef>"));
        // Capo 2: the high string starts at F#4, so E4 goes on the B
        // string, 3 frets above the capo.
        assert!(tab.contains("<capo>2</capo>"));
        assert!(tab.contains("<string>2</string><fret>3</fret>"), "{tab}");
        let notes = export_musicxml_with(&p, Some(&[id]), GuitarView::Notes);
        assert!(!notes.contains("<fret>"));
        assert!(notes.contains(
            "<clef><sign>G</sign><line>2</line><clef-octave-change>-1</clef-octave-change></clef>"
        ));
    }

    #[test]
    fn chord_names_go_above_the_top_part() {
        let mut p = project_with(1, vec![note(60, 0.0, 4.0)], 4.0);
        for (beats, root, quality, bass) in [
            (0.0, 9, ChordQuality::Minor7, None),
            (2.0, 0, ChordQuality::Major, Some(4)),
        ] {
            Command::AddChord {
                start_beats: beats,
                root,
                quality,
                bass,
            }
            .apply(&mut p)
            .expect("chord");
        }
        let xml = export_musicxml(&p, None);
        assert!(xml.contains(
            "<harmony><root><root-step>A</root-step></root><kind text=\"m7\">minor-seventh</kind></harmony>"
        ));
        // C/E halfway through the held note.
        assert!(xml.contains(
            "<harmony><root><root-step>C</root-step></root><kind text=\"\">major</kind><bass><bass-step>E</bass-step></bass><offset>8</offset></harmony>"
        ));
        assert_eq!(xml.matches("<harmony>").count(), 2, "only on the top part");
        super::super::parse_xml(&xml).expect("parses");
    }

    #[test]
    fn guitar_techniques_show_in_the_tab() {
        let with = |pitch: u8, start: f64, f: fn(&mut daw_model::GuitarNote)| {
            let mut n = note(pitch, start, 1.0);
            let mut g = daw_model::GuitarNote::default();
            f(&mut g);
            n.guitar = Some(g);
            n
        };
        // A3 picked, hammer on to B3, slide up to D4, pull off to C#4... and
        // a separate E4 forced onto the B string with a whole-step bend.
        let notes = vec![
            note(57, 0.0, 1.0),
            with(59, 1.0, |g| g.legato = true),
            with(62, 2.0, |g| g.slide = true),
            with(61, 3.0, |g| g.legato = true),
            with(64, 4.0, |g| {
                g.string = Some(2);
                g.bend_semitones = Some(2.0);
            }),
        ];
        let (mut p, id) = guitar_song(notes, 0.0, 0.0);
        // Make the clip long enough for the last note.
        p.tracks.last_mut().expect("track").clips[0].length_beats = 8.0;
        let xml = export_musicxml_with(&p, Some(&[id]), GuitarView::Tab);
        super::super::parse_xml(&xml).expect("well-formed");
        // All on the G string (3): 2, 4, 7, 6.
        for fret in [2, 4, 7, 6] {
            assert!(
                xml.contains(&format!("<string>3</string><fret>{fret}</fret>")),
                "fret {fret}:
{xml}"
            );
        }
        assert!(xml.contains("<hammer-on number=\"1\" type=\"start\">H</hammer-on>"));
        assert!(xml.contains("<hammer-on number=\"1\" type=\"stop\">H</hammer-on>"));
        assert!(xml.contains("<slide number=\"1\" type=\"start\"/>"));
        assert!(xml.contains("<slide number=\"1\" type=\"stop\"/>"));
        assert!(xml.contains("<pull-off number=\"1\" type=\"stop\">P</pull-off>"));
        assert!(
            xml.contains("<string>2</string><fret>5</fret><bend><bend-alter>2</bend-alter></bend>")
        );
    }
}
