use std::sync::{Arc, Mutex};

use daw_audio::AudioPool;
use daw_model::effect::effect_params;
use daw_model::instrument::param_specs;
use daw_model::{Effect, Project, Track, TrackId};

use crate::message::{EffectChain, EffectSlot, EngineMessage, Garbage, RecordedEvent, TrackSlot};
use crate::processor::{
    AudioProcessor, ProcessorInit, ProcessorOutputs, db_to_fader_gain, strip_settings,
};
use crate::sequence::build_sequence;
use crate::status::{EngineStatus, StatusSnapshot};

const MESSAGE_CAPACITY: usize = 8192;
const GARBAGE_CAPACITY: usize = 256;
const RECORD_CAPACITY: usize = 16_384;

/// The control-side handle to a running [`AudioProcessor`].
///
/// Safe to share between threads (UI commands, MIDI input). Locks here only
/// guard the control side of the queues; the audio thread never sees them.
pub struct Engine {
    producer: Mutex<rtrb::Producer<EngineMessage>>,
    garbage: Mutex<rtrb::Consumer<Garbage>>,
    recorded: Mutex<rtrb::Consumer<RecordedEvent>>,
    status: Arc<EngineStatus>,
    sample_rate_hz: f32,
    // Where audio clips' files come from.
    audio: Arc<AudioPool>,
    // The project as last sent to the processor, for diffing.
    synced: Mutex<Project>,
}

impl Engine {
    /// Creates an engine for `project` and the processor it controls. Move
    /// the processor to the audio thread (or drive it offline). Audio clips
    /// are silent unless their files are in a scratch folder; use
    /// [`with_audio`](Self::with_audio) to play a project's audio.
    pub fn new(project: &Project, sample_rate_hz: u32) -> (Engine, AudioProcessor) {
        Self::with_audio(project, sample_rate_hz, AudioPool::in_temp_dir())
    }

    /// Like [`new`](Self::new), loading audio clips from `audio`.
    pub fn with_audio(
        project: &Project,
        sample_rate_hz: u32,
        audio: Arc<AudioPool>,
    ) -> (Engine, AudioProcessor) {
        let sample_rate = sample_rate_hz.max(1) as f32;
        let (producer, consumer) = rtrb::RingBuffer::new(MESSAGE_CAPACITY);
        let (garbage_tx, garbage_rx) = rtrb::RingBuffer::new(GARBAGE_CAPACITY);
        let (record_tx, record_rx) = rtrb::RingBuffer::new(RECORD_CAPACITY);
        let status = Arc::new(EngineStatus::default());
        status.set_sample_rate(sample_rate_hz);
        let processor = AudioProcessor::new(
            sample_rate,
            consumer,
            ProcessorOutputs {
                garbage: garbage_tx,
                recorded: record_tx,
            },
            Arc::clone(&status),
            ProcessorInit {
                tracks: build_tracks(project, sample_rate, &audio),
                master_effects: build_chain(
                    &project.master.effects,
                    project.tempo_bpm,
                    sample_rate,
                ),
                master_gain: db_to_fader_gain(project.master.volume_db),
                tempo_bpm: project.tempo_bpm,
                beats_per_bar: project.time_signature.numerator,
                loop_enabled: project.loop_region.enabled,
                loop_start: project.loop_region.start_beats,
                loop_end: project.loop_region.end_beats,
            },
        );
        let engine = Engine {
            producer: Mutex::new(producer),
            garbage: Mutex::new(garbage_rx),
            recorded: Mutex::new(record_rx),
            status,
            sample_rate_hz: sample_rate,
            audio,
            synced: Mutex::new(project.clone()),
        };
        (engine, processor)
    }

    pub fn sample_rate_hz(&self) -> f32 {
        self.sample_rate_hz
    }

    /// The pool audio clips are loaded from.
    pub fn audio(&self) -> &Arc<AudioPool> {
        &self.audio
    }

