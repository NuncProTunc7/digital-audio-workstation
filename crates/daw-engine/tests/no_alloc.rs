//! Proves the audio thread never allocates or frees memory.
//!
//! Memory allocation can block for an unpredictable time, which shows up as
//! clicks and dropouts. This test installs a counting allocator and fails if
//! `process_interleaved` touches the heap, across every kind of message.

// A global allocator is inherently unsafe; this file is test-only.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

use daw_engine::{AudioPool, Engine, EngineMessage};
use daw_model::{
    AudioRegion, Command, EffectKind, Instrument, InstrumentKind, NoteInput, Project, Session,
};

struct CountingAllocator;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    // Only count on the thread under test, inside the measured region.
    static COUNTING: Cell<bool> = const { Cell::new(false) };
}

fn note(event: &str) {
    if COUNTING.with(Cell::get) {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // Stop counting so a failure message can't recurse.
        COUNTING.with(|c| c.set(false));
        let _ = event;
    }
}

// SAFETY: forwards every call unchanged to the system allocator; the counter
// is a lock-free atomic and the thread-local flag is a const-initialized Cell.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note("alloc");
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note("dealloc");
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note("realloc");
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn process_counting(processor: &mut daw_engine::AudioProcessor, out: &mut [f32]) -> usize {
    ALLOCATIONS.store(0, Ordering::Relaxed);
    COUNTING.with(|c| c.set(true));
    for chunk in out.chunks_mut(1024) {
        processor.process_interleaved(chunk, 2);
    }
    COUNTING.with(|c| c.set(false));
    ALLOCATIONS.load(Ordering::Relaxed)
}

