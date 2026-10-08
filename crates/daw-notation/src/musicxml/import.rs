//! Reading MusicXML scores (`.musicxml`, `.xml`, compressed `.mxl`).
//!
//! Handles partwise scores with any number of parts and voices: notes,
//! chords, rests, ties, backup/forward, tempo, time signature, and drum
//! parts (percussion clef or unpitched notes). Grace notes are skipped.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use daw_model::{MIN_LENGTH_BEATS, NoteInput, TimeSignature};
use roxmltree::Node;

use super::parse_xml;
use crate::midi::preset_for_program;
use crate::{ImportedPart, ImportedSong, NotationError, quarters_per_beat, whole_bars};

const DEFAULT_VELOCITY: u8 = 90;

fn bad(msg: impl Into<String>) -> NotationError {
    NotationError::BadMusicXml(msg.into())
}

fn child<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| c.has_tag_name(name))
}

fn child_text<'a>(n: Node<'a, '_>, name: &str) -> Option<&'a str> {
    child(n, name).and_then(|c| c.text()).map(str::trim)
}

fn child_num(n: Node<'_, '_>, name: &str) -> Option<f64> {
    child_text(n, name).and_then(|t| t.parse().ok())
}

/// The score inside a compressed `.mxl` file.
pub fn read_mxl(bytes: &[u8]) -> Result<String, NotationError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| bad(format!("not a .mxl file: {e}")))?;
    // META-INF/container.xml names the score; fall back to the first .xml.
    let mut root = None;
    if let Ok(mut f) = zip.by_name("META-INF/container.xml") {
        let mut s = String::new();
        if f.read_to_string(&mut s).is_ok()
            && let Ok(doc) = parse_xml(&s)
        {
            root = doc
                .descendants()
                .find(|n| n.has_tag_name("rootfile"))
                .and_then(|n| n.attribute("full-path"))
                .map(str::to_owned);
        }
    }
    let name = match root {
        Some(r) => r,
        None => (0..zip.len())
            .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_owned()))
            .find(|n| {
                !n.starts_with("META-INF") && (n.ends_with(".xml") || n.ends_with(".musicxml"))
            })
            .ok_or_else(|| bad("the .mxl file has no score in it"))?,
    };
    let mut f = zip.by_name(&name).map_err(|e| bad(e.to_string()))?;
    let mut s = String::new();
    f.read_to_string(&mut s).map_err(|e| bad(e.to_string()))?;
    Ok(s)
}

/// Reads a score from file bytes: compressed `.mxl` or plain XML.
pub fn import_musicxml_bytes(bytes: &[u8]) -> Result<ImportedSong, NotationError> {
    if bytes.starts_with(b"PK") {
        import_musicxml(&read_mxl(bytes)?)
    } else {
        let text = String::from_utf8_lossy(bytes);
        import_musicxml(text.trim_start_matches('\u{feff}'))
    }
}

/// General MIDI drum note for a note written at `step`/`octave` on a
/// percussion staff (the inverse of `drum_position`).
fn drum_note(step: &str, octave: i32, x_head: bool) -> u8 {
    match (step, octave, x_head) {
        ("F", 4, _) | ("E", 4, _) => 36,
        ("C", 5, true) => 37,
        ("C", 5, false) => 38,
        ("A", 4, _) => 43,
        ("D", 5, _) => 45,
        ("E", 5, _) => 48,
        ("G", 5, _) => 42,
        ("A", 5, _) => 49,
        ("F", 5, _) => 51,
        _ => 38,
    }
}

fn step_semitones(step: &str) -> Option<i32> {
    Some(match step {
        "C" => 0,
        "D" => 2,
        "E" => 4,
        "F" => 5,
        "G" => 7,
        "A" => 9,
        "B" => 11,
        _ => return None,
    })
}

/// A note being read, in quarter notes from the score start.
struct Pending {
    start_q: f64,
    end_q: f64,
    pitch: u8,
    velocity: u8,
}

struct PartReader {
    divisions: f64,
    position_q: f64,
    measure_start_q: f64,
    measure_max_q: f64,
    last_start_q: f64,
    drums: bool,
    notes: Vec<Pending>,
    /// Tied notes waiting for their continuation: pitch -> index in `notes`.
    open_ties: HashMap<u8, usize>,
    /// Drum notes from `<midi-unpitched>`, keyed by instrument id.
    unpitched: HashMap<String, u8>,
    program: Option<u8>,
}

impl PartReader {
    fn advance(&mut self, quarters: f64) {
        self.position_q = (self.position_q + quarters).max(self.measure_start_q);
        self.measure_max_q = self.measure_max_q.max(self.position_q);
    }

