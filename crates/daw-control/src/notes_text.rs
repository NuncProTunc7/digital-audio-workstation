//! A clip's notes as compact text, bar by bar, for AIs: easier to read and
//! reason about than a long JSON list ("bar 2: @1 C4+E4+G4 q v96").

use std::fmt::Write as _;

use daw_model::music::NOTE_NAMES;
use daw_model::{Clip, Note};

/// The legend printed above the bars.
pub const LEGEND: &str = "Each note: @beat-in-bar (1 = downbeat), pitch (C4 = middle C = MIDI 60; \
drums by pad name), length (w=4 beats, h=2, q=1, e=1/2, s=1/4, t=1/8; a trailing . is dotted, \
3e etc. a triplet, other lengths in beats), v=velocity 1-127, and n% if it plays only sometimes. \
Notes that start together with the same length and velocity are joined with +.";

/// MIDI pitch as a name: 60 -> "C4".
pub fn pitch_name(pitch: u8) -> String {
    let octave = i32::from(pitch / 12) - 1;
    format!("{}{octave}", NOTE_NAMES[usize::from(pitch % 12)])
}

/// A length in beats as a short code ("q", "e.", "3e", or "1.25").
pub fn length_code(beats: f64) -> String {
    const CODES: [(f64, &str); 6] = [
        (4.0, "w"),
        (2.0, "h"),
        (1.0, "q"),
        (0.5, "e"),
        (0.25, "s"),
        (0.125, "t"),
    ];
    let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
    for (len, code) in CODES {
        if near(beats, len) {
            return code.to_owned();
        }
        if near(beats, len * 1.5) {
            return format!("{code}.");
        }
        if near(beats, len * 2.0 / 3.0) {
            return format!("3{code}");
        }
    }
    trim(beats)
}

/// A number without needless decimals: 2.0 -> "2", 2.50 -> "2.5".
fn trim(x: f64) -> String {
    let s = format!("{:.3}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.to_owned()
    }
}

/// The clip's notes as text. `drum_name` names a pitch on drum tracks.
/// `bars` limits it to a range of the clip's own bars (1-based, inclusive).
pub fn clip_text(
    clip: &Clip,
    beats_per_bar: f64,
    drum_name: Option<&dyn Fn(u8) -> Option<&'static str>>,
    bars: Option<(u32, u32)>,
) -> String {
    let bpb = beats_per_bar.max(1.0);
    let total_bars = ((clip.length_beats / bpb).ceil() as u32).max(1);
    let (first, last) = bars.unwrap_or((1, total_bars));
    let last = last.min(total_bars);
    let mut out = format!(
        "{} ({} bars from song bar {}):\n",
        if clip.name.is_empty() {
            "clip"
        } else {
            &clip.name
        },
        trim(clip.length_beats / bpb),
        trim(clip.start_beats / bpb + 1.0),
    );
    if clip.audio.is_some() {
        out.push_str("an audio clip: no notes.\n");
        return out;
    }
    let name = |pitch: u8| -> String {
        drum_name
            .and_then(|f| f(pitch))
            .map_or_else(|| pitch_name(pitch), str::to_owned)
    };
    let mut notes: Vec<&Note> = clip.notes.iter().collect();
    notes.sort_by(|a, b| {
        a.start_beats
            .total_cmp(&b.start_beats)
            .then(a.pitch.cmp(&b.pitch))
    });
    for bar in first..=last {
        let from = f64::from(bar - 1) * bpb;
        let to = from + bpb;
        let in_bar: Vec<&&Note> = notes
            .iter()
            .filter(|n| n.start_beats >= from - 1e-9 && n.start_beats < to - 1e-9)
            .collect();
        let _ = write!(out, "bar {bar}:");
        if in_bar.is_empty() {
            out.push_str(" rest\n");
            continue;
        }
        let mut i = 0;
        while i < in_bar.len() {
            let n = in_bar[i];
            let mut group = vec![name(n.pitch)];
            let mut j = i + 1;
            while j < in_bar.len() {
                let m = in_bar[j];
                let same = (m.start_beats - n.start_beats).abs() < 1e-9
                    && (m.length_beats - n.length_beats).abs() < 1e-9
                    && m.velocity == n.velocity
                    && m.chance == n.chance;
                if !same {
                    break;
                }
                group.push(name(m.pitch));
                j += 1;
            }
            let _ = write!(
                out,
                " @{} {} {} v{}",
                trim(n.start_beats - from + 1.0),
                group.join("+"),
                length_code(n.length_beats),
                n.velocity
            );
            if n.chance < 100 {
                let _ = write!(out, " {}%", n.chance);
            }
            out.push(',');
            i = j;
        }
        out.pop();
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(pitch: u8, start: f64, len: f64, velocity: u8) -> Note {
        Note {
            id: 0,
            pitch,
            start_beats: start,
            length_beats: len,
            velocity,
            chance: 100,
            guitar: None,
        }
    }

    fn clip(notes: Vec<Note>, length_beats: f64) -> Clip {
        let mut c: Clip = serde_json::from_value(serde_json::json!({
            "id": 1, "name": "", "start_beats": 4.0, "length_beats": length_beats, "notes": []
        }))
        .expect("clip");
        c.notes = notes;
        c.name = "Verse".into();
        c
    }

    #[test]
    fn names_pitches_and_lengths() {
        assert_eq!(pitch_name(60), "C4");
        assert_eq!(pitch_name(69), "A4");
        assert_eq!(pitch_name(21), "A0");
        assert_eq!(length_code(1.0), "q");
        assert_eq!(length_code(1.5), "q.");
        assert_eq!(length_code(1.0 / 3.0), "3e");
        assert_eq!(length_code(0.25), "s");
        assert_eq!(length_code(1.25), "1.25");
    }

    #[test]
    fn writes_bars_with_chords_joined_and_rests() {
        let c = clip(
            vec![
                note(64, 0.0, 1.0, 100),
                note(60, 0.0, 1.0, 100),
                note(67, 0.0, 1.0, 100),
                note(72, 1.5, 0.5, 80),
                Note {
                    chance: 50,
                    ..note(74, 8.0, 2.0, 90)
                },
            ],
            12.0,
        );
        let text = clip_text(&c, 4.0, None, None);
        assert_eq!(
            text,
            "Verse (3 bars from song bar 2):\n\
             bar 1: @1 C4+E4+G4 q v100, @2.5 C5 e v80\n\
             bar 2: rest\n\
             bar 3: @1 D5 h v90 50%\n"
        );
        // A range of bars.
        let part = clip_text(&c, 4.0, None, Some((3, 9)));
        assert!(part.ends_with("bar 3: @1 D5 h v90 50%\n"), "{part}");
        assert!(!part.contains("bar 1:"));
    }

    #[test]
    fn drums_use_pad_names() {
        let c = clip(
            vec![note(36, 0.0, 0.25, 110), note(38, 1.0, 0.25, 100)],
            4.0,
        );
        let names = |p: u8| match p {
            36 => Some("Kick"),
            38 => Some("Snare"),
            _ => None,
        };
        let text = clip_text(&c, 4.0, Some(&names), None);
        assert!(
            text.contains("bar 1: @1 Kick s v110, @2 Snare s v100"),
            "{text}"
        );
    }
}
