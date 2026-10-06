use std::sync::{Arc, Mutex};

use daw_model::instrument::param_specs;
use daw_model::{Project, TrackId};

use crate::message::{EngineMessage, TrackSlot};
use crate::processor::AudioProcessor;
use crate::status::{EngineStatus, StatusSnapshot};

const MESSAGE_CAPACITY: usize = 4096;
const GARBAGE_CAPACITY: usize = 64;

/// The control-side handle to a running [`AudioProcessor`].
///
/// Safe to share between threads (UI commands, MIDI input). Locks here only
/// guard the control side of the queues; the audio thread never sees them.
pub struct Engine {
    producer: Mutex<rtrb::Producer<EngineMessage>>,
    garbage: Mutex<rtrb::Consumer<Box<[TrackSlot]>>>,
    status: Arc<EngineStatus>,
    sample_rate_hz: f32,
    // The project as last sent to the processor, for diffing.
    synced: Mutex<Project>,
}

impl Engine {
    /// Creates an engine for `project` and the processor it controls. Move
    /// the processor to the audio thread (or drive it offline).
    pub fn new(project: &Project, sample_rate_hz: u32) -> (Engine, AudioProcessor) {
        let sample_rate = sample_rate_hz.max(1) as f32;
        let (producer, consumer) = rtrb::RingBuffer::new(MESSAGE_CAPACITY);
        let (garbage_tx, garbage_rx) = rtrb::RingBuffer::new(GARBAGE_CAPACITY);
        let status = Arc::new(EngineStatus::default());
        status.set_sample_rate(sample_rate_hz);
        let processor = AudioProcessor::new(
            sample_rate,
            consumer,
            garbage_tx,
            Arc::clone(&status),
            build_tracks(project, sample_rate),
            project.tempo_bpm,
            project.time_signature.numerator,
        );
        let engine = Engine {
            producer: Mutex::new(producer),
            garbage: Mutex::new(garbage_rx),
            status,
            sample_rate_hz: sample_rate,
            synced: Mutex::new(project.clone()),
        };
        (engine, processor)
    }

    pub fn sample_rate_hz(&self) -> f32 {
        self.sample_rate_hz
    }

    /// Queues a message for the audio thread. Returns `false` if the queue
    /// was full and the message was dropped.
    pub fn send(&self, message: EngineMessage) -> bool {
        self.collect_garbage();
        match self.producer.lock() {
            Ok(mut p) => p.push(message).is_ok(),
            Err(_) => false,
        }
    }

    pub fn note_on(&self, track_id: TrackId, note: u8, velocity: f32) {
        self.send(EngineMessage::NoteOn {
            track_id,
            note: note.min(127),
            velocity: velocity.clamp(0.0, 1.0),
        });
    }

    pub fn note_off(&self, track_id: TrackId, note: u8) {
        self.send(EngineMessage::NoteOff {
            track_id,
            note: note.min(127),
        });
    }

    pub fn all_notes_off(&self) {
        self.send(EngineMessage::AllNotesOff);
    }

    pub fn play(&self) {
        self.send(EngineMessage::Play);
    }

    /// Stops playback. A second stop while stopped returns to the start.
    pub fn stop(&self) {
        if !self.status.is_playing() {
            self.send(EngineMessage::Locate(0.0));
        }
        self.send(EngineMessage::Stop);
    }

    pub fn set_metronome(&self, on: bool) {
        self.send(EngineMessage::SetMetronome(on));
    }

    pub fn status(&self) -> StatusSnapshot {
        self.status.take_snapshot()
    }

    /// Brings the processor in line with `project`, sending only what changed.
    pub fn sync(&self, project: &Project) {
        let Ok(mut synced) = self.synced.lock() else {
            return;
        };
        if synced.tempo_bpm != project.tempo_bpm {
            self.send(EngineMessage::SetTempo(project.tempo_bpm));
        }
        if synced.time_signature != project.time_signature {
            self.send(EngineMessage::SetTimeSignature {
                numerator: project.time_signature.numerator,
                denominator: project.time_signature.denominator,
            });
        }
        let same_layout = synced.tracks.len() == project.tracks.len()
            && synced
                .tracks
                .iter()
                .zip(&project.tracks)
                .all(|(a, b)| a.id == b.id && a.instrument.kind == b.instrument.kind);
        if same_layout {
            for (old, new) in synced.tracks.iter().zip(&project.tracks) {
                for (index, spec) in param_specs(new.instrument.kind).iter().enumerate() {
                    let value = new.instrument.value(spec.id);
                    if old.instrument.value(spec.id) != value {
                        self.send(EngineMessage::SetParam {
                            track_id: new.id,
                            index,
                            value: value.unwrap_or(spec.default) as f32,
                        });
                    }
                }
            }
        } else {
            let tracks = build_tracks(project, self.sample_rate_hz);
            self.send(EngineMessage::ReplaceTracks(tracks));
        }
        *synced = project.clone();
    }

    /// Frees track sets the audio thread has swapped out.
    fn collect_garbage(&self) {
        if let Ok(mut g) = self.garbage.lock() {
            while g.pop().is_ok() {}
        }
    }
}

