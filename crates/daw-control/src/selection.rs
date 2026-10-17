//! What the user is pointing at in the app, so an AI can act on "this".
//!
//! Selection is a live view, like the playhead: it is not saved, not a
//! project edit, and never enters undo. The UI reports it whenever it
//! changes; [`describe`] turns it into ids plus enough context for the AI to
//! check it has the right thing.

use daw_model::{ClipId, Id, NoteId, Project, TrackId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// What is selected in the app's window.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Selection {
    /// The open tab at the bottom of the window ("instrument", "pianoroll",
    /// "sheet", "mixer", "sounds").
    #[serde(default)]
    pub view: Option<String>,
    #[serde(default)]
    pub track_id: Option<TrackId>,
    #[serde(default)]
    pub clip_id: Option<ClipId>,
    /// Notes selected in the piano roll (in `clip_id`).
    #[serde(default)]
    pub note_ids: Vec<NoteId>,
    /// The sound selected in the Sound FX panel, while it is open.
    #[serde(default)]
    pub sound_id: Option<Id>,
}

/// "C4" for 60, "F#3" for 54.
fn pitch_name(pitch: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let octave = i32::from(pitch / 12) - 1;
    format!("{}{octave}", NAMES[usize::from(pitch % 12)])
}

/// Bar number (1 = the first bar) at a time in beats, to two places.
fn bar_at(project: &Project, beats: f64) -> f64 {
    let bar = 1.0 + beats / project.beats_per_bar();
    (bar * 100.0).round() / 100.0
}

/// The selection with its context: names, bar ranges, note counts and
/// pitch range, and a one-line summary. Things deleted since the UI
/// reported them are left out. `position_beats` is the playhead, when audio
/// is running.
pub fn describe(project: &Project, selection: &Selection, position_beats: Option<f64>) -> Value {
    let mut out = serde_json::Map::new();
    let mut said: Vec<String> = Vec::new();
    if let Some(view) = &selection.view {
        out.insert("view".into(), json!(view));
    }
    if let Some(beats) = position_beats {
        out.insert(
            "playhead".into(),
            json!({ "beats": beats, "bar": bar_at(project, beats) }),
        );
    }
    if let Some(track) = selection.track_id.and_then(|id| project.track(id)) {
        out.insert(
            "track".into(),
            json!({ "id": track.id, "name": track.name, "instrument": track.instrument.kind }),
        );
        said.push(format!("track \"{}\" (id {})", track.name, track.id));
    }
    let clip = selection.clip_id.and_then(|id| project.clip(id));
    if let Some((track_id, clip)) = clip {
        let end = clip.start_beats + clip.length_beats;
        out.insert(
            "clip".into(),
            json!({
                "id": clip.id,
                "name": clip.name,
                "track_id": track_id,
                "start_beats": clip.start_beats,
                "length_beats": clip.length_beats,
                "start_bar": bar_at(project, clip.start_beats),
                "end_bar": bar_at(project, end),
                "audio": clip.is_audio(),
            }),
        );
        said.push(format!(
            "clip \"{}\" (id {}, bars {}-{})",
            clip.name,
            clip.id,
            bar_at(project, clip.start_beats),
            bar_at(project, end)
        ));
        let notes: Vec<_> = clip
            .notes
            .iter()
            .filter(|n| selection.note_ids.contains(&n.id))
            .collect();
        if let (Some(low), Some(high)) = (
            notes.iter().map(|n| n.pitch).min(),
            notes.iter().map(|n| n.pitch).max(),
        ) {
            let ids: Vec<NoteId> = notes.iter().map(|n| n.id).collect();
            out.insert(
                "notes".into(),
                json!({
                    "count": ids.len(),
                    "ids": ids,
                    "lowest_pitch": low,
                    "highest_pitch": high,
                    "lowest": pitch_name(low),
                    "highest": pitch_name(high),
                }),
            );
            let range = if low == high {
                pitch_name(low)
            } else {
                format!("{}-{}", pitch_name(low), pitch_name(high))
            };
            said.push(format!(
                "{} selected note{} ({range})",
                ids.len(),
                if ids.len() == 1 { "" } else { "s" }
            ));
        }
    }
    if let Some(sound) = selection.sound_id.and_then(|id| project.sound(id)) {
        out.insert(
            "sound".into(),
            json!({ "id": sound.id, "name": sound.name }),
        );
        said.push(format!("sound \"{}\" (id {})", sound.name, sound.id));
    }
    let summary = if said.is_empty() {
        "Nothing is selected.".to_owned()
    } else {
        format!("Selected: {}.", said.join(", "))
    };
    out.insert("summary".into(), json!(summary));
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_names_follow_middle_c_is_c4() {
        assert_eq!(pitch_name(60), "C4");
        assert_eq!(pitch_name(54), "F#3");
        assert_eq!(pitch_name(0), "C-1");
    }

    #[test]
    fn selection_json_round_trips_and_defaults_to_nothing() {
        let s: Selection = serde_json::from_str("{}").expect("empty");
        assert_eq!(s, Selection::default());
        let s = Selection {
            view: Some("pianoroll".into()),
            track_id: Some(1),
            clip_id: Some(9),
            note_ids: vec![10, 11],
            sound_id: None,
        };
        let back: Selection =
            serde_json::from_value(serde_json::to_value(&s).expect("json")).expect("parse");
        assert_eq!(back, s);
    }

    #[test]
    fn nothing_selected_says_so() {
        let v = describe(&Project::default(), &Selection::default(), None);
        assert_eq!(v["summary"], "Nothing is selected.");
        assert!(v.get("track").is_none());
        assert!(v.get("playhead").is_none());
    }

    #[test]
    fn deleted_things_are_left_out() {
        let s = Selection {
            track_id: Some(999),
            clip_id: Some(998),
            note_ids: vec![997],
            sound_id: Some(996),
            ..Default::default()
        };
        let v = describe(&Project::default(), &s, None);
        for key in ["track", "clip", "notes", "sound"] {
            assert!(v.get(key).is_none(), "{key}");
        }
    }
}
