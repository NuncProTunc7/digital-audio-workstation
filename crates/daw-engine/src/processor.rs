use std::sync::Arc;

use daw_model::TrackId;

use crate::message::{
    AudioRegionPlay, EffectChain, EngineMessage, Garbage, RecordedEvent, StripSettings, TrackSlot,
};
use crate::metronome::Metronome;
use crate::status::EngineStatus;

/// Largest chunk the processor renders at once. Sound card buffers bigger
/// than this are rendered in several chunks, so scratch buffers never grow.
pub const MAX_BLOCK_FRAMES: usize = 512;

/// Audio clips fade over at least this long at their edges, so a cut in the
/// middle of a waveform doesn't click.
const DECLICK_SECONDS: f64 = 0.0015;

/// Output stays perfectly linear below this level (about -2 dBFS).
const SOFT_CLIP_KNEE: f32 = 0.8;

/// Queues from the audio thread back to the control side.
pub(crate) struct ProcessorOutputs {
    pub garbage: rtrb::Producer<Garbage>,
    pub recorded: rtrb::Producer<RecordedEvent>,
}

/// The real-time half of the engine. Lives on the audio thread.
///
/// Everything reachable from [`process_interleaved`](Self::process_interleaved)
/// is real-time safe: no allocation, locking, I/O, or panics.
pub struct AudioProcessor {
    sample_rate_hz: f32,
    messages: rtrb::Consumer<EngineMessage>,
    outputs: ProcessorOutputs,
    status: Arc<EngineStatus>,
    tracks: Box<[TrackSlot]>,
    master_effects: Box<EffectChain>,
    master_gain: daw_dsp::Smoother,
    left: Box<[f32]>,
    right: Box<[f32]>,
    playing: bool,
    position_beats: f64,
    next_click_beat: f64,
    tempo_bpm: f64,
    beats_per_bar: u8,
    metronome_on: bool,
    metronome: Metronome,
    loop_enabled: bool,
    loop_start: f64,
    loop_end: f64,
    record_track: Option<TrackId>,
    /// When the next output buffer reaches the speakers (device clock, ns).
    output_time_ns: Option<u64>,
}

/// Initial transport and mix state for a new processor.
pub(crate) struct ProcessorInit {
    pub tracks: Box<[TrackSlot]>,
    pub master_effects: Box<EffectChain>,
    pub master_gain: f32,
    pub tempo_bpm: f64,
    pub beats_per_bar: u8,
    pub loop_enabled: bool,
    pub loop_start: f64,
    pub loop_end: f64,
}

impl AudioProcessor {
    pub(crate) fn new(
        sample_rate_hz: f32,
        messages: rtrb::Consumer<EngineMessage>,
        outputs: ProcessorOutputs,
        status: Arc<EngineStatus>,
        init: ProcessorInit,
    ) -> Self {
        Self {
            sample_rate_hz,
            messages,
            outputs,
            status,
            tracks: init.tracks,
            master_effects: init.master_effects,
            master_gain: daw_dsp::Smoother::new(init.master_gain, 0.01, sample_rate_hz),
            left: vec![0.0; MAX_BLOCK_FRAMES].into_boxed_slice(),
            right: vec![0.0; MAX_BLOCK_FRAMES].into_boxed_slice(),
            playing: false,
            position_beats: 0.0,
            next_click_beat: 0.0,
            tempo_bpm: init.tempo_bpm,
            beats_per_bar: init.beats_per_bar.max(1),
            metronome_on: true,
            metronome: Metronome::new(sample_rate_hz),
            loop_enabled: init.loop_enabled,
            loop_start: init.loop_start,
            loop_end: init.loop_end,
            record_track: None,
            output_time_ns: None,
        }
    }