fn build_tracks(project: &Project, sample_rate_hz: f32) -> Box<[TrackSlot]> {
    project
        .tracks
        .iter()
        .map(|t| TrackSlot {
            id: t.id,
            instrument: daw_instruments::create(&t.instrument, sample_rate_hz),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_model::{Command, Session};

    const SR: u32 = 48_000;

    fn render(p: &mut AudioProcessor, seconds: f32) -> Vec<f32> {
        let mut out = vec![0.0; (seconds * SR as f32) as usize * 2];
        for chunk in out.chunks_mut(256) {
            p.process_interleaved(chunk, 2);
        }
        out
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn silent_until_a_note_is_played() {
        let (engine, mut p) = Engine::new(&Project::default(), SR);
        assert_eq!(peak(&render(&mut p, 0.2)), 0.0);
        engine.note_on(1, 60, 1.0);
        assert!(peak(&render(&mut p, 0.2)) > 0.01);
    }

    #[test]
    fn notes_reach_only_their_track() {
        let (engine, mut p) = Engine::new(&Project::default(), SR);
        engine.note_on(99, 60, 1.0);
        assert_eq!(peak(&render(&mut p, 0.2)), 0.0);
    }

    #[test]
    fn metronome_clicks_on_every_beat_while_playing() {
        let (engine, mut p) = Engine::new(&Project::default(), SR);
        engine.play();
        let out = render(&mut p, 2.0);
        // At 120 BPM a beat is 0.5 s = 24 000 frames. Find click onsets.
        let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
        let mut onsets = Vec::new();
        let mut quiet_for = usize::MAX;
        for (i, s) in left.iter().enumerate() {
            if s.abs() > 0.01 {
                if quiet_for > 4_800 {
                    onsets.push(i);
                }
                quiet_for = 0;
            } else {
                quiet_for = quiet_for.saturating_add(1);
            }
        }
        assert_eq!(onsets.len(), 4, "{onsets:?}");
        for (n, &onset) in onsets.iter().enumerate() {
            let expected = n * 24_000;
            assert!(onset.abs_diff(expected) <= 2, "click {n} at {onset}");
        }
        // Downbeat is louder than the other beats.
        let loudness = |at: usize| peak(&left[at..at + 400]);
        assert!(loudness(onsets[0]) > loudness(onsets[1]));
    }

    #[test]
    fn metronome_can_be_muted() {
        let (engine, mut p) = Engine::new(&Project::default(), SR);
        engine.set_metronome(false);
        engine.play();
        assert_eq!(peak(&render(&mut p, 1.0)), 0.0);
    }

    #[test]
    fn transport_position_follows_tempo() {
        let mut session = Session::default();
        session
            .execute(Command::SetTempo { bpm: 90.0 })
            .expect("tempo");
        let (engine, mut p) = Engine::new(session.project(), SR);
        engine.play();
        render(&mut p, 2.0);
        let s = engine.status();
        assert!(s.playing);
        // 90 BPM for 2 s = 3 beats.
        assert!(
            (s.position_beats - 3.0).abs() < 0.01,
            "{}",
            s.position_beats
        );
        engine.stop();
        render(&mut p, 0.01);
        engine.stop();
        render(&mut p, 0.01);
        assert_eq!(engine.status().position_beats, 0.0);
    }

    #[test]
    fn param_changes_are_synced() {
        let mut session = Session::default();
        let (engine, mut p) = Engine::new(session.project(), SR);
        engine.set_metronome(false);
        session
            .execute(Command::SetInstrumentParam {
                track_id: 1,
                param: "master.gain_db".into(),
                value: -36.0,
            })
            .expect("ok");
        engine.sync(session.project());
        render(&mut p, 0.1);
        engine.note_on(1, 60, 1.0);
        let quiet = peak(&render(&mut p, 0.3));

        session.undo();
        engine.sync(session.project());
        render(&mut p, 0.1);
        engine.all_notes_off();
        render(&mut p, 2.0);
        engine.note_on(1, 60, 1.0);
        let loud = peak(&render(&mut p, 0.3));
        assert!(loud > quiet * 10.0, "quiet {quiet} loud {loud}");
    }

    #[test]
    fn swapped_out_tracks_are_freed_off_the_audio_thread() {
        let mut session = Session::default();
        let (engine, mut p) = Engine::new(session.project(), SR);
        session
            .execute(Command::SetInstrument {
                track_id: 1,
                instrument: daw_model::Instrument::from_preset(
                    daw_model::InstrumentKind::Drums,
                    "Classic Kit",
                )
                .expect("kit"),
            })
            .expect("ok");
        engine.sync(session.project());
        render(&mut p, 0.01);
        assert_eq!(engine.garbage.lock().expect("lock").slots(), 1);
        engine.collect_garbage();
        assert_eq!(engine.garbage.lock().expect("lock").slots(), 0);
        // The track now plays drums.
        engine.note_on(1, 36, 1.0);
        assert!(peak(&render(&mut p, 0.1)) > 0.05);
    }
}