    /// Reloads every audio track's clips, for when files appear or change
    /// on disk without the project changing (a project was saved elsewhere).
    pub fn reload_audio(&self, project: &Project) {
        for t in project
            .tracks
            .iter()
            .filter(|t| t.instrument.kind.is_audio())
        {
            self.send(EngineMessage::ReplaceSequence {
                track_id: t.id,
                sequence: Box::new(build_sequence(
                    t,
                    &self.audio,
                    self.sample_rate_hz as u32,
                    project.tempo_bpm,
                )),
            });
        }
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

    /// Game preview: fades a track to `gain` (0.0–1.0) over `seconds`.
    pub fn fade_layer(&self, track_id: TrackId, gain: f32, seconds: f32) {
        self.send(EngineMessage::FadeLayer {
            track_id,
            gain,
            seconds,
        });
    }

    /// Game preview: every track back to full level.
    pub fn reset_layers(&self) {
        self.send(EngineMessage::ResetLayers);
    }

    /// Game preview: at the next bar line, jump to `to_beats` and loop
    /// `loop_start..loop_end` from then on.
    pub fn jump_at_next_bar(&self, to_beats: f64, loop_start: f64, loop_end: f64) {
        self.send(EngineMessage::JumpAtNextBar {
            to_beats,
            loop_start,
            loop_end,
        });
    }

    /// Forgets a queued section change.
    pub fn cancel_jump(&self) {
        self.send(EngineMessage::CancelJump);
    }

    /// Plays `beats` metronome clicks, then starts the song from the
    /// playhead (a count-in for recording). 0 beats plays straight away.
    pub fn play_with_count_in(&self, beats: f64) {
        if beats > 0.0 {
            self.send(EngineMessage::CountIn(beats));
        } else {
            self.play();
        }
    }

    /// Stops playback. A second stop while stopped returns to the start.
    pub fn stop(&self) {
        if !self.status.is_playing() {
            self.send(EngineMessage::Locate(0.0));
        }
        self.send(EngineMessage::Stop);
    }

    /// Moves the playhead to `beats`.
    pub fn locate(&self, beats: f64) {
        self.send(EngineMessage::Locate(beats.max(0.0)));
    }

    pub fn set_metronome(&self, on: bool) {
        self.send(EngineMessage::SetMetronome(on));
    }

    /// Starts capturing live notes played on `track_id` while the transport
    /// runs. Clears anything captured before.
    pub fn start_recording(&self, track_id: TrackId) {
        self.take_recorded();
        self.send(EngineMessage::SetRecordTrack(Some(track_id)));
    }

    /// Stops capturing and returns what was played.
    pub fn stop_recording(&self) -> Vec<RecordedEvent> {
        self.send(EngineMessage::SetRecordTrack(None));
        self.take_recorded()
    }

    /// Notes captured so far (drains them).
    pub fn take_recorded(&self) -> Vec<RecordedEvent> {
        let mut out = Vec::new();
        if let Ok(mut r) = self.recorded.lock() {
            while let Ok(e) = r.pop() {
                out.push(e);
            }
        }
        out
    }

    /// Shared live status (meters, transport, playback clock).
    pub fn status_handle(&self) -> Arc<EngineStatus> {
        Arc::clone(&self.status)
    }

    /// Which beat the speakers play when, if the sound card has said.
    pub fn clock(&self) -> Option<crate::ClockAnchor> {
        self.status.clock()
    }

    pub fn status(&self) -> StatusSnapshot {
        self.status.take_snapshot()
    }

    /// Brings the processor in line with `project`, sending only what changed.
    pub fn sync(&self, project: &Project) {
        let Ok(mut synced) = self.synced.lock() else {
            return;
        };
        let sr = self.sample_rate_hz;
        if synced.tempo_bpm != project.tempo_bpm {
            self.send(EngineMessage::SetTempo(project.tempo_bpm));
        }
        if synced.time_signature != project.time_signature {
            self.send(EngineMessage::SetTimeSignature {
                numerator: project.time_signature.numerator,
                denominator: project.time_signature.denominator,
            });
        }
        if synced.loop_region != project.loop_region {
            let l = project.loop_region;
            self.send(EngineMessage::SetLoop {
                enabled: l.enabled,
                start_beats: l.start_beats,
                end_beats: l.end_beats,
            });
        }
        if synced.master.volume_db != project.master.volume_db {
            self.send(EngineMessage::SetMasterGain(db_to_fader_gain(
                project.master.volume_db,
            )));
        }
        self.sync_chain(
            None,
            &synced.master.effects,
            &project.master.effects,
            project.tempo_bpm,
        );

        let same_layout = synced.tracks.len() == project.tracks.len()
            && synced.tracks.iter().zip(&project.tracks).all(|(a, b)| {
                a.id == b.id
                    && a.instrument.kind == b.instrument.kind
                    && a.instrument.sample_pack == b.instrument.sample_pack
            });
        if same_layout {
            let tempo_changed = synced.tempo_bpm != project.tempo_bpm;
            for (old, new) in synced.tracks.iter().zip(&project.tracks) {
                self.sync_track(old, new, project.tempo_bpm, tempo_changed);
            }
        } else {
            self.send(EngineMessage::ReplaceTracks(build_tracks(
                project,
                sr,
                &self.audio,
            )));
        }
        *synced = project.clone();
    }

    fn sync_track(&self, old: &Track, new: &Track, tempo_bpm: f64, tempo_changed: bool) {
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
        let (os, ns) = (&old.mixer, &new.mixer);
        if (os.volume_db, os.pan, os.mute, os.solo) != (ns.volume_db, ns.pan, ns.mute, ns.solo) {
            self.send(EngineMessage::SetStrip {
                track_id: new.id,
                strip: strip_settings(ns),
            });
        }
        self.sync_chain(Some(new.id), &os.effects, &ns.effects, tempo_bpm);
        let automation_changed = old.automation != new.automation;
        // Clips that follow the tempo need re-stretching when it changes.
        let restretch = tempo_changed
            && new
                .clips
                .iter()
                .any(|c| c.audio.as_ref().is_some_and(|a| a.source_bpm.is_some()));
        if old.clips != new.clips || automation_changed || restretch {
            self.send(EngineMessage::ReplaceSequence {
                track_id: new.id,
                sequence: Box::new(build_sequence(
                    new,
                    &self.audio,
                    self.sample_rate_hz as u32,
                    tempo_bpm,
                )),
            });
        }
        if automation_changed {
            // Lanes that went away leave their settings where the curve
            // left them; put the sliders' values back. Lanes that remain
            // take over again on the next block.
            self.resend_static(new);
        }
    }

    /// Sends a track's fader, pan, and every instrument and effect setting.
    fn resend_static(&self, track: &Track) {
        self.send(EngineMessage::SetStrip {
            track_id: track.id,
            strip: strip_settings(&track.mixer),
        });
        for (index, spec) in param_specs(track.instrument.kind).iter().enumerate() {
            self.send(EngineMessage::SetParam {
                track_id: track.id,
                index,
                value: track.instrument.value(spec.id).unwrap_or(spec.default) as f32,
            });
        }
        for e in &track.mixer.effects {
            for (index, spec) in effect_params(e.kind).iter().enumerate() {
                self.send(EngineMessage::SetEffectParam {
                    track_id: Some(track.id),
                    effect_id: e.id,
                    index,
                    value: e.value(spec.id).unwrap_or(spec.default) as f32,
                });
            }
        }
    }

    fn sync_chain(&self, track_id: Option<TrackId>, old: &[Effect], new: &[Effect], tempo: f64) {
        let same_shape = old.len() == new.len()
            && old
                .iter()
                .zip(new)
                .all(|(a, b)| a.id == b.id && a.kind == b.kind);
        if !same_shape {
            self.send(EngineMessage::ReplaceEffects {
                track_id,
                chain: build_chain(new, tempo, self.sample_rate_hz),
            });
            return;
        }
        for (a, b) in old.iter().zip(new) {
            if a.enabled != b.enabled {
                self.send(EngineMessage::SetEffectEnabled {
                    track_id,
                    effect_id: b.id,
                    enabled: b.enabled,
                });
            }
            for (index, spec) in effect_params(b.kind).iter().enumerate() {
                let value = b.value(spec.id);
                if a.value(spec.id) != value {
                    self.send(EngineMessage::SetEffectParam {
                        track_id,
                        effect_id: b.id,
                        index,
                        value: value.unwrap_or(spec.default) as f32,
                    });
                }
            }
        }
    }

    /// Frees things the audio thread has swapped out.
    fn collect_garbage(&self) {
        if let Ok(mut g) = self.garbage.lock() {
            while g.pop().is_ok() {}
        }
    }
}

fn build_chain(effects: &[Effect], tempo_bpm: f64, sample_rate_hz: f32) -> Box<EffectChain> {
    Box::new(EffectChain {
        effects: effects
            .iter()
            .map(|e| {
                let mut processor = daw_effects::create(e, sample_rate_hz);
                processor.set_tempo(tempo_bpm as f32);
                EffectSlot {
                    id: e.id,
                    enabled: e.enabled,
                    processor,
                }
            })
            .collect(),
    })
}

fn build_tracks(project: &Project, sample_rate_hz: f32, audio: &AudioPool) -> Box<[TrackSlot]> {
    project
        .tracks
        .iter()
        .map(|t| {
            TrackSlot::new(
                t.id,
                daw_instruments::create(&t.instrument, sample_rate_hz),
                build_chain(&t.mixer.effects, project.tempo_bpm, sample_rate_hz),
                Box::new(build_sequence(
                    t,
                    audio,
                    sample_rate_hz as u32,
                    project.tempo_bpm,
                )),
                strip_settings(&t.mixer),
                sample_rate_hz,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_model::{Command, EffectKind, NoteInput, Session};

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

    fn left(x: &[f32]) -> Vec<f32> {
        x.chunks(2).map(|f| f[0]).collect()
    }

    fn onsets(x: &[f32], threshold: f32, min_gap: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut quiet = usize::MAX;
        for (i, s) in x.iter().enumerate() {
            if s.abs() > threshold {
                if quiet > min_gap {
                    out.push(i);
                }
                quiet = 0;
            } else {
                quiet = quiet.saturating_add(1);
            }
        }
        out
    }

    /// Session with a one-bar kick clip on the drums (kicks on each beat).
    fn kick_session() -> Session {
        let mut s = Session::default();
        s.execute(Command::CreateClip {
            track_id: 3,
            start_beats: 0.0,
            length_beats: 4.0,
            name: None,
            notes: (0..4)
                .map(|b| NoteInput {
                    chance: 100,
                    pitch: 36,
                    start_beats: f64::from(b),
                    length_beats: 0.25,
                    velocity: 127,
                    id: None,
                })
                .collect(),
        })
        .expect("clip");
        s
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
        let out = left(&render(&mut p, 2.0));
        let found = onsets(&out, 0.01, 4_800);
        assert_eq!(found.len(), 4, "{found:?}");
        for (n, &onset) in found.iter().enumerate() {
            assert!(onset.abs_diff(n * 24_000) <= 2, "click {n} at {onset}");
        }
        let loudness = |at: usize| peak(&out[at..at + 400]);
        assert!(loudness(found[0]) > loudness(found[1]));
    }

    #[test]
    fn clips_play_sample_accurately() {
        let s = kick_session();
        let (engine, mut p) = Engine::new(s.project(), SR);
        engine.set_metronome(false);
        engine.play();
        let out = left(&render(&mut p, 2.0));
        let found = onsets(&out, 0.05, 4_800);
        assert_eq!(found.len(), 4, "{found:?}");
        for (n, &onset) in found.iter().enumerate() {
            assert!(onset.abs_diff(n * 24_000) <= 1, "kick {n} at {onset}");
        }
    }

    #[test]
    fn loop_region_repeats() {
        let mut s = kick_session();
        // Loop beats 1..2: only the second kick, every half second.
        s.execute(Command::SetLoop {
            enabled: Some(true),
            start_beats: Some(1.0),
            end_beats: Some(2.0),
        })
        .expect("loop");
        let (engine, mut p) = Engine::new(s.project(), SR);
        engine.set_metronome(false);
        engine.locate(1.0);
        engine.play();
        let out = left(&render(&mut p, 2.0));
        let found = onsets(&out, 0.05, 4_800);
        assert_eq!(found.len(), 4, "{found:?}");
        for (n, &onset) in found.iter().enumerate() {
            assert!(onset.abs_diff(n * 24_000) <= 1, "loop {n} at {onset}");
        }
        assert!(engine.status().position_beats < 2.0);
    }

    #[test]
    fn notes_on_the_loop_start_play_on_every_lap() {
        // At 44.1 kHz a beat isn't a whole number of samples, so the loop
        // wraps a hair past its start; the downbeat must still play.
        const SR_441: u32 = 44_100;
        let mut s = kick_session();
        s.execute(Command::SetLoop {
            enabled: Some(true),
            start_beats: Some(0.0),
            end_beats: Some(4.0),
        })
        .expect("loop");
        let (engine, mut p) = Engine::new(s.project(), SR_441);
        engine.set_metronome(false);
        engine.play();
        let n = (5.9 * f64::from(SR_441)) as usize;
        let mut out = vec![0.0; n * 2];
        p.process_interleaved(&mut out, 2);
        let found = onsets(&left(&out), 0.05, 4_000);
        // A kick on every beat of each two-second lap (within 2 samples:
        // later laps start a fraction of a sample late).
        assert_eq!(found.len(), 12, "{found:?}");
        for (k, &onset) in found.iter().enumerate() {
            assert!(onset.abs_diff(k * 22_050) <= 2, "kick {k} at {onset}");
        }
    }

    #[test]
    fn mute_and_solo() {
        let mut s = kick_session();
        let (engine, mut p) = Engine::new(s.project(), SR);
        engine.set_metronome(false);
        s.execute(Command::SetTrackMixer {
            track_id: 3,
            volume_db: None,
            pan: None,
            mute: Some(true),
            solo: None,
        })
        .expect("mute");
        engine.sync(s.project());
        engine.play();
        // Let the fader glide down (it smooths over ~10 ms).
        render(&mut p, 0.2);
        assert!(peak(&render(&mut p, 1.0)) < 1e-3, "muted track is audible");

        s.undo();
        s.execute(Command::SetTrackMixer {
            track_id: 1,
            volume_db: None,
            pan: None,
            mute: None,
            solo: Some(true),
        })
        .expect("solo other");
        engine.sync(s.project());
        engine.locate(0.0);
        render(&mut p, 0.2);
        assert!(
            peak(&render(&mut p, 1.0)) < 1e-3,
            "non-soloed track is audible"
        );
    }

    #[test]
    fn pan_moves_sound_to_one_side() {
        let mut s = Session::default();
        s.execute(Command::SetTrackMixer {
            track_id: 1,
            volume_db: None,
            pan: Some(-1.0),
            mute: None,
            solo: None,
        })
        .expect("pan");
        let (engine, mut p) = Engine::new(s.project(), SR);
        engine.note_on(1, 60, 1.0);
        let out = render(&mut p, 0.3);
        let l = peak(&out.chunks(2).map(|f| f[0]).collect::<Vec<_>>());
        let r = peak(&out.chunks(2).map(|f| f[1]).collect::<Vec<_>>());
        assert!(l > 0.05 && r < 1e-4, "l {l} r {r}");
    }

    #[test]
    fn effects_are_applied_and_bypassable() {
        let mut s = Session::default();
        let (engine, mut p) = Engine::new(s.project(), SR);
        engine.note_on(1, 60, 1.0);
        render(&mut p, 0.2);
        engine.note_off(1, 60);
        render(&mut p, 2.0);
        // Dry: silent two seconds later.
        assert!(peak(&render(&mut p, 0.1)) < 1e-4);

        s.execute(Command::AddEffect {
            track_id: Some(1),
            kind: EffectKind::Delay,
            index: None,
        })
        .expect("fx");
        let id = s.project().tracks[0].mixer.effects[0].id;
        s.execute(Command::SetEffectParam {
            track_id: Some(1),
            effect_id: id,
            param: "feedback".into(),
            value: 0.9,
        })
        .expect("fb");
        engine.sync(s.project());
        engine.note_on(1, 60, 1.0);
        render(&mut p, 0.2);
        engine.note_off(1, 60);
        render(&mut p, 2.0);
        assert!(peak(&render(&mut p, 0.1)) > 1e-3, "no echoes");

        s.execute(Command::SetEffectEnabled {
            track_id: Some(1),
            effect_id: id,
            enabled: false,
        })
        .expect("bypass");
        engine.sync(s.project());
        render(&mut p, 0.5);
        assert!(peak(&render(&mut p, 0.1)) < 1e-4, "bypass leaks");
    }

    #[test]
    fn master_volume_scales_output() {
        let mut s = Session::default();
        s.execute(Command::SetMasterVolume { volume_db: -60.0 })
            .expect("vol");
        let (engine, mut p) = Engine::new(s.project(), SR);
        engine.note_on(1, 60, 1.0);
        assert_eq!(peak(&render(&mut p, 0.2)), 0.0);
    }

    #[test]
    fn records_live_notes_with_beat_positions() {
        let (engine, mut p) = Engine::new(&Project::default(), SR);
        engine.set_metronome(false);
        engine.start_recording(1);
        engine.play();
        render(&mut p, 0.5); // one beat at 120 BPM
        engine.note_on(1, 64, 0.8);
        render(&mut p, 0.25);
        engine.note_off(1, 64);
        engine.note_on(2, 40, 0.8); // other track: not recorded
        render(&mut p, 0.01);
        let events = engine.stop_recording();
        assert_eq!(events.len(), 2, "{events:?}");
        assert!((events[0].beat - 1.0).abs() < 0.02, "{}", events[0].beat);
        assert!((events[1].beat - 1.5).abs() < 0.02, "{}", events[1].beat);
        assert_eq!(events[1].velocity, 0.0);
    }

    #[test]
    fn count_in_clicks_then_starts_the_song_on_the_beat() {
        let s = kick_session();
        let (engine, mut p) = Engine::new(s.project(), SR);
        // Clicks during the count-in even with the metronome off.
        engine.set_metronome(false);
        engine.play_with_count_in(4.0);
        render(&mut p, 0.25);
        let status = engine.status();
        assert!(status.playing);
        assert_eq!(
            status.position_beats, 0.0,
            "the playhead waits at the start"
        );
        assert!(
            (status.count_in_beats - 3.5).abs() < 0.02,
            "{}",
            status.count_in_beats
        );
        let out = left(&render(&mut p, 3.75));
        let found = onsets(&out, 0.01, 4_800);
        // Four clicks (at 0.0 s..1.5 s, the first already heard), then a
        // kick on each beat from 2.0 s: the song starts right after the
        // last click, on beat 1.
        let expected: Vec<usize> = (1..8).map(|n| n * 24_000 - 12_000).collect();
        assert_eq!(found.len(), expected.len(), "{found:?}");
        for (&onset, &want) in found.iter().zip(&expected) {
            assert!(onset.abs_diff(want) <= 2, "onset at {onset}, wanted {want}");
        }
        assert_eq!(engine.status().count_in_beats, 0.0);
    }

    #[test]
    fn the_song_is_silent_during_a_count_in_from_the_middle() {
        // Kicks on the off-beats of bars 1 and 2; count in to bar 2.
        let mut s = Session::default();
        s.execute(Command::CreateClip {
            track_id: 3,
            start_beats: 0.0,
            length_beats: 8.0,
            name: None,
            notes: (0..8)
                .map(|b| NoteInput {
                    chance: 100,
                    pitch: 36,
                    start_beats: f64::from(b) + 0.5,
                    length_beats: 0.25,
                    velocity: 127,
                    id: None,
                })
                .collect(),
        })
        .expect("clip");
        let (engine, mut p) = Engine::new(s.project(), SR);
        engine.set_metronome(false);
        engine.locate(4.0);
        engine.play_with_count_in(4.0);
        let out = left(&render(&mut p, 4.0));
        let found = onsets(&out, 0.01, 4_800);
        // Clicks on the beats of the count-in, then the bar-2 kicks.
        let expected: Vec<usize> = (0..4)
            .map(|n| n * 24_000)
            .chain((0..4).map(|n| 48_000 * 2 + 12_000 + n * 24_000))
            .collect();
        assert_eq!(found.len(), expected.len(), "{found:?}");
        for (&onset, &want) in found.iter().zip(&expected) {
            assert!(onset.abs_diff(want) <= 2, "onset at {onset}, wanted {want}");
        }
    }

    /// One-bar clip on the drums with a kick at each of `starts`.
    fn kicks_at(starts: &[f64], chance: u8) -> Session {
        let mut s = Session::default();
        s.execute(Command::CreateClip {
            track_id: 3,
            start_beats: 0.0,
            length_beats: 4.0,
            name: None,
            notes: starts
                .iter()
                .map(|&b| NoteInput {
                    chance,
                    pitch: 36,
                    start_beats: b,
                    length_beats: 0.1,
                    velocity: 127,
                    id: None,
                })
                .collect(),
        })
        .expect("clip");
        s
    }

    #[test]
    fn swing_plays_off_steps_late() {
        let mut s = kicks_at(&[0.25, 2.25], 100);
        let clip = s.project().tracks[2].clips[0].id;
        s.execute(Command::SetClipSwing {
            clip_id: clip,
            swing: Some(daw_model::Swing {
                amount_percent: 100.0,
                grid_beats: 0.25,
            }),
        })
        .expect("swing");
        let (engine, mut p) = Engine::new(s.project(), SR);
        engine.set_metronome(false);
        engine.play();
        let found = onsets(&left(&render(&mut p, 2.0)), 0.05, 4_800);
        // 0.25 and 2.25 beats are heard at 0.375 and 2.375 (24 000 samples a beat).
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].abs_diff(9_000) <= 2, "{found:?}");
        assert!(found[1].abs_diff(57_000) <= 2, "{found:?}");
    }

    #[test]
    fn chance_notes_vary_by_lap_but_repeat_exactly() {
        let starts: Vec<f64> = (0..4).map(f64::from).collect();
        let mut s = kicks_at(&starts, 50);
        s.execute(Command::SetLoop {
            enabled: Some(true),
            start_beats: Some(0.0),
            end_beats: Some(4.0),
        })
        .expect("loop");
        let laps = 16;
        let play = |s: &Session| {
            let (engine, mut p) = Engine::new(s.project(), SR);
            engine.set_metronome(false);
            engine.play();
            left(&render(&mut p, 2.0 * laps as f32))
        };
        let first = play(&s);
        assert_eq!(first, play(&s), "the same song must play the same way");
        let per_lap: Vec<usize> = first
            .chunks(96_000)
            .map(|lap| onsets(lap, 0.05, 4_800).len())
            .collect();
        let total: usize = per_lap.iter().sum();
        // About half of the 64 kicks play, and not the same in every lap.
        assert!((16..=48).contains(&total), "{per_lap:?}");
        assert!(per_lap.iter().any(|&n| n != per_lap[0]), "{per_lap:?}");
    }

    #[test]
    fn a_section_change_waits_for_the_next_bar_then_loops_the_new_section() {
        // Kicks on every beat of bar 1, one snare at the start of bar 3.
        let mut s = kicks_at(&[0.0, 1.0, 2.0, 3.0], 100);
        s.execute(Command::CreateClip {
            track_id: 3,
            start_beats: 8.0,
            length_beats: 4.0,
            name: None,
            notes: vec![NoteInput {
                chance: 100,
                pitch: 38,
                start_beats: 0.0,
                length_beats: 0.1,
                velocity: 127,
                id: None,
            }],
        })
        .expect("snare");
        s.execute(Command::SetLoop {
            enabled: Some(true),
            start_beats: Some(0.0),
            end_beats: Some(4.0),
        })
        .expect("loop");
        let (engine, mut p) = Engine::new(s.project(), SR);
        engine.set_metronome(false);
        engine.play();
        render(&mut p, 1.25); // beat 2.5
        engine.jump_at_next_bar(8.0, 8.0, 12.0);
        render(&mut p, 0.01);
        assert_eq!(engine.status().jump_at_beats, Some(4.0));
        // Bar 1 finishes (beat 3 at 1.5 s), then bar 3 starts at 2.0 s.
        let out = left(&render(&mut p, 1.24));
        let found = onsets(&out, 0.05, 4_800);
        // Offsets from 1.26 s: the beat-3 kick at 1.5 s, the snare at 2.0 s.
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].abs_diff(11_520) <= 2, "{found:?}");
        assert!(found[1].abs_diff(35_520) <= 2, "{found:?}");
        let status = engine.status();
        assert_eq!(status.jump_at_beats, None);
        assert!(
            (8.0..9.1).contains(&status.position_beats),
            "{}",
            status.position_beats
        );
        // Bar 3 now loops: two seconds later the playhead is back in it.
        render(&mut p, 2.0);
        let at = engine.status().position_beats;
        assert!((8.0..12.0).contains(&at), "{at}");
    }

