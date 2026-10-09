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
        "key": project.key.map(|k| json!({
            "name": k.name(),
            "tonic": k.tonic,
            "mode": k.mode,
            "scale_notes": k.scale().iter().map(|p| crate::music::NOTE_NAMES[usize::from(*p)]).collect::<Vec<_>>(),
        })),
        "length_beats": project.end_beats(),
        "length_bars": (project.end_beats() / bpb).ceil(),
        "loop": project.loop_region,
        "master": {
            "volume_db": project.master.volume_db,
            "effects": project.master.effects.iter().map(|e| json!({"id": e.id, "kind": e.kind, "enabled": e.enabled})).collect::<Vec<_>>(),
        },
        "tracks": project.tracks.iter().map(|t| track_brief(project, t)).collect::<Vec<_>>(),
        "buses": project.buses.iter().map(|b| json!({
            "id": b.id,
            "name": b.name,
            "volume_db": b.mixer.volume_db,
            "pan": b.mixer.pan,
            "mute": b.mixer.mute,
            "effects": b.mixer.effects.iter().map(|e| json!({"id": e.id, "kind": e.kind, "enabled": e.enabled})).collect::<Vec<_>>(),
            "tracks_playing_into_it": project.tracks.iter().filter(|t| t.output == Some(b.id)).map(|t| t.id).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "chords": project.chords.iter().enumerate().map(|(i, c)| json!({
            "id": c.id,
            "name": c.name(),
            "start_beats": c.start_beats,
            "end_beats": project.chords.get(i + 1).map_or(project.end_beats().max(c.start_beats + bpb), |n| n.start_beats),
            "notes": c.pitch_classes().iter().map(|p| crate::music::NOTE_NAMES[usize::from(*p)]).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "markers": project.markers.iter().map(|m| json!({
            "id": m.id,
            "name": m.name,
            "start_beats": m.start_beats,
        })).collect::<Vec<_>>(),
        "sections": project.sections(),
        "versions": project.snapshots.iter().map(|s| json!({
            "id": s.id,
            "name": s.name,
            "tracks": s.song.tracks.len(),
            "tempo_bpm": s.song.tempo_bpm,
        })).collect::<Vec<_>>(),
    })
}

/// "frozen", "out of date (plays live)" (edited since freezing), or "no".
fn frozen_state(project: &Project, t: &Track) -> &'static str {
    match &t.frozen {
        None => "no",
        Some(_) if project.frozen_is_current(t) => "frozen",
        Some(_) => "out of date (plays live)",
    }
}

fn track_brief(project: &Project, t: &Track) -> Value {
    json!({
        "id": t.id,
        "name": t.name,
        "instrument": t.instrument.kind,
        "preset": t.instrument.preset,
        "plugin": t.instrument.plugin.as_ref().map(|p| json!({
            "uid": p.uid,
            "name": p.name,
            "vendor": p.vendor,
        })),
        "volume_db": t.mixer.volume_db,
        "pan": t.mixer.pan,
        "mute": t.mixer.mute,
        "solo": t.mixer.solo,
        "output_bus": t.output,
        "frozen": frozen_state(project, t),
        "sends": t.sends,
        "effects": t.mixer.effects.iter().map(|e| json!({"id": e.id, "kind": e.kind, "enabled": e.enabled})).collect::<Vec<_>>(),
        "clips": t.clips.iter().map(clip_brief).collect::<Vec<_>>(),
        "automation": t.automation.iter().map(|l| json!({
            "id": l.id,
            "target": l.target,
            "enabled": l.enabled,
            "points": l.points.len(),
            "from_beats": l.points.first().map(|p| p.beats),
            "to_beats": l.points.last().map(|p| p.beats),
        })).collect::<Vec<_>>(),
    })
}

fn clip_brief(c: &Clip) -> Value {
    if let Some(a) = &c.audio {
        return json!({
            "id": c.id,
            "name": c.name,
            "start_beats": c.start_beats,
            "length_beats": c.length_beats,
            "muted": c.muted,
            "audio": a,
        });
    }
    let lo = c.notes.iter().map(|n| n.pitch).min();
    let hi = c.notes.iter().map(|n| n.pitch).max();
    json!({
        "id": c.id,
        "name": c.name,
        "start_beats": c.start_beats,
        "length_beats": c.length_beats,
        "muted": c.muted,
        "note_count": c.notes.len(),
        "lowest_pitch": lo,
        "highest_pitch": hi,
    })
}

/// One track in full (instrument parameters, effect settings), with clip
/// summaries instead of notes.
pub fn track_detail(project: &Project, track: &Track) -> Value {
    let mut v = track_brief(project, track);
    v["instrument_params"] = json!(track.instrument.params);
    v["effects"] = json!(track.mixer.effects);
    v["automation"] = json!(track.automation);
    v
}

/// One clip with every note.
pub fn clip_detail(track: &Track, clip: &Clip) -> Value {
    let mut v = json!({
        "track_id": track.id,
        "track_name": track.name,
        "instrument": track.instrument.kind,
        "clip": clip,
    });
    // Serialization leaves out `muted` when false; say it either way.
    v["clip"]["muted"] = json!(clip.muted);
    v
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
        let d = track_detail(&p, &p.tracks[0]);
        assert!(d["instrument_params"]["filter.cutoff_hz"].is_number());
    }

    #[test]
    fn audio_clips_show_their_audio_instead_of_notes() {
        let c = Clip {
            link: None,
            muted: false,
            swing: None,
            id: 9,
            name: "Vox".into(),
            start_beats: 0.0,
            length_beats: 8.0,
            notes: Vec::new(),
            audio: Some(crate::AudioRegion {
                file: "vox.wav".into(),
                file_seconds: 4.0,
                offset_seconds: 0.0,
                gain_db: -3.0,
                fade_in_seconds: 0.0,
                fade_out_seconds: 0.5,
                source_bpm: None,
            }),
        };
        let b = clip_brief(&c);
        assert_eq!(b["audio"]["file"], "vox.wav");
        assert!(b.get("note_count").is_none());
        assert_eq!(b["muted"], false);
    }

    #[test]
    fn muted_takes_and_frozen_tracks_are_reported() {
        // Found 2026-10-09: after comping, Claude saw stacked takes with no
        // way to tell which one plays, and couldn't tell a track was frozen.
        let mut p = Project::default();
        p.tracks[0].clips.push(Clip {
            link: None,
            muted: true,
            swing: None,
            id: 90,
            name: "Take 1".into(),
            start_beats: 0.0,
            length_beats: 4.0,
            notes: Vec::new(),
            audio: None,
        });
        p.tracks[0].frozen = Some(crate::project::Frozen {
            file: "keys frozen.wav".into(),
            fingerprint: p.freeze_fingerprint(&p.tracks[0]),
        });
        let song = song_summary(&p);
        assert_eq!(song["tracks"][0]["clips"][0]["muted"], true);
        assert_eq!(song["tracks"][0]["frozen"], "frozen");
        assert_eq!(song["tracks"][1]["frozen"], "no");
        let detail = track_detail(&p, &p.tracks[0]);
        assert_eq!(detail["frozen"], "frozen");
        let clip = clip_detail(&p.tracks[0], &p.tracks[0].clips[0]);
        assert_eq!(clip["clip"]["muted"], true);

        // An edit after freezing makes it play live again.
        p.tempo_bpm = 90.0;
        assert_eq!(
            song_summary(&p)["tracks"][0]["frozen"],
            "out of date (plays live)"
        );
    }
}