    fn note(&mut self, n: Node<'_, '_>) {
        if child(n, "grace").is_some() || child(n, "cue").is_some() {
            return;
        }
        let duration_q = child_num(n, "duration").unwrap_or(0.0) / self.divisions;
        let chord = child(n, "chord").is_some();
        let start = if chord {
            self.last_start_q
        } else {
            self.position_q
        };
        if !chord {
            self.last_start_q = start;
            self.advance(duration_q);
        }
        if child(n, "rest").is_some() {
            return;
        }
        let pitch = if let Some(p) = child(n, "pitch") {
            let step = child_text(p, "step").and_then(step_semitones);
            let octave = child_num(p, "octave");
            let alter = child_num(p, "alter").unwrap_or(0.0).round() as i32;
            match (step, octave) {
                (Some(s), Some(o)) => (o as i32 + 1) * 12 + s + alter,
                _ => return,
            }
        } else if let Some(u) = child(n, "unpitched") {
            self.drums = true;
            let from_instrument = child(n, "instrument")
                .and_then(|i| i.attribute("id"))
                .and_then(|id| self.unpitched.get(id).copied());
            match from_instrument {
                Some(p) => i32::from(p),
                None => {
                    let step = child_text(u, "display-step").unwrap_or("C");
                    let octave = child_num(u, "display-octave").unwrap_or(5.0) as i32;
                    let x = child_text(n, "notehead").is_some_and(|h| h == "x" || h == "cross");
                    i32::from(drum_note(step, octave, x))
                }
            }
        } else {
            return;
        };
        let Ok(pitch) = u8::try_from(pitch.clamp(0, 127)) else {
            return;
        };
        let ties: Vec<&str> = n
            .children()
            .filter(|c| c.has_tag_name("tie"))
            .filter_map(|c| c.attribute("type"))
            .collect();
        let end = start + duration_q;
        if ties.contains(&"stop")
            && let Some(i) = self.open_ties.remove(&pitch)
        {
            self.notes[i].end_q = end;
            if ties.contains(&"start") {
                self.open_ties.insert(pitch, i);
            }
            return;
        }
        let velocity = n
            .attribute("dynamics")
            .and_then(|d| d.parse::<f64>().ok())
            .map_or(DEFAULT_VELOCITY, |d| {
                (d / 100.0 * f64::from(DEFAULT_VELOCITY))
                    .round()
                    .clamp(1.0, 127.0) as u8
            });
        self.notes.push(Pending {
            start_q: start,
            end_q: end,
            pitch,
            velocity,
        });
        if ties.contains(&"start") {
            self.open_ties.insert(pitch, self.notes.len() - 1);
        }
    }
}

