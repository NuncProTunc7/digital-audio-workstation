//! Compact views of a project for Claude: enough to understand and edit the
//! song without thousands of lines of instrument parameters and notes.

use serde_json::{Value, json};

use crate::project::{Clip, Project, Track};

/// The whole song at a glance: tracks, clips (without notes), mixer, effects.
pub fn song_summary(project: &Project) -> Value {
    let bpb = project.beats_per_bar();
    json!({
        "name": project.name,
        "tempo_bpm": project.tempo_bpm,
        "time_signature": format!("{}/{}", project.time_signature.numerator, project.time_signature.denominator),
        "beats_per_bar": bpb,
        "length_beats": project.end_beats(),
        "length_bars": (project.end_beats() / bpb).ceil(),
        "loop": project.loop_region,
        "master": {
            "volume_db": project.master.volume_db,
            "effects": project.master.effects.iter().map(|e| json!({"id": e.id, "kind": e.kind, "enabled": e.enabled})).collect::<Vec<_>>(),
        },
        "tracks": project.tracks.iter().map(track_brief).collect::<Vec<_>>(),
    })
}

fn track_brief(t: &Track) -> Value {
    json!({
        "id": t.id,
        "name": t.name,
        "instrument": t.instrument.kind,
        "preset": t.instrument.preset,
        "volume_db": t.mixer.volume_db,
        "pan": t.mixer.pan,
        "mute": t.mixer.mute,
        "solo": t.mixer.solo,
        "effects": t.mixer.effects.iter().map(|e| json!({"id": e.id, "kind": e.kind, "enabled": e.enabled})).collect::<Vec<_>>(),
        "clips": t.clips.iter().map(clip_brief).collect::<Vec<_>>(),
    })
}

fn clip_brief(c: &Clip) -> Value {
    let lo = c.notes.iter().map(|n| n.pitch).min();
    let hi = c.notes.iter().map(|n| n.pitch).max();
    json!({
        "id": c.id,
        "name": c.name,
        "start_beats": c.start_beats,
        "length_beats": c.length_beats,
        "note_count": c.notes.len(),
        "lowest_pitch": lo,
        "highest_pitch": hi,
    })
}

/// One track in full (instrument parameters, effect settings), with clip
/// summaries instead of notes.
pub fn track_detail(track: &Track) -> Value {
    let mut v = track_brief(track);
    v["instrument_params"] = json!(track.instrument.params);
    v["effects"] = json!(track.mixer.effects);
    v
}

/// One clip with every note.
pub fn clip_detail(track: &Track, clip: &Clip) -> Value {
    json!({
        "track_id": track.id,
        "track_name": track.name,
        "instrument": track.instrument.kind,
        "clip": clip,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_lists_tracks_without_instrument_params() {
        let s = song_summary(&Project::default());
        assert_eq!(s["tracks"].as_array().map(Vec::len), Some(3));
        assert!(s["tracks"][0].get("instrument_params").is_none());
        assert_eq!(s["time_signature"], "4/4");
    }

    #[test]
    fn track_detail_includes_params() {
        let p = Project::default();
        let d = track_detail(&p.tracks[0]);
        assert!(d["instrument_params"]["filter.cutoff_hz"].is_number());
    }
}