    /// Tells the processor when the buffer it is about to render will be
    /// heard, in the sound card clock's nanoseconds. Recording uses this to
    /// line takes up with what the performer heard.
    // RT-SAFE
    pub fn set_output_time(&mut self, playback_ns: u64) {
        self.output_time_ns = Some(playback_ns);
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
        if let Some(ns) = self.output_time_ns.take() {
            self.status
                .publish_clock(ns, self.position_beats, self.tempo_bpm, self.playing);
        }
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
    fn throw_away(&mut self, garbage: Garbage) {
        if let Err(rtrb::PushError::Full(g)) = self.outputs.garbage.push(garbage) {
            // Never expected (the control side drains often), but leaking
            // beats freeing memory on the audio thread.
            std::mem::forget(g);
        }
    }

    // RT-SAFE
    fn record(&mut self, track_id: TrackId, note: u8, velocity: f32) {
        if self.playing && self.record_track == Some(track_id) {
            let _ = self.outputs.recorded.push(RecordedEvent {
                beat: self.position_beats,
                note,
                velocity,
            });
        }
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
                    self.record(track_id, note, velocity);
                }
                EngineMessage::NoteOff { track_id, note } => {
                    if let Some(t) = self.track(track_id) {
                        t.instrument.note_off(note);
                    }
                    self.record(track_id, note, 0.0);
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
                        t.sounding = 0;
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
                EngineMessage::SetStrip { track_id, strip } => {
                    if let Some(t) = self.track(track_id) {
                        t.strip = strip;
                    }
                }
                EngineMessage::SetMasterGain(gain) => self.master_gain.set_target(gain),
                EngineMessage::SetEffectParam {
                    track_id,
                    effect_id,
                    index,
                    value,
                } => {
                    if let Some(chain) = self.chain(track_id)
                        && let Some(e) = chain.effects.iter_mut().find(|e| e.id == effect_id)
                    {
                        e.processor.set_param(index, value);
                    }
                }
                EngineMessage::SetEffectEnabled {
                    track_id,
                    effect_id,
                    enabled,
                } => {
                    if let Some(chain) = self.chain(track_id)
                        && let Some(e) = chain.effects.iter_mut().find(|e| e.id == effect_id)
                    {
                        if enabled && !e.enabled {
                            // Don't replay a stale echo or reverb tail.
                            e.processor.reset();
                        }
                        e.enabled = enabled;
                    }
                }
                EngineMessage::ReplaceEffects { track_id, chain } => {
                    let old = match track_id {
                        None => Some(std::mem::replace(&mut self.master_effects, chain)),
                        Some(id) => self
                            .track(id)
                            .map(|t| std::mem::replace(&mut t.effects, chain)),
                    };
                    if let Some(old) = old {
                        self.throw_away(Garbage::Effects(old));
                    }
                }
                EngineMessage::ReplaceSequence { track_id, sequence } => {
                    let position = self.position_beats;
                    let old = self.track(track_id).map(|t| {
                        release_sequenced(t);
                        let old = std::mem::replace(&mut t.sequence, sequence);
                        t.cursor = first_event_at(t, position);
                        old
                    });
                    if let Some(old) = old {
                        self.throw_away(Garbage::Sequence(old));
                    }
                }
                EngineMessage::ReplaceTracks(new_tracks) => {
                    let old = std::mem::replace(&mut self.tracks, new_tracks);
                    let position = self.position_beats;
                    for t in self.tracks.iter_mut() {
                        t.cursor = first_event_at(t, position);
                    }
                    self.throw_away(Garbage::Tracks(old));
                }
                EngineMessage::Play => {
                    if !self.playing {
                        self.playing = true;
                        self.seek(self.position_beats);
                    }
                }
                EngineMessage::Stop => {
                    self.playing = false;
                    for t in self.tracks.iter_mut() {
                        release_sequenced(t);
                    }
                }
                EngineMessage::Locate(beats) => self.seek(beats.max(0.0)),
                EngineMessage::SetTempo(bpm) => {
                    self.tempo_bpm = bpm;
                    let bpm = bpm as f32;
                    for t in self.tracks.iter_mut() {
                        for e in t.effects.effects.iter_mut() {
                            e.processor.set_tempo(bpm);
                        }
                    }
                    for e in self.master_effects.effects.iter_mut() {
                        e.processor.set_tempo(bpm);
                    }
                }
                EngineMessage::SetTimeSignature { numerator, .. } => {
                    self.beats_per_bar = numerator.max(1);
                }
                EngineMessage::SetMetronome(on) => self.metronome_on = on,
                EngineMessage::SetLoop {
                    enabled,
                    start_beats,
                    end_beats,
                } => {
                    self.loop_enabled = enabled && end_beats > start_beats;
                    self.loop_start = start_beats;
                    self.loop_end = end_beats;
                }
                EngineMessage::SetRecordTrack(track) => self.record_track = track,
            }
        }
    }

    // RT-SAFE
    fn track(&mut self, id: TrackId) -> Option<&mut TrackSlot> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    // RT-SAFE
    fn chain(&mut self, track_id: Option<TrackId>) -> Option<&mut EffectChain> {
        match track_id {
            None => Some(&mut self.master_effects),
            Some(id) => self.track(id).map(|t| &mut *t.effects),
        }
    }

    /// Moves the playhead, stopping notes the sequence had started.
    // RT-SAFE
    fn seek(&mut self, beats: f64) {
        self.position_beats = beats;
        self.next_click_beat = beats.ceil();
        for t in self.tracks.iter_mut() {
            release_sequenced(t);
            t.cursor = first_event_at(t, beats);
        }
    }

    // RT-SAFE
    fn render_block(&mut self, frames: usize) {
        let frames = frames.min(MAX_BLOCK_FRAMES);
        self.left[..frames].fill(0.0);
        self.right[..frames].fill(0.0);

        let beats_per_sample = self.tempo_bpm / 60.0 / f64::from(self.sample_rate_hz);
        let mut start = 0;
        while start < frames {
            let mut end = frames;
            let looping = self.playing && self.loop_enabled && self.position_beats < self.loop_end;
            if looping {
                // Tolerate float error so an exact loop length doesn't gain
                // or lose a sample per lap.
                let to_end =
                    ((self.loop_end - self.position_beats) / beats_per_sample - 1e-6).ceil();
                end = (start + (to_end as usize).max(1)).min(frames);
            }
            self.render_segment(start, end, beats_per_sample);
            if looping && self.position_beats >= self.loop_end - 1e-9 {
                let over = self.position_beats - self.loop_end;
                self.seek(self.loop_start + over);
            }
            start = end;
        }

        let (left, right) = (&mut self.left[..frames], &mut self.right[..frames]);
        for e in self.master_effects.effects.iter_mut().filter(|e| e.enabled) {
            e.processor.process(left, right);
        }
        let mut peak_l = 0.0f32;
        let mut peak_r = 0.0f32;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let g = self.master_gain.next_value();
            *l = soft_clip(*l * g);
            *r = soft_clip(*r * g);
            peak_l = peak_l.max(l.abs());
            peak_r = peak_r.max(r.abs());
        }
        self.status.add_peaks(peak_l, peak_r);
    }

    /// Renders frames `start..end` of the block: every track (with its
    /// sequence, effects, and fader) plus the metronome.
    // RT-SAFE
    fn render_segment(&mut self, start: usize, end: usize, beats_per_sample: f64) {
        let n = end - start;
        let window_start = self.position_beats;
        let window_end = window_start + n as f64 * beats_per_sample;
        let playing = self.playing;
        let any_solo = self.tracks.iter().any(|t| t.strip.solo);

        for (index, t) in self.tracks.iter_mut().enumerate() {
            let (bl, br) = (&mut t.buf_left[..n], &mut t.buf_right[..n]);
            bl.fill(0.0);
            br.fill(0.0);

            // Play sequence events at their exact sample.
            let mut done = 0;
            if playing {
                while let Some(ev) = t.sequence.events.get(t.cursor).copied() {
                    if ev.beat >= window_end {
                        break;
                    }
                    let offset = (((ev.beat - window_start) / beats_per_sample + 1e-6).max(0.0)
                        as usize)
                        .clamp(done, n);
                    t.instrument
                        .process(&mut bl[done..offset], &mut br[done..offset]);
                    done = offset;
                    let bit = 1u128 << (ev.note & 0x7F);
                    if ev.velocity > 0.0 {
                        t.instrument.note_on(ev.note, ev.velocity);
                        t.sounding |= bit;
                    } else if t.sounding & bit != 0 {
                        t.instrument.note_off(ev.note);
                        t.sounding &= !bit;
                    }
                    t.cursor += 1;
                }
            }
            t.instrument.process(&mut bl[done..], &mut br[done..]);
            if playing {
                let declick = DECLICK_SECONDS * f64::from(self.sample_rate_hz);
                mix_audio(
                    &t.sequence.audio,
                    bl,
                    br,
                    window_start,
                    beats_per_sample,
                    declick,
                );
            }

            for e in t.effects.effects.iter_mut().filter(|e| e.enabled) {
                e.processor.process(bl, br);
            }

            let audible = !t.strip.mute && (!any_solo || t.strip.solo);
            let (pan_l, pan_r) = t.strip.pan_gains();
            let level = if audible { t.strip.gain } else { 0.0 };
            t.gain_left.set_target(level * pan_l);
            t.gain_right.set_target(level * pan_r);
            let mut peak = 0.0f32;
            for ((l, r), (ml, mr)) in bl.iter().zip(br.iter()).zip(
                self.left[start..end]
                    .iter_mut()
                    .zip(self.right[start..end].iter_mut()),
            ) {
                let (sl, sr) = (l * t.gain_left.next_value(), r * t.gain_right.next_value());
                *ml += sl;
                *mr += sr;
                peak = peak.max(sl.abs()).max(sr.abs());
            }
            self.status.add_track_peak(index, peak);
        }

        // Metronome and transport advance, sample by sample.
        for (l, r) in self.left[start..end]
            .iter_mut()
            .zip(self.right[start..end].iter_mut())
        {
            if playing {
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
            *l += click;
            *r += click;
        }
    }
}