    #[test]
    fn layers_fade_out_and_back_in() {
        let starts: Vec<f64> = (0..8).map(|b| f64::from(b) * 0.5).collect();
        let mut s = kicks_at(&starts, 100);
        s.execute(Command::SetLoop {
            enabled: Some(true),
            start_beats: Some(0.0),
            end_beats: Some(4.0),
        })
        .expect("loop");
        let (engine, mut p) = Engine::new(s.project(), SR);
        engine.set_metronome(false);
        engine.play();
        let loud = peak(&render(&mut p, 1.0));
        engine.fade_layer(3, 0.0, 0.5);
        let fading = render(&mut p, 0.5);
        let silent = peak(&render(&mut p, 1.0));
        assert!(loud > 0.1, "{loud}");
        assert!(silent < 1e-4, "{silent}");
        // The fade is gradual: the first kick in it is louder than the last.
        let first = peak(&fading[..fading.len() / 4]);
        let last = peak(&fading[fading.len() * 3 / 4..]);
        assert!(first > last, "{first} vs {last}");
        engine.reset_layers();
        assert!(peak(&render(&mut p, 1.0)) > 0.1);
    }

    #[test]
    fn stopping_during_a_count_in_returns_to_where_it_would_start() {
        let (engine, mut p) = Engine::new(&Project::default(), SR);
        engine.locate(8.0);
        engine.play_with_count_in(4.0);
        render(&mut p, 0.5);
        engine.stop();
        render(&mut p, 0.01);
        let status = engine.status();
        assert!(!status.playing);
        assert_eq!(status.position_beats, 8.0);
        assert_eq!(status.count_in_beats, 0.0);
    }

