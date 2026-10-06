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

use daw_engine::{Engine, EngineMessage};
use daw_model::{Command, EffectKind, Instrument, InstrumentKind, NoteInput, Project, Session};

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
    let (engine, mut processor) = Engine::new(session.project(), 48_000);
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
    engine.sync(session.project());
    engine.locate(1.0);
    assert_eq!(process_counting(&mut processor, &mut out), 0, "live edits");
    let _ = engine.stop_recording();

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