#[test]
fn audio_thread_never_allocates() {
    let mut session = Session::new(Project::default());
    // An audio track with a recording on it, playing alongside everything.
    let pool = AudioPool::in_temp_dir();
    pool.insert(
        "take.wav",
        daw_audio::AudioData {
            sample_rate_hz: 44_100,
            channels: vec![vec![0.25; 44_100], vec![-0.25; 44_100]],
        },
    );
    session
        .execute(Command::AddTrack {
            name: "Vox".into(),
            instrument: InstrumentKind::Audio,
            preset: None,
            index: None,
        })
        .expect("audio track");
    let audio_track = session.project().tracks[3].id;
    session
        .execute(Command::AddAudioClip {
            track_id: audio_track,
            start_beats: 0.0,
            audio: AudioRegion {
                file: "take.wav".into(),
                file_seconds: 1.0,
                offset_seconds: 0.1,
                gain_db: -3.0,
                fade_in_seconds: 0.05,
                fade_out_seconds: 0.2,
                source_bpm: None,
            },
            length_beats: Some(1.5),
            name: None,
        })
        .expect("audio clip");
    // Automation on volume, pan, an instrument parameter, and (below) an effect.
    for (track, target) in [
        (1, daw_model::AutomationTarget::Volume),
        (2, daw_model::AutomationTarget::Pan),
        (
            1,
            daw_model::AutomationTarget::InstrumentParam {
                param: "filter.cutoff_hz".into(),
            },
        ),
    ] {
        let value = if matches!(target, daw_model::AutomationTarget::InstrumentParam { .. }) {
            1000.0
        } else {
            -0.5
        };
        session
            .execute(Command::AddAutomationLane {
                track_id: track,
                target,
                points: vec![
                    daw_model::AutomationPoint { beats: 0.0, value },
                    daw_model::AutomationPoint {
                        beats: 4.0,
                        value: value * 2.0,
                    },
                ],
            })
            .expect("lane");
    }
    // A bus with an effect that the drums play into and the keys send to.
    session
        .execute(Command::AddBus {
            name: "Room".into(),
        })
        .expect("bus");
    let bus = session.project().buses[0].id;
    session
        .execute(Command::AddEffect {
            track_id: Some(bus),
            kind: daw_model::EffectKind::Reverb,
            index: None,
        })
        .expect("bus reverb");
    session
        .execute(Command::SetTrackOutput {
            track_id: 3,
            bus_id: Some(bus),
        })
        .expect("route");
    // The bass compressor listens to the drums (sidechain).
    session
        .execute(Command::AddEffect {
            track_id: Some(2),
            kind: daw_model::EffectKind::Compressor,
            index: None,
        })
        .expect("comp");
    let comp = session.project().tracks[1].mixer.effects[0].id;
    session
        .execute(Command::SetEffectSidechain {
            track_id: Some(2),
            effect_id: comp,
            source: Some(3),
        })
        .expect("sidechain");
    for pre_fader in [false, true] {
        session
            .execute(Command::SetSend {
                track_id: if pre_fader { 2 } else { 1 },
                bus_id: bus,
                level_db: Some(-3.0),
                pre_fader: Some(pre_fader),
            })
            .expect("send");
    }
    let (engine, mut processor) = Engine::with_audio(session.project(), 48_000, pool);
    // The sound card reports when each buffer will be heard.
    processor.set_output_time(1_000_000_000);
    // Buffer sized larger than MAX_BLOCK_FRAMES to exercise chunking.
    let mut out = vec![0.0f32; 48_000 * 2];

    // Notes on every track, transport running, metronome on.
    engine.play();
    for track in 1..=3 {
        for n in 0..20 {
            engine.note_on(track, 36 + n, 0.9);
        }
    }
    assert_eq!(
        process_counting(&mut processor, &mut out),
        0,
        "notes + metronome"
    );

    // Parameter changes, pedal, pitch bend, note-offs, stop.
    session
        .execute(Command::LoadPreset {
            track_id: 1,
            preset: "Acid Bass".into(),
        })
        .expect("preset");
    engine.sync(session.project());
    engine.send(EngineMessage::ControlChange {
        track_id: 1,
        controller: 64,
        value: 127,
    });
    engine.send(EngineMessage::PitchBend {
        track_id: 1,
        semitones: 1.0,
    });
    for n in 0..20 {
        engine.note_off(1, 36 + n);
    }
    engine.stop();
    assert_eq!(
        process_counting(&mut processor, &mut out),
        0,
        "params + controls"
    );

    // A count-in (clicks, then the song) while recording notes, stopped
    // once and run through to the song.
    engine.locate(2.0);
    engine.start_recording(1);
    engine.play_with_count_in(4.0);
    engine.note_on(1, 60, 0.8);
    engine.stop();
    engine.play_with_count_in(1.0);
    assert_eq!(process_counting(&mut processor, &mut out), 0, "count-in");

    // Bus and send levels change live.
    session
        .execute(Command::SetBusMixer {
            bus_id: bus,
            volume_db: Some(-4.0),
            pan: Some(0.3),
            mute: None,
        })
        .expect("bus level");
    session
        .execute(Command::SetSend {
            track_id: 1,
            bus_id: bus,
            level_db: Some(-9.0),
            pre_fader: None,
        })
        .expect("send level");
    engine.sync(session.project());
    assert_eq!(process_counting(&mut processor, &mut out), 0, "bus levels");

    // Game preview: layer fades and a section change at the next bar.
    engine.fade_layer(1, 0.2, 0.3);
    engine.fade_layer(2, 0.0, 0.0);
    engine.jump_at_next_bar(8.0, 8.0, 12.0);
    assert_eq!(
        process_counting(&mut processor, &mut out),
        0,
        "game preview"
    );
    engine.jump_at_next_bar(0.0, 0.0, 4.0);
    engine.cancel_jump();
    engine.audition_seam(0.0, 4.0, 1.0);
    assert_eq!(
        process_counting(&mut processor, &mut out),
        0,
        "seam audition"
    );
    engine.reset_layers();
    assert_eq!(
        process_counting(&mut processor, &mut out),
        0,
        "preview reset"
    );
    engine.note_off(1, 60);
    engine.stop_recording();
    engine.stop();

    // Arrangement playback: clips, effects on tracks and master, a loop that
    // wraps several times, live recording, and edits while playing.
    session
        .execute(Command::CreateClip {
            track_id: 3,
            start_beats: 0.0,
            length_beats: 2.0,
            name: None,
            notes: (0..8)
                .map(|i| NoteInput {
                    // Some notes roll a chance each lap.
                    chance: if i % 2 == 0 { 50 } else { 100 },
                    pitch: 36 + (i % 4) * 2,
                    start_beats: f64::from(i) * 0.25,
                    length_beats: 0.2,
                    velocity: 100,
                    id: None,
                })
                .collect(),
        })
        .expect("clip");
    for (track, kind) in [
        (Some(1), EffectKind::Reverb),
        (Some(1), EffectKind::Delay),
        (Some(3), EffectKind::Compressor),
        (Some(3), EffectKind::Eq),
        (Some(2), EffectKind::Chorus),
        (Some(2), EffectKind::Distortion),
        (None, EffectKind::Limiter),
    ] {
        session
            .execute(Command::AddEffect {
                track_id: track,
                kind,
                index: None,
            })
            .expect("fx");
    }
    session
        .execute(Command::SetLoop {
            enabled: Some(true),
            start_beats: Some(0.0),
            end_beats: Some(2.0),
        })
        .expect("loop");
    engine.sync(session.project());
    engine.start_recording(1);
    engine.play();
    for n in 0..30 {
        engine.note_on(1, 60 + n, 0.7);
    }
    assert_eq!(
        process_counting(&mut processor, &mut out),
        0,
        "arrangement + loop"
    );

    // Edits while playing: new sequence, new chain, mixer and effect params.
    session
        .execute(Command::TransposeNotes {
            clip_id: session.project().tracks[2].clips[0].id,
            semitones: 1,
            note_ids: None,
        })
        .expect("transpose");
    let fx = session.project().tracks[0].mixer.effects[0].id;
    session
        .execute(Command::SetEffectParam {
            track_id: Some(1),
            effect_id: fx,
            param: "mix".into(),
            value: 0.9,
        })
        .expect("param");
    session
        .execute(Command::SetTrackMixer {
            track_id: 2,
            volume_db: Some(-12.0),
            pan: Some(0.5),
            mute: None,
            solo: Some(true),
        })
        .expect("mixer");
    session
        .execute(Command::AddEffect {
            track_id: Some(3),
            kind: EffectKind::Reverb,
            index: Some(0),
        })
        .expect("chain swap");
    session
        .execute(Command::SetMasterVolume { volume_db: -3.0 })
        .expect("master");
    let reverb = session.project().tracks[0].mixer.effects[0].id;
    session
        .execute(Command::AddAutomationLane {
            track_id: 1,
            target: daw_model::AutomationTarget::EffectParam {
                effect_id: reverb,
                param: "mix".into(),
            },
            points: vec![
                daw_model::AutomationPoint {
                    beats: 0.0,
                    value: 0.1,
                },
                daw_model::AutomationPoint {
                    beats: 2.0,
                    value: 0.6,
                },
            ],
        })
        .expect("fx lane");
    engine.sync(session.project());
    engine.locate(1.0);
    assert_eq!(process_counting(&mut processor, &mut out), 0, "live edits");
    let _ = engine.stop_recording();

    // Audio clip edits while playing swap in a new sequence (with its
    // shared buffer); the old one must go back, not be freed here.
    let audio_clip = session.project().tracks[3].clips[0].id;
    session
        .execute(Command::SetAudioClip {
            clip_id: audio_clip,
            gain_db: Some(-9.0),
            fade_in_seconds: None,
            fade_out_seconds: Some(0.0),
        })
        .expect("audio gain");
    session
        .execute(Command::SplitClip {
            clip_id: audio_clip,
            at_beats: 0.75,
        })
        .expect("split");
    engine.sync(session.project());
    engine.play();
    processor.set_output_time(2_000_000_000);
    assert_eq!(process_counting(&mut processor, &mut out), 0, "audio edits");

    // A sampler playing a (tiny) sample pack, with the pedal and pitch bend.
    let dir = tempfile::tempdir().expect("tmp");
    let tone: Vec<f32> = (0..24_000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    daw_audio::write_wav(
        &dir.path().join("c4.wav"),
        &daw_audio::AudioData {
            sample_rate_hz: 44_100,
            channels: vec![tone.clone(), tone],
        },
    )
    .expect("wav");
    let sfz = dir.path().join("pack.sfz");
    std::fs::write(&sfz, "<region> sample=c4.wav lokey=0 hikey=127 pitch_keycenter=60 loop_mode=loop_continuous loop_start=100 loop_end=20000\n").expect("sfz");
    session
        .execute(Command::AddTrack {
            name: "Piano".into(),
            instrument: InstrumentKind::Sampler,
            preset: None,
            index: None,
        })
        .expect("sampler");
    let piano = session.project().tracks.last().expect("piano").id;
    session
        .execute(Command::LoadSamplePack {
            track_id: piano,
            path: Some(sfz.display().to_string()),
        })
        .expect("pack");
    engine.sync(session.project());
    let start = std::time::Instant::now();
    while !matches!(
        daw_sampler::pack_status(&sfz),
        daw_sampler::PackStatus::Ready { .. }
    ) {
        assert!(start.elapsed().as_secs() < 10, "pack never loaded");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // Let the processor take the new tracks (and hand the old ones back).
    process_counting(&mut processor, &mut out);
    engine.play();
    engine.send(EngineMessage::ControlChange {
        track_id: piano,
        controller: 64,
        value: 127,
    });
    for n in 0..60 {
        engine.note_on(piano, 30 + n, 0.8);
    }
    engine.send(EngineMessage::PitchBend {
        track_id: piano,
        semitones: -2.0,
    });
    for n in 0..60 {
        engine.note_off(piano, 30 + n);
    }
    engine.send(EngineMessage::ControlChange {
        track_id: piano,
        controller: 64,
        value: 0,
    });
    assert_eq!(process_counting(&mut processor, &mut out), 0, "sampler");

    // A plugin instrument: notes and parameter changes.
    let path = "in-process://no-alloc/NPT Test.vst3";
    // SAFETY: GetPluginFactory returns an owned factory reference.
    let module = unsafe {
        daw_plugins::Module::from_factory(
            daw_test_plugin::GetPluginFactory(),
            std::path::Path::new(path),
        )
    }
    .expect("factory");
    std::mem::forget(module);
    session
        .execute(Command::AddTrack {
            name: "Strings".into(),
            instrument: InstrumentKind::Synth,
            preset: None,
            index: None,
        })
        .expect("track");
    let strings = session.project().tracks.last().expect("strings").id;
    session
        .execute(Command::SetInstrument {
            track_id: strings,
            instrument: Instrument {
                kind: InstrumentKind::Plugin,
                preset: String::new(),
                params: Default::default(),
                sample_pack: None,
                plugin: Some(Box::new(daw_model::plugin::PluginRef {
                    uid: "4E50543153594E544800000000000001".into(),
                    name: "NPT Test Synth".into(),
                    vendor: "Nunc Pro Tune".into(),
                    path: path.into(),
                    params: [(0, 0.5)].into_iter().collect(),
                    state: None,
                })),
            },
        })
        .expect("plugin");
    engine.sync(session.project());
    process_counting(&mut processor, &mut out);
    for n in 0..40 {
        engine.note_on(strings, 40 + n, 0.7);
    }
    session
        .execute(Command::SetPluginParams {
            track_id: strings,
            effect_id: None,
            params: [(0, 0.9)].into_iter().collect(),
        })
        .expect("param");
    engine.sync(session.project());
    engine.send(EngineMessage::SetTempo(97.0));
    for n in 0..40 {
        engine.note_off(strings, 40 + n);
    }
    assert_eq!(process_counting(&mut processor, &mut out), 0, "plugin");

    // Swapping the whole track set: the old set must be handed back, not freed.
    session
        .execute(Command::SetInstrument {
            track_id: 2,
            instrument: Instrument::from_preset(InstrumentKind::Drums, "Tight Kit").expect("kit"),
        })
        .expect("swap");
    engine.sync(session.project());
    assert_eq!(process_counting(&mut processor, &mut out), 0, "track swap");
}