/// Adds the audio clips that overlap this window, sample-accurately, with
/// clip gain and fades.
// RT-SAFE
fn mix_audio(
    regions: &[AudioRegionPlay],
    left: &mut [f32],
    right: &mut [f32],
    window_start: f64,
    beats_per_sample: f64,
    declick_frames: f64,
) {
    let n = left.len().min(right.len());
    let window_end = window_start + n as f64 * beats_per_sample;
    for r in regions {
        if r.end_beats <= window_start || r.start_beats >= window_end {
            continue;
        }
        let first = (((r.start_beats - window_start) / beats_per_sample).ceil()).max(0.0) as usize;
        let last = (((r.end_beats - window_start) / beats_per_sample).ceil()).max(0.0) as usize;
        let (first, last) = (first.min(n), last.min(n));
        // Frames since the clip started, at sample `first`.
        let elapsed_first = (window_start - r.start_beats) / beats_per_sample + first as f64;
        let length = (r.end_beats - r.start_beats) / beats_per_sample;
        let fade_in = r.fade_in_frames.max(declick_frames);
        let fade_out = r.fade_out_frames.max(declick_frames);
        let buf = &*r.buffer;
        for i in first..last {
            let elapsed = elapsed_first + (i - first) as f64;
            let pos = r.offset_frames + elapsed;
            if pos < 0.0 {
                continue;
            }
            let idx = pos as usize;
            let Some(&l) = buf.left.get(idx) else {
                break;
            };
            let rs = buf
                .right
                .as_deref()
                .and_then(|right| right.get(idx).copied())
                .unwrap_or(l);
            let remaining = length - elapsed;
            let mut g = f64::from(r.gain);
            if elapsed < fade_in {
                g *= (elapsed / fade_in).max(0.0);
            }
            if remaining < fade_out {
                g *= (remaining / fade_out).max(0.0);
            }
            let g = g as f32;
            if let (Some(lo), Some(ro)) = (left.get_mut(i), right.get_mut(i)) {
                *lo += l * g;
                *ro += rs * g;
            }
        }
    }
}