    #[test]
    fn notes_played_early_in_a_count_in_land_on_the_start() {
        let (engine, mut p) = Engine::new(&Project::default(), SR);
        engine.locate(4.0);
        engine.start_recording(1);
        engine.play_with_count_in(4.0);
        render(&mut p, 1.9); // just before the song starts
        engine.note_on(1, 64, 0.8);
        render(&mut p, 0.35);
        engine.note_off(1, 64);
        render(&mut p, 0.01);
        let events = engine.stop_recording();
        assert_eq!(events.len(), 2, "{events:?}");
        assert_eq!(events[0].beat, 4.0);
        assert!((events[1].beat - 4.5).abs() < 0.02, "{}", events[1].beat);
    }

    #[test]
    fn nothing_is_recorded_while_stopped() {
        let (engine, mut p) = Engine::new(&Project::default(), SR);
        engine.start_recording(1);
        engine.note_on(1, 60, 1.0);
        render(&mut p, 0.1);
        assert!(engine.stop_recording().is_empty());
    }

    #[test]
    fn editing_clips_during_playback_takes_effect() {
        let mut s = Session::default();
        let (engine, mut p) = Engine::new(s.project(), SR);
        engine.set_metronome(false);
        engine.play();
        render(&mut p, 0.1);
        s.execute(Command::CreateClip {
            track_id: 3,
            start_beats: 1.0,
            length_beats: 1.0,
            name: None,
            notes: vec![NoteInput {
                chance: 100,
                pitch: 36,
                start_beats: 0.0,
                length_beats: 0.25,
                velocity: 127,
                id: None,
            }],
        })
        .expect("clip");
        engine.sync(s.project());
        let out = left(&render(&mut p, 1.0));
        let found = onsets(&out, 0.05, 4_800);
        assert_eq!(found.len(), 1);
        // Beat 1 is 0.5 s from the start; we had already played 0.1 s.
        assert!(found[0].abs_diff(24_000 - 4_800) <= 1, "{found:?}");
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
        engine.note_on(1, 36, 1.0);
        assert!(peak(&render(&mut p, 0.1)) > 0.05);
    }

    #[test]
    fn track_meters_report_per_track_levels() {
        let (engine, mut p) = Engine::new(&Project::default(), SR);
        engine.note_on(3, 36, 1.0);
        render(&mut p, 0.1);
        let s = engine.status();
        assert!(s.track_peaks[2] > 0.05);
        assert_eq!(s.track_peaks[0], 0.0);
    }
}
