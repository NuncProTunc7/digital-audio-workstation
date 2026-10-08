//! Turns a track's clips into the time-ordered note events and audio
//! regions the audio thread plays.

use daw_audio::AudioPool;
use daw_model::effect::effect_params;
use daw_model::instrument::param_specs;
use daw_model::{AutomationTarget, Project, Track};

use crate::message::{AudioRegionPlay, AutoCurve, AutoTarget, SeqEvent, Sequence};

/// The track's enabled automation lanes, resolved to engine targets. Lanes
/// whose target no longer exists (a removed effect) are skipped.
fn build_automation(track: &Track) -> Vec<AutoCurve> {
    track
        .automation
        .iter()
        .filter(|l| l.enabled && !l.points.is_empty())
        .filter_map(|l| {
            let target = match &l.target {
                AutomationTarget::Volume => AutoTarget::Volume,
                AutomationTarget::Pan => AutoTarget::Pan,
                AutomationTarget::InstrumentParam { param } => AutoTarget::Instrument(
                    param_specs(track.instrument.kind)
                        .iter()
                        .position(|s| s.id == param)?,
                ),
                AutomationTarget::EffectParam { effect_id, param } => {
                    let fx = track.mixer.effects.iter().find(|e| e.id == *effect_id)?;
                    AutoTarget::Effect {
                        effect_id: *effect_id,
                        index: effect_params(fx.kind).iter().position(|s| s.id == param)?,
                    }
                }
            };
            Some(AutoCurve::new(
                target,
                l.points.iter().map(|p| (p.beats, p.value as f32)).collect(),
            ))
        })
        .collect()
}

/// Whether `track` plays its frozen rendering: it has one, nothing changed
/// since, and the file can be loaded.
pub fn plays_frozen(project: &Project, track: &Track, audio: &AudioPool) -> bool {
    track
        .frozen
        .as_ref()
        .is_some_and(|f| audio.has(&f.file) && project.frozen_is_current(track))
}

/// What the audio thread plays for `track`: its frozen rendering (with its
/// automation) when it plays frozen, else its clips.
pub fn track_sequence(
    project: &Project,
    track: &Track,
    audio: &AudioPool,
    sample_rate_hz: u32,
) -> Sequence {
    if plays_frozen(project, track, audio)
        && let Some(f) = &track.frozen
        && let Ok(buffer) = audio.buffer(&f.file, sample_rate_hz)
    {
        let seconds = buffer.frames() as f64 / f64::from(sample_rate_hz.max(1));
        return Sequence {
            events: Vec::new(),
            audio: vec![AudioRegionPlay {
                start_beats: 0.0,
                end_beats: seconds * project.tempo_bpm / 60.0,
                buffer,
                offset_frames: 0.0,
                gain: 1.0,
                fade_in_frames: 0.0,
                fade_out_frames: 0.0,
            }],
            automation: build_automation(track),
        };
    }
    build_sequence(track, audio, sample_rate_hz, project.tempo_bpm)
}

/// Flattens all clips on a track. Notes are cut at their clip's end; at the
/// same beat, note-offs come before note-ons so repeated notes retrigger.
/// Audio clips whose file can't be loaded are left out (silent).
/// Clips that follow the tempo are stretched to `tempo_bpm`.
pub fn build_sequence(
    track: &Track,
    audio: &AudioPool,
    sample_rate_hz: u32,
    tempo_bpm: f64,
) -> Sequence {
    let mut events = Vec::new();
    let sr = f64::from(sample_rate_hz);
    let mut regions = Vec::new();
    // Muted clips (unused takes) are kept but not heard.
    for clip in track.clips.iter().filter(|c| !c.muted) {
        if let Some(a) = &clip.audio {
            // Following the tempo: slower songs stretch the audio longer.
            let ratio = a.source_bpm.map_or(1.0, |src| src / tempo_bpm);
            if let Ok(buffer) = audio.stretched(&a.file, sample_rate_hz, ratio) {
                regions.push(AudioRegionPlay {
                    start_beats: clip.start_beats,
                    end_beats: clip.start_beats + clip.length_beats,
                    buffer,
                    offset_frames: a.offset_seconds * sr * ratio,
                    gain: if a.gain_db <= daw_model::MIN_VOLUME_DB {
                        0.0
                    } else {
                        10f32.powf(a.gain_db as f32 / 20.0)
                    },
                    fade_in_frames: a.fade_in_seconds * sr,
                    fade_out_frames: a.fade_out_seconds * sr,
                });
            }
            continue;
        }
        for n in &clip.notes {
            if n.start_beats >= clip.length_beats {
                continue;
            }
            // Swing moves notes as they play; it never moves one past the
            // clip's (swung) end.
            let on = clip.played_song_beats(n.start_beats);
            let off =
                clip.played_song_beats((n.start_beats + n.length_beats).min(clip.length_beats));
            events.push(SeqEvent {
                beat: on,
                note: n.pitch.min(127),
                velocity: (f32::from(n.velocity) / 127.0).clamp(1.0 / 127.0, 1.0),
                chance: n.chance.min(100),
            });
            events.push(SeqEvent {
                beat: off,
                note: n.pitch.min(127),
                velocity: 0.0,
                chance: 100,
            });
        }
    }
    events.sort_by(|a, b| {
        a.beat
            .total_cmp(&b.beat)
            .then_with(|| a.velocity.total_cmp(&b.velocity))
    });
    regions.sort_by(|a, b| a.start_beats.total_cmp(&b.start_beats));
    Sequence {
        events,
        audio: regions,
        automation: build_automation(track),
    }
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
            chance: 100,
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
            link: None,
            muted: false,
            swing: None,
            id: 10,
            name: "c".into(),
            start_beats: 4.0,
            length_beats: 2.0,
            notes: vec![
                note(1, 60, 0.0, 1.0),
                note(2, 62, 1.5, 4.0),
                note(3, 64, 3.0, 1.0),
            ],
            audio: None,
        }]);
        let s = build_sequence(&t, &AudioPool::in_temp_dir(), 48_000, 120.0);
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
            link: None,
            muted: false,
            swing: None,
            id: 10,
            name: "c".into(),
            start_beats: 0.0,
            length_beats: 4.0,
            notes: vec![note(1, 60, 0.0, 1.0), note(2, 60, 1.0, 1.0)],
            audio: None,
        }]);
        let s = build_sequence(&t, &AudioPool::in_temp_dir(), 48_000, 120.0);
        assert_eq!(s.events[1].beat, 1.0);
        assert_eq!(s.events[1].velocity, 0.0);
        assert!(s.events[2].velocity > 0.0);
    }
}
