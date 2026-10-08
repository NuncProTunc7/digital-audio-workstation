use std::sync::Arc;

use daw_model::TrackId;

use crate::message::{
    AudioRegionPlay, AutoTarget, EffectChain, EngineMessage, Garbage, PendingJump, RecordedEvent,
    StripSettings, TrackSlot, next_layer_gain,
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
    /// Where the song starts after a count-in; until the playhead gets
    /// there only clicks play. NEG_INFINITY when not counting in.
    count_in_end: f64,
    next_click_beat: f64,
    tempo_bpm: f64,
    beats_per_bar: u8,
    metronome_on: bool,
    metronome: Metronome,
    loop_enabled: bool,
    loop_start: f64,
    loop_end: f64,
    record_track: Option<TrackId>,
    /// Laps of the loop since playback started; notes with a chance below
    /// 100 % roll differently on each lap.
    loop_pass: u32,
    /// A game-preview section change waiting for its bar line.
    pending_jump: Option<PendingJump>,
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
            count_in_end: f64::NEG_INFINITY,
            next_click_beat: 0.0,
            tempo_bpm: init.tempo_bpm,
            beats_per_bar: init.beats_per_bar.max(1),
            metronome_on: true,
            metronome: Metronome::new(sample_rate_hz),
            loop_enabled: init.loop_enabled,
            loop_start: init.loop_start,
            loop_end: init.loop_end,
            record_track: None,
            loop_pass: 0,
            pending_jump: None,
            output_time_ns: None,
        }
    }

    /// Tells the processor when the buffer it is about to render will be
    /// heard, in nanoseconds on the app's shared clock (`device::clock_ns`). Recording uses this to
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
        self.status.publish(
            self.playing,
            self.song_position(),
            (self.count_in_end - self.position_beats).max(0.0),
        );
        self.status
            .publish_jump(self.pending_jump.map(|j| j.at_beats));
    }

    /// The playhead as the song sees it: during a count-in, where the song
    /// will start.
    // RT-SAFE
    fn song_position(&self) -> f64 {
        self.position_beats.max(self.count_in_end)
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
            // A note played during the count-in (jumping the gun on the
            // first downbeat) lands on the start.
            let _ = self.outputs.recorded.push(RecordedEvent {
                beat: self.song_position(),
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
                        reassert_automation(t);
                    }
                }
                EngineMessage::SetStrip { track_id, strip } => {
                    if let Some(t) = self.track(track_id) {
                        t.strip = strip;
                        reassert_automation(t);
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
                    if let Some(t) = track_id.and_then(|id| self.track(id)) {
                        reassert_automation(t);
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
                        Some(id) => self.track(id).map(|t| {
                            reassert_automation(t);
                            std::mem::replace(&mut t.effects, chain)
                        }),
                    };
                    if let Some(old) = old {
                        self.throw_away(Garbage::Effects(old));
                    }
                }
                EngineMessage::ReplaceSequence { track_id, sequence } => {
                    let position = self.song_position();
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
                    let position = self.song_position();
                    for t in self.tracks.iter_mut() {
                        t.cursor = first_event_at(t, position);
                    }
                    self.throw_away(Garbage::Tracks(old));
                }
                EngineMessage::Play => {
                    if !self.playing {
                        self.playing = true;
                        self.loop_pass = 0;
                        self.seek(self.position_beats);
                    }
                }
                EngineMessage::CountIn(beats) => {
                    if !self.playing {
                        self.playing = true;
                        self.loop_pass = 0;
                        let start = self.position_beats;
                        // Cursors wait at the start; nothing plays before it.
                        self.seek(start);
                        if beats > 0.0 && beats.is_finite() {
                            self.position_beats = start - beats;
                            self.next_click_beat = self.position_beats.ceil();
                            self.count_in_end = start;
                        }
                    }
                }
                EngineMessage::Stop => {
                    self.playing = false;
                    self.pending_jump = None;
                    for t in self.tracks.iter_mut() {
                        release_sequenced(t);
                    }
                    // Stopped during a count-in: back where it would have started.
                    if self.count_in_end.is_finite() {
                        let start = self.song_position();
                        self.count_in_end = f64::NEG_INFINITY;
                        self.seek(start);
                    }
                }
                EngineMessage::Locate(beats) => {
                    self.count_in_end = f64::NEG_INFINITY;
                    self.pending_jump = None;
                    self.seek(beats.max(0.0));
                }
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
                EngineMessage::FadeLayer {
                    track_id,
                    gain,
                    seconds,
                } => {
                    let samples = seconds.max(0.0) * self.sample_rate_hz;
                    if let Some(t) = self.track(track_id) {
                        let target = gain.clamp(0.0, 1.0);
                        t.layer_target = target;
                        if samples < 1.0 {
                            t.layer = target;
                            t.layer_step = 0.0;
                        } else {
                            t.layer_step = (target - t.layer) / samples;
                        }
                    }
                }
                EngineMessage::ResetLayers => {
                    for t in self.tracks.iter_mut() {
                        t.layer = 1.0;
                        t.layer_target = 1.0;
                        t.layer_step = 0.0;
                    }
                }
                EngineMessage::JumpAtNextBar {
                    to_beats,
                    loop_start,
                    loop_end,
                } => {
                    let bar = f64::from(self.beats_per_bar.max(1));
                    let here = self.song_position();
                    let jump = PendingJump {
                        // Strictly after the playhead: a bar that is
                        // starting right now is already under way.
                        at_beats: ((here / bar).floor() + 1.0) * bar,
                        to_beats: to_beats.max(0.0),
                        loop_start,
                        loop_end,
                    };
                    if self.playing {
                        self.pending_jump = Some(jump);
                    } else {
                        self.apply_jump(jump, 0.0);
                    }
                }
                EngineMessage::CancelJump => self.pending_jump = None,
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

    /// Makes a section change: loops the new section and moves the playhead
    /// to `to_beats` plus however far past the bar line it already is.
    // RT-SAFE
    fn apply_jump(&mut self, jump: PendingJump, over: f64) {
        self.pending_jump = None;
        self.loop_enabled = jump.loop_end > jump.loop_start;
        self.loop_start = jump.loop_start;
        self.loop_end = jump.loop_end;
        self.seek(jump.to_beats + over);
        // Like a loop wrap: notes right on the target must still play.
        for t in self.tracks.iter_mut() {
            t.cursor = first_event_at(t, jump.to_beats);
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
            let counting = self.playing && self.position_beats < self.count_in_end;
            if counting {
                // The song starts on the first sample past the count-in.
                let to_start =
                    ((self.count_in_end - self.position_beats) / beats_per_sample - 1e-6).ceil();
                end = end.min(start + (to_start as usize).max(1));
            }
            let jump = self.pending_jump.filter(|_| self.playing);
            if let Some(j) = jump
                && self.position_beats < j.at_beats
            {
                let to_jump = ((j.at_beats - self.position_beats) / beats_per_sample - 1e-6).ceil();
                end = end.min(start + (to_jump as usize).max(1));
            }
            self.render_segment(start, end, beats_per_sample);
            if counting && self.position_beats >= self.count_in_end - 1e-9 {
                self.count_in_end = f64::NEG_INFINITY;
            }
            if let Some(j) = jump
                && self.position_beats >= j.at_beats - 1e-9
            {
                let over = self.position_beats - j.at_beats;
                self.apply_jump(j, over);
            } else if let Some(j) = jump
                && looping
                && self.position_beats >= self.loop_end - 1e-9
            {
                // The section ended before the bar line: change here.
                let over = self.position_beats - self.loop_end;
                self.apply_jump(j, over);
            } else if looping && self.position_beats >= self.loop_end - 1e-9 {
                self.loop_pass = self.loop_pass.wrapping_add(1);
                let over = self.position_beats - self.loop_end;
                self.seek(self.loop_start + over);
                // The wrap lands a sliver past the loop start (a beat is
                // rarely a whole number of samples). Events in that sliver,
                // like a downbeat exactly on the loop start, must still play;
                // they fire at the top of the next segment.
                for t in self.tracks.iter_mut() {
                    t.cursor = first_event_at(t, self.loop_start);
                }
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
        // Segments never straddle the end of a count-in (see render_block).
        let counting = self.playing && window_start < self.count_in_end;
        let playing = self.playing && !counting;
        let song_start = window_start.max(self.count_in_end);
        let pass = self.loop_pass;
        let any_solo = self.tracks.iter().any(|t| t.strip.solo);

        for (index, t) in self.tracks.iter_mut().enumerate() {
            apply_automation(t, song_start);
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
                    // A skipped note never sounds, so its note-off is
                    // ignored below too.
                    let plays = chance_plays(ev.chance, t.id, ev.beat, ev.note, pass);
                    if ev.velocity > 0.0 && !plays {
                        t.cursor += 1;
                        continue;
                    }
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
                let layer = next_layer_gain(&mut t.layer, t.layer_target, &mut t.layer_step);
                let (sl, sr) = (
                    l * t.gain_left.next_value() * layer,
                    r * t.gain_right.next_value() * layer,
                );
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
            if self.playing {
                if self.position_beats >= self.next_click_beat {
                    // Count-ins can start before the song (negative beats).
                    let beat_in_bar =
                        (self.next_click_beat as i64).rem_euclid(i64::from(self.beats_per_bar));
                    if self.metronome_on || self.position_beats < self.count_in_end {
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

/// Whether a note with `chance` percent plays this time. A pure function of
/// the note's place and the loop lap, so a song plays and renders the same
/// way every time while repeats of a pattern still vary.
// RT-SAFE
fn chance_plays(chance: u8, track: TrackId, beat: f64, note: u8, pass: u32) -> bool {
    if chance >= 100 {
        return true;
    }
    // splitmix64 over the note's identity.
    let mut z = beat.to_bits()
        ^ (u64::from(note) << 56)
        ^ (u64::from(track) << 32)
        ^ u64::from(pass).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    z % 100 < u64::from(chance)
}

/// Makes automation re-apply its values on the next block, after a manual
/// change (a slider, a preset, a new effect chain) overwrote them.
// RT-SAFE
fn reassert_automation(t: &mut TrackSlot) {
    for c in t.sequence.automation.iter_mut() {
        c.last = f32::NAN;
    }
}

/// Sets every automated value to its curve's value at `beats`. Automation
/// follows the playhead whether or not the song is playing.
// RT-SAFE
fn apply_automation(t: &mut TrackSlot, beats: f64) {
    for curve in t.sequence.automation.iter_mut() {
        let Some(v) = curve.value_at(beats) else {
            continue;
        };
        if v == curve.last {
            continue;
        }
        curve.last = v;
        match curve.target {
            AutoTarget::Volume => t.strip.gain = db_to_fader_gain(f64::from(v)),
            AutoTarget::Pan => t.strip.pan = v.clamp(-1.0, 1.0),
            AutoTarget::Instrument(index) => t.instrument.set_param(index, v),
            AutoTarget::Effect { effect_id, index } => {
                if let Some(e) = t.effects.effects.iter_mut().find(|e| e.id == effect_id) {
                    e.processor.set_param(index, v);
                }
            }
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
