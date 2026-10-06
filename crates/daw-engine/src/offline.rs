//! Rendering without a sound card, for tests, CI, and file export.

use daw_model::Project;

use crate::{Engine, EngineMessage, ToneGenerator};

/// Renders `seconds` of a test tone as interleaved stereo samples.
///
/// The tone fades in at the start and fades out at the end, exactly as it
/// would when started and stopped live.
pub fn render_test_tone(sample_rate_hz: u32, freq_hz: f64, gain: f32, seconds: f64) -> Vec<f32> {
    const CHANNELS: usize = 2;
    let total_frames = (seconds.max(0.0) * f64::from(sample_rate_hz)).round() as usize;
    // Start fading out early enough that the fade completes inside the render.
    let fade_frames = (sample_rate_hz as usize / 100).max(1);
    let stop_frame = total_frames.saturating_sub(fade_frames);

    let mut tone = ToneGenerator::new(sample_rate_hz, freq_hz, gain);
    let mut out = vec![0.0; total_frames * CHANNELS];
    tone.set_on(true);
    let (head, tail) = out.split_at_mut(stop_frame * CHANNELS);
    tone.process(head, CHANNELS);
    tone.set_on(false);
    tone.process(tail, CHANNELS);
    out
}

/// A message to deliver at a specific time during an offline render.
pub struct TimedMessage {
    pub at_seconds: f64,
    pub message: EngineMessage,
}

/// Renders `project` for `seconds`, delivering each message at its time
/// (sample-accurate). Returns interleaved stereo. The metronome is off unless
/// a message turns it on.
pub fn render_project(
    project: &Project,
    mut messages: Vec<TimedMessage>,
    seconds: f64,
    sample_rate_hz: u32,
) -> Vec<f32> {
    let (engine, mut processor) = Engine::new(project, sample_rate_hz);
    engine.set_metronome(false);
    messages.sort_by(|a, b| a.at_seconds.total_cmp(&b.at_seconds));

    let total_frames = (seconds.max(0.0) * f64::from(sample_rate_hz)).round() as usize;
    let mut out = vec![0.0; total_frames * 2];
    let mut frame = 0;
    let mut pending = messages.into_iter().peekable();
    while frame < total_frames {
        while let Some(m) = pending
            .next_if(|m| (m.at_seconds * f64::from(sample_rate_hz)).round() as usize <= frame)
        {
            engine.send(m.message);
        }
        let next_event = pending
            .peek()
            .map(|m| (m.at_seconds * f64::from(sample_rate_hz)).round() as usize)
            .unwrap_or(total_frames)
            .clamp(frame + 1, total_frames);
        processor.process_interleaved(&mut out[frame * 2..next_event * 2], 2);
        frame = next_event;
    }
    out
}

/// Renders beats `start_beats..end_beats` (looping off) plus `tail_seconds`
/// of ring-out. Returns interleaved stereo.
pub fn render_region(
    project: &Project,
    start_beats: f64,
    end_beats: f64,
    tail_seconds: f64,
    sample_rate_hz: u32,
) -> Vec<f32> {
    let mut p = project.clone();
    p.loop_region.enabled = false;
    let beats = (end_beats - start_beats).max(0.0);
    let seconds = beats * 60.0 / p.tempo_bpm + tail_seconds.max(0.0);
    render_project(
        &p,
        vec![
            TimedMessage {
                at_seconds: 0.0,
                message: EngineMessage::Locate(start_beats.max(0.0)),
            },
            TimedMessage {
                at_seconds: 0.0,
                message: EngineMessage::Play,
            },
        ],
        seconds,
        sample_rate_hz,
    )
}

/// Renders the whole song from the start (looping off) plus `tail_seconds`
/// for reverb and delay tails. Returns interleaved stereo.
pub fn render_song(project: &Project, sample_rate_hz: u32, tail_seconds: f64) -> Vec<f32> {
    let mut p = project.clone();
    p.loop_region.enabled = false;
    let seconds = p.end_beats() * 60.0 / p.tempo_bpm + tail_seconds.max(0.0);
    render_project(
        &p,
        vec![TimedMessage {
            at_seconds: 0.0,
            message: EngineMessage::Play,
        }],
        seconds,
        sample_rate_hz,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DEFAULT_SAMPLE_RATE_HZ;

    const SR: u32 = DEFAULT_SAMPLE_RATE_HZ;

    fn render() -> Vec<f32> {
        render_test_tone(SR, 440.0, 0.5, 1.0)
    }

    #[test]
    fn has_expected_length() {
        assert_eq!(render().len(), SR as usize * 2);
    }

    #[test]
    fn all_samples_are_finite_and_within_gain() {
        for (i, s) in render().iter().enumerate() {
            assert!(s.is_finite(), "sample {i} is {s}");
            assert!(s.abs() <= 0.5 + 1e-6, "sample {i} is {s}");
        }
    }

    #[test]
    fn reaches_requested_peak() {
        let peak = render().iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.5).abs() < 0.01, "peak {peak}");
    }

    #[test]
    fn starts_and_ends_silent_without_clicks() {
        let samples = render();
        assert_eq!(samples[0], 0.0);
        assert_eq!(*samples.last().expect("non-empty"), 0.0);
        let max_jump = samples
            .chunks(2)
            .map(|f| f[0])
            .collect::<Vec<_>>()
            .windows(2)
            .fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
        assert!(max_jump < 0.03, "max jump {max_jump}");
    }

    #[test]
    fn project_render_places_notes_sample_accurately() {
        let at = 0.25;
        let out = render_project(
            &Project::default(),
            vec![TimedMessage {
                at_seconds: at,
                message: EngineMessage::NoteOn {
                    track_id: 3,
                    note: 36,
                    velocity: 1.0,
                },
            }],
            0.5,
            SR,
        );
        let first_sound = out.chunks(2).position(|f| f[0].abs() > 1e-6);
        assert_eq!(first_sound, Some((at * f64::from(SR)) as usize));
    }

    #[test]
    fn song_render_covers_all_clips_plus_tail() {
        let mut s = daw_model::Session::default();
        s.execute(daw_model::Command::CreateClip {
            track_id: 1,
            start_beats: 2.0,
            length_beats: 2.0,
            name: None,
            notes: vec![daw_model::NoteInput {
                pitch: 60,
                start_beats: 0.0,
                length_beats: 1.0,
                velocity: 100,
                id: None,
            }],
        })
        .expect("clip");
        let out = render_song(s.project(), SR, 1.0);
        // 4 beats at 120 BPM = 2 s, plus 1 s tail.
        assert_eq!(out.len(), 3 * SR as usize * 2);
        let first = out
            .chunks(2)
            .position(|f| f[0].abs() > 1e-4)
            .expect("sound");
        assert!(first.abs_diff(SR as usize) < 10, "{first}");
    }

    #[test]
    fn project_render_is_deterministic() {
        let run = || {
            let msgs = (0..8)
                .map(|i| TimedMessage {
                    at_seconds: f64::from(i) * 0.1,
                    message: EngineMessage::NoteOn {
                        track_id: 1,
                        note: 60 + i as u8,
                        velocity: 0.8,
                    },
                })
                .collect();
            render_project(&Project::default(), msgs, 1.0, SR)
        };
        assert_eq!(run(), run());
    }
}