/// Reads a partwise MusicXML score.
pub fn import_musicxml(xml: &str) -> Result<ImportedSong, NotationError> {
    let doc = parse_xml(xml).map_err(|e| bad(e.to_string()))?;
    let root = doc.root_element();
    if root.has_tag_name("score-timewise") {
        return Err(bad(
            "timewise scores aren't supported; export as partwise MusicXML",
        ));
    }
    if !root.has_tag_name("score-partwise") {
        return Err(bad("there is no <score-partwise> in the file"));
    }
    let title = child(root, "work")
        .and_then(|w| child_text(w, "work-title"))
        .or_else(|| child_text(root, "movement-title"))
        .filter(|t| !t.is_empty())
        .map(str::to_owned);

    // Part names, instruments, and drum maps from the part list.
    let mut names: HashMap<String, String> = HashMap::new();
    let mut programs: HashMap<String, u8> = HashMap::new();
    let mut unpitched: HashMap<String, HashMap<String, u8>> = HashMap::new();
    if let Some(list) = child(root, "part-list") {
        for sp in list.children().filter(|c| c.has_tag_name("score-part")) {
            let Some(id) = sp.attribute("id") else {
                continue;
            };
            if let Some(name) = child_text(sp, "part-name").filter(|n| !n.is_empty()) {
                names.insert(id.to_owned(), name.to_owned());
            }
            for mi in sp.children().filter(|c| c.has_tag_name("midi-instrument")) {
                if let Some(p) = child_num(mi, "midi-program") {
                    programs.insert(id.to_owned(), (p as i64 - 1).clamp(0, 127) as u8);
                }
                if let (Some(iid), Some(u)) = (mi.attribute("id"), child_num(mi, "midi-unpitched"))
                {
                    // midi-unpitched counts from 1.
                    unpitched
                        .entry(id.to_owned())
                        .or_default()
                        .insert(iid.to_owned(), (u as i64 - 1).clamp(0, 127) as u8);
                }
            }
        }
    }

    let mut tempo_q: Option<f64> = None;
    let mut time_signature: Option<TimeSignature> = None;
    let mut parts = Vec::new();
    for (index, part) in root
        .children()
        .filter(|c| c.has_tag_name("part"))
        .enumerate()
    {
        let id = part.attribute("id").unwrap_or_default();
        let mut r = PartReader {
            divisions: 1.0,
            position_q: 0.0,
            measure_start_q: 0.0,
            measure_max_q: 0.0,
            last_start_q: 0.0,
            drums: false,
            notes: Vec::new(),
            open_ties: HashMap::new(),
            unpitched: unpitched.remove(id).unwrap_or_default(),
            program: programs.get(id).copied(),
        };
        for measure in part.children().filter(|c| c.has_tag_name("measure")) {
            r.measure_start_q = r.measure_max_q.max(r.position_q);
            r.position_q = r.measure_start_q;
            r.measure_max_q = r.measure_start_q;
            for el in measure.children().filter(Node::is_element) {
                match el.tag_name().name() {
                    "attributes" => {
                        if let Some(d) = child_num(el, "divisions").filter(|d| *d > 0.0) {
                            r.divisions = d;
                        }
                        if let Some(t) = child(el, "time")
                            && time_signature.is_none()
                            && let (Some(n), Some(d)) =
                                (child_num(t, "beats"), child_num(t, "beat-type"))
                        {
                            time_signature = Some(TimeSignature {
                                numerator: n.clamp(1.0, 32.0) as u8,
                                denominator: d.clamp(1.0, 32.0) as u8,
                            });
                        }
                        if child(el, "clef")
                            .and_then(|c| child_text(c, "sign"))
                            .is_some_and(|s| s == "percussion")
                        {
                            r.drums = true;
                        }
                    }
                    "note" => r.note(el),
                    "backup" => {
                        let q = child_num(el, "duration").unwrap_or(0.0) / r.divisions;
                        r.position_q = (r.position_q - q).max(r.measure_start_q);
                    }
                    "forward" => {
                        let q = child_num(el, "duration").unwrap_or(0.0) / r.divisions;
                        r.advance(q);
                    }
                    "direction" | "sound" => {
                        let sound = if el.has_tag_name("sound") {
                            Some(el)
                        } else {
                            child(el, "sound")
                        };
                        if tempo_q.is_none()
                            && let Some(t) = sound
                                .and_then(|s| s.attribute("tempo"))
                                .and_then(|t| t.parse::<f64>().ok())
                            && t > 0.0
                        {
                            tempo_q = Some(t);
                        }
                    }
                    _ => {}
                }
            }
        }
        if r.notes.is_empty() {
            continue;
        }
        let ts = time_signature.unwrap_or_default();
        let qpb = quarters_per_beat(ts);
        let mut notes: Vec<NoteInput> = r
            .notes
            .iter()
            .map(|n| NoteInput {
                chance: 100,
                pitch: n.pitch,
                start_beats: n.start_q / qpb,
                length_beats: ((n.end_q - n.start_q) / qpb).max(MIN_LENGTH_BEATS),
                velocity: n.velocity,
                id: None,
            })
            .collect();
        notes.sort_by(|a, b| {
            a.start_beats
                .total_cmp(&b.start_beats)
                .then(a.pitch.cmp(&b.pitch))
        });
        let end = notes
            .iter()
            .map(|n| n.start_beats + n.length_beats)
            .fold(0.0, f64::max);
        let mean = notes.iter().map(|n| f64::from(n.pitch)).sum::<f64>() / notes.len() as f64;
        let preset = (!r.drums).then(|| match r.program {
            Some(p) => preset_for_program(p),
            None if mean < 48.0 => "Fat Bass",
            None => "Warm Keys",
        });
        parts.push(ImportedPart {
            name: names
                .get(id)
                .cloned()
                .unwrap_or_else(|| format!("Part {}", index + 1)),
            drums: r.drums,
            preset,
            notes,
            length_beats: whole_bars(end, f64::from(ts.numerator.max(1))),
        });
    }
    if parts.is_empty() {
        return Err(NotationError::NoNotes);
    }
    let ts = time_signature.unwrap_or_default();
    Ok(ImportedSong {
        title,
        tempo_bpm: tempo_q.map(|t| t / quarters_per_beat(ts)),
        time_signature,
        parts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::musicxml::export_musicxml;
    use daw_model::{Command, Project};

    fn n(pitch: u8, start: f64, len: f64) -> NoteInput {
        NoteInput {
            chance: 100,
            pitch,
            start_beats: start,
            length_beats: len,
            velocity: 100,
            id: None,
        }
    }

    #[test]
    fn our_own_export_reads_back() {
        let mut p = Project {
            name: "Overworld".into(),
            tempo_bpm: 132.0,
            ..Project::default()
        };
        Command::CreateClip {
            track_id: 1,
            start_beats: 0.0,
            length_beats: 8.0,
            name: None,
            notes: vec![
                n(60, 0.0, 1.0),
                n(64, 0.0, 1.0),
                n(67, 1.5, 0.5),
                n(61, 3.0, 2.0),
            ],
        }
        .apply(&mut p)
        .expect("keys");
        Command::CreateClip {
            track_id: 3,
            start_beats: 0.0,
            length_beats: 4.0,
            name: None,
            notes: vec![n(36, 0.0, 0.25), n(42, 0.0, 0.25), n(38, 1.0, 0.25)],
        }
        .apply(&mut p)
        .expect("drums");
        let song = import_musicxml(&export_musicxml(&p, Some(&[1, 3]))).expect("import");
        assert_eq!(song.title.as_deref(), Some("Overworld"));
        assert!((song.tempo_bpm.expect("tempo") - 132.0).abs() < 1e-9);
        assert_eq!(song.parts.len(), 2);
        let keys: Vec<(f64, f64, u8)> = song.parts[0]
            .notes
            .iter()
            .map(|x| (x.start_beats, x.length_beats, x.pitch))
            .collect();
        // The tie across the barline comes back as one two-beat note.
        assert_eq!(
            keys,
            vec![
                (0.0, 1.0, 60),
                (0.0, 1.0, 64),
                (1.5, 0.5, 67),
                (3.0, 2.0, 61)
            ]
        );
        let drums = &song.parts[1];
        assert!(drums.drums);
        let pitches: Vec<u8> = drums.notes.iter().map(|x| x.pitch).collect();
        assert_eq!(pitches, vec![36, 42, 38]);
    }

    #[test]
    fn reads_voices_backup_and_tempo_from_hand_written_xml() {
        // What Claude might write after reading a photo of a score: two
        // voices on one staff in 3/4 at 90 BPM.
        let xml = r#"<?xml version="1.0"?>
<score-partwise version="4.0">
  <part-list><score-part id="P1"><part-name>Waltz</part-name></score-part></part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>2</divisions><time><beats>3</beats><beat-type>4</beat-type></time></attributes>
      <direction><sound tempo="90"/></direction>
      <note><pitch><step>E</step><octave>5</octave></pitch><duration>6</duration><voice>1</voice></note>
      <backup><duration>6</duration></backup>
      <note><pitch><step>C</step><octave>3</octave></pitch><duration>2</duration><voice>2</voice></note>
      <note><pitch><step>G</step><alter>-1</alter><octave>3</octave></pitch><duration>2</duration><voice>2</voice></note>
      <note><chord/><pitch><step>B</step><octave>3</octave></pitch><duration>2</duration><voice>2</voice></note>
      <note><rest/><duration>2</duration><voice>2</voice></note>
    </measure>
    <measure number="2">
      <note><pitch><step>D</step><octave>5</octave></pitch><duration>1</duration></note>
    </measure>
  </part>
</score-partwise>"#;
        let song = import_musicxml(xml).expect("import");
        assert_eq!(
            song.time_signature,
            Some(TimeSignature {
                numerator: 3,
                denominator: 4
            })
        );
        assert_eq!(song.tempo_bpm, Some(90.0));
        let p = &song.parts[0];
        assert_eq!(p.name, "Waltz");
        let got: Vec<(f64, u8)> = p.notes.iter().map(|x| (x.start_beats, x.pitch)).collect();
        assert_eq!(
            got,
            vec![(0.0, 48), (0.0, 76), (1.0, 54), (1.0, 59), (3.0, 74)]
        );
        assert_eq!(p.notes[1].length_beats, 3.0);
        assert_eq!(p.length_beats, 6.0);
    }

    #[test]
    fn readable_errors() {
        assert!(import_musicxml("not xml").is_err());
        let err = import_musicxml("<score-timewise/>").expect_err("timewise");
        assert!(err.to_string().contains("partwise"));
        assert!(matches!(
            import_musicxml("<score-partwise><part id=\"P1\"/></score-partwise>"),
            Err(NotationError::NoNotes)
        ));
    }

    #[test]
    fn compressed_mxl_files_open() {
        use std::io::Write;
        let mut p = Project::default();
        Command::CreateClip {
            track_id: 1,
            start_beats: 0.0,
            length_beats: 4.0,
            name: None,
            notes: vec![n(72, 0.0, 1.0)],
        }
        .apply(&mut p)
        .expect("clip");
        let xml = export_musicxml(&p, Some(&[1]));
        let mut buf = Vec::new();
        {
            let mut z = zip::ZipWriter::new(Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default();
            z.start_file("META-INF/container.xml", opts).expect("start");
            z.write_all(br#"<container><rootfiles><rootfile full-path="score.xml"/></rootfiles></container>"#)
                .expect("write");
            z.start_file("score.xml", opts).expect("start");
            z.write_all(xml.as_bytes()).expect("write");
            z.finish().expect("finish");
        }
        let song = import_musicxml_bytes(&buf).expect("mxl");
        assert_eq!(song.parts[0].notes[0].pitch, 72);
    }
}
