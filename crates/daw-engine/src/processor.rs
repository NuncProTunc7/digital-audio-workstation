use std::sync::Arc;

use crate::message::{EngineMessage, TrackSlot};
use crate::metronome::Metronome;
use crate::status::EngineStatus;

/// Largest chunk the processor renders at once. Sound card buffers bigger
/// than this are rendered in several chunks, so scratch buffers never grow.
pub const MAX_BLOCK_FRAMES: usize = 512;

/// Output stays perfectly linear below this level (about -2 dBFS).
const SOFT_CLIP_KNEE: f32 = 0.8;

/// The real-time half of the engine. Lives on the audio thread.
///
/// Everything reachable from [`process_interleaved`](Self::process_interleaved)
/// is real-time safe: no allocation, locking, I/O, or panics.
pub struct AudioProcessor {
    sample_rate_hz: f32,
    messages: rtrb::Consumer<EngineMessage>,
    garbage: rtrb::Producer<Box<[TrackSlot]>>,
    status: Arc<EngineStatus>,
    tracks: Box<[TrackSlot]>,
    left: Box<[f32]>,
    right: Box<[f32]>,
    playing: bool,
    position_beats: f64,
    next_click_beat: f64,
    tempo_bpm: f64,
    beats_per_bar: u8,
    metronome_on: bool,
    metronome: Metronome,
}

impl AudioProcessor {
    pub(crate) fn new(
        sample_rate_hz: f32,
        messages: rtrb::Consumer<EngineMessage>,
        garbage: rtrb::Producer<Box<[TrackSlot]>>,
        status: Arc<EngineStatus>,
        tracks: Box<[TrackSlot]>,
        tempo_bpm: f64,
        beats_per_bar: u8,
    ) -> Self {
        Self {
            sample_rate_hz,
            messages,
            garbage,
            status,
            tracks,
            left: vec![0.0; MAX_BLOCK_FRAMES].into_boxed_slice(),
            right: vec![0.0; MAX_BLOCK_FRAMES].into_boxed_slice(),
            playing: false,
            position_beats: 0.0,
            next_click_beat: 0.0,
            tempo_bpm,
            beats_per_bar: beats_per_bar.max(1),
            metronome_on: true,
            metronome: Metronome::new(sample_rate_hz),
        }
    }

    pub fn sample_rate_hz(&self) -> f32 {
        self.sample_rate_hz
    }

    /// Shared status, for device code that reports CPU load.
    pub fn status_handle(&self) -> Arc<EngineStatus> {
        Arc::clone(&self.status)
    }

    /// Fills an interleaved output buffer with `channels` channels.
    /// Channels beyond the first two are left silent.
    // RT-SAFE
    pub fn process_interleaved(&mut self, out: &mut [f32], channels: usize) {
        let channels = channels.max(1);
        self.handle_messages();
        for chunk in out.chunks_mut(MAX_BLOCK_FRAMES * channels) {
            let frames = chunk.len() / channels;
            self.render_block(frames);
            let (left, right) = (&self.left[..frames], &self.right[..frames]);
            for ((frame, &l), &r) in chunk.chunks_mut(channels).zip(left).zip(right) {
                match frame {
                    [mono] => *mono = 0.5 * (l + r),
                    [fl, fr, rest @ ..] => {
                        *fl = l;
                        *fr = r;
                        rest.fill(0.0);
                    }
                    [] => {}
                }
            }
        }
        self.status.publish(self.playing, self.position_beats);
    }

