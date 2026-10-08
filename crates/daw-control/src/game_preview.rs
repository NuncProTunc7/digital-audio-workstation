//! Game preview: hear the song the way the game will play the Godot
//! export, before exporting. Each section loops on its own; switching
//! sections waits for the next bar line; tracks act as layers the game can
//! fade in and out. Live actions only: nothing here changes the song.

use daw_model::{Project, SongSection, TrackId};
use serde::Serialize;

use crate::host::{Host, engine};

/// One track, as a layer the game can fade.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Layer {
    pub track_id: TrackId,
    pub name: String,
}

/// What can be switched and faded.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PreviewPlan {
    pub sections: Vec<SongSection>,
    pub layers: Vec<Layer>,
}

/// The sections to preview: the song's markers, or else one section (the
/// loop region if it's on, otherwise the whole song).
pub fn plan(project: &Project) -> PreviewPlan {
    let mut sections = project.sections();
    if sections.is_empty() {
        let bpb = project.beats_per_bar();
        let (start, end) = if project.loop_region.enabled {
            (
                project.loop_region.start_beats,
                project.loop_region.end_beats,
            )
        } else {
            (0.0, ((project.end_beats() / bpb).ceil() * bpb).max(bpb))
        };
        sections.push(SongSection {
            name: if project.loop_region.enabled {
                "Loop".into()
            } else {
                "Whole song".into()
            },
            start_beats: start,
            end_beats: end,
        });
    }
    PreviewPlan {
        sections,
        layers: project
            .tracks
            .iter()
            .map(|t| Layer {
                track_id: t.id,
                name: t.name.clone(),
            })
            .collect(),
    }
}

fn section(project: &Project, index: usize) -> Result<SongSection, String> {
    plan(project)
        .sections
        .get(index)
        .cloned()
        .ok_or_else(|| format!("there is no section {index}"))
}

/// Starts playing section `index` on a loop with every layer up.
pub fn start<H: Host>(host: &H, index: usize) -> Result<PreviewPlan, String> {
    let project = host.session()?.project().clone();
    let s = section(&project, index)?;
    let engine = engine(host)?;
    engine.stop();
    engine.cancel_jump();
    engine.reset_layers();
    engine.send(daw_engine::EngineMessage::SetLoop {
        enabled: true,
        start_beats: s.start_beats,
        end_beats: s.end_beats,
    });
    engine.locate(s.start_beats);
    engine.play();
    Ok(plan(&project))
}

/// Changes to section `index` at the next bar line (as the exported
/// `AudioStreamInteractive` does).
pub fn switch<H: Host>(host: &H, index: usize) -> Result<(), String> {
    let project = host.session()?.project().clone();
    let s = section(&project, index)?;
    engine(host)?.jump_at_next_bar(s.start_beats, s.start_beats, s.end_beats);
    Ok(())
}

/// Fades a track in or out over `fade_beats` (0 = at once).
pub fn fade<H: Host>(host: &H, track_id: TrackId, on: bool, fade_beats: f64) -> Result<(), String> {
    let tempo = host.session()?.project().tempo_bpm;
    let seconds = (fade_beats.max(0.0) * 60.0 / tempo) as f32;
    engine(host)?.fade_layer(track_id, if on { 1.0 } else { 0.0 }, seconds);
    Ok(())
}

/// Stops the preview: playback stops, layers come back up, and the song's
/// own loop setting returns.
pub fn stop<H: Host>(host: &H) -> Result<(), String> {
    let project = host.session()?.project().clone();
    let engine = engine(host)?;
    engine.stop();
    engine.cancel_jump();
    engine.reset_layers();
    let l = project.loop_region;
    engine.send(daw_engine::EngineMessage::SetLoop {
        enabled: l.enabled,
        start_beats: l.start_beats,
        end_beats: l.end_beats,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_model::Command;

    #[test]
    fn sections_come_from_markers_or_the_loop_or_the_whole_song() {
        let mut p = Project::default();
        Command::CreateClip {
            track_id: 1,
            start_beats: 0.0,
            length_beats: 14.0,
            name: None,
            notes: Vec::new(),
        }
        .apply(&mut p)
        .expect("clip");
        let names = |p: &Project| {
            plan(p)
                .sections
                .iter()
                .map(|s| (s.name.clone(), s.start_beats, s.end_beats))
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&p), [("Whole song".into(), 0.0, 16.0)]);
        Command::SetLoop {
            enabled: Some(true),
            start_beats: Some(4.0),
            end_beats: Some(8.0),
        }
        .apply(&mut p)
        .expect("loop");
        assert_eq!(names(&p), [("Loop".into(), 4.0, 8.0)]);
        for (name, beat) in [("Explore", 0.0), ("Combat", 8.0)] {
            Command::AddMarker {
                name: name.into(),
                start_beats: beat,
            }
            .apply(&mut p)
            .expect("marker");
        }
        assert_eq!(
            names(&p),
            [("Explore".into(), 0.0, 8.0), ("Combat".into(), 8.0, 16.0)]
        );
        assert_eq!(plan(&p).layers.len(), 3);
    }
}