/// Stops every note this track's sequence started.
// RT-SAFE
fn release_sequenced(t: &mut TrackSlot) {
    let mut bits = t.sounding;
    while bits != 0 {
        let note = bits.trailing_zeros() as u8;
        t.instrument.note_off(note);
        bits &= bits - 1;
    }
    t.sounding = 0;
}

/// Index of the first sequence event at or after `beats`.
// RT-SAFE
fn first_event_at(t: &TrackSlot, beats: f64) -> usize {
    t.sequence.events.partition_point(|e| e.beat < beats)
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

/// Linear strip settings from a track's mixer.
pub(crate) fn strip_settings(mixer: &daw_model::Mixer) -> StripSettings {
    StripSettings {
        gain: db_to_fader_gain(mixer.volume_db),
        pan: mixer.pan as f32,
        mute: mixer.mute,
        solo: mixer.solo,
    }
}

/// Fader dB to linear gain; the bottom of the fader is true silence.
pub(crate) fn db_to_fader_gain(db: f64) -> f32 {
    if db <= daw_model::MIN_VOLUME_DB {
        0.0
    } else {
        10f32.powf(db as f32 / 20.0)
    }
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

    #[test]
    fn fader_bottom_is_silent() {
        assert_eq!(db_to_fader_gain(-60.0), 0.0);
        assert!((db_to_fader_gain(0.0) - 1.0).abs() < 1e-6);
        assert!((db_to_fader_gain(-6.0) - 0.501).abs() < 0.01);
    }
}