    // RT-SAFE
    fn handle_messages(&mut self) {
        while let Ok(message) = self.messages.pop() {
            match message {
                EngineMessage::NoteOn {
                    track_id,
                    note,
                    velocity,
                } => {
                    if let Some(t) = self.track(track_id) {
                        t.instrument.note_on(note, velocity);
                    }
                }
                EngineMessage::NoteOff { track_id, note } => {
                    if let Some(t) = self.track(track_id) {
                        t.instrument.note_off(note);
                    }
                }
                EngineMessage::ControlChange {
                    track_id,
                    controller,
                    value,
                } => {
                    if let Some(t) = self.track(track_id) {
                        t.instrument.control_change(controller, value);
                    }
                }
                EngineMessage::PitchBend {
                    track_id,
                    semitones,
                } => {
                    if let Some(t) = self.track(track_id) {
                        t.instrument.pitch_bend(semitones);
                    }
                }
                EngineMessage::AllNotesOff => {
                    for t in self.tracks.iter_mut() {
                        t.instrument.all_notes_off();
                    }
                }
                EngineMessage::SetParam {
                    track_id,
                    index,
                    value,
                } => {
                    if let Some(t) = self.track(track_id) {
                        t.instrument.set_param(index, value);
                    }
                }
                EngineMessage::Play => {
                    if !self.playing {
                        self.playing = true;
                        self.next_click_beat = self.position_beats.ceil();
                    }
                }
                EngineMessage::Stop => self.playing = false,
                EngineMessage::Locate(beats) => {
                    self.position_beats = beats.max(0.0);
                    self.next_click_beat = self.position_beats.ceil();
                }
                EngineMessage::SetTempo(bpm) => self.tempo_bpm = bpm,
                EngineMessage::SetTimeSignature { numerator, .. } => {
                    self.beats_per_bar = numerator.max(1);
                }
                EngineMessage::SetMetronome(on) => self.metronome_on = on,
                EngineMessage::ReplaceTracks(new_tracks) => {
                    let old = std::mem::replace(&mut self.tracks, new_tracks);
                    if let Err(rtrb::PushError::Full(old)) = self.garbage.push(old) {
                        // Never expected (the control side drains often), but
                        // leaking beats freeing memory on the audio thread.
                        std::mem::forget(old);
                    }
                }
            }
        }
    }

    // RT-SAFE
    fn track(&mut self, id: daw_model::TrackId) -> Option<&mut TrackSlot> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    // RT-SAFE
    fn render_block(&mut self, frames: usize) {
        let frames = frames.min(MAX_BLOCK_FRAMES);
        let (left, right) = (&mut self.left[..frames], &mut self.right[..frames]);
        left.fill(0.0);
        right.fill(0.0);
        for t in self.tracks.iter_mut() {
            t.instrument.process(left, right);
        }

        let beats_per_sample = self.tempo_bpm / 60.0 / f64::from(self.sample_rate_hz);
        let mut peak_l = 0.0f32;
        let mut peak_r = 0.0f32;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            if self.playing {
                if self.position_beats >= self.next_click_beat {
                    let beat_in_bar = (self.next_click_beat as u64) % u64::from(self.beats_per_bar);
                    if self.metronome_on {
                        self.metronome.trigger(beat_in_bar == 0);
                    }
                    self.next_click_beat += 1.0;
                }
                self.position_beats += beats_per_sample;
            }
            let click = self.metronome.next();
            *l = soft_clip(*l + click);
            *r = soft_clip(*r + click);
            peak_l = peak_l.max(l.abs());
            peak_r = peak_r.max(r.abs());
        }
        self.status.add_peaks(peak_l, peak_r);
    }
}

/// Transparent below the knee, then rounds off smoothly so that a sudden
/// overload never hits the speakers as harsh digital clipping.
// RT-SAFE
fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= SOFT_CLIP_KNEE || !a.is_finite() {
        return if x.is_finite() { x } else { 0.0 };
    }
    let range = 1.0 - SOFT_CLIP_KNEE;
    x.signum() * (SOFT_CLIP_KNEE + range * ((a - SOFT_CLIP_KNEE) / range).tanh())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_clip_is_transparent_below_knee_and_bounded_above() {
        assert_eq!(soft_clip(0.5), 0.5);
        assert_eq!(soft_clip(-0.8), -0.8);
        assert!(soft_clip(10.0) <= 1.0);
        assert!(soft_clip(-10.0) >= -1.0);
        assert_eq!(soft_clip(f32::NAN), 0.0);
        assert_eq!(soft_clip(f32::INFINITY), 0.0);
    }

    #[test]
    fn soft_clip_is_continuous_at_knee() {
        let below = soft_clip(SOFT_CLIP_KNEE);
        let above = soft_clip(SOFT_CLIP_KNEE + 1e-4);
        assert!((above - below).abs() < 2e-4);
    }
}
