//! Standard MIDI Files (`.mid`), type 1, via midly.

use std::collections::BTreeMap;

use daw_model::{InstrumentKind, MIN_LENGTH_BEATS, NoteInput, Project, TimeSignature, Track};
use midly::num::{u4, u7, u15, u24, u28};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};

use crate::{ImportedPart, ImportedSong, NotationError, quarters_per_beat, whole_bars};

/// Ticks per quarter note in exported files.
const PPQ: u16 = 480;
/// MIDI channel 10 (index 9) is drums in General MIDI.
const DRUM_CHANNEL: u8 = 9;

/// General MIDI program that best matches a synth preset, so the file
/// sounds sensible in other apps.
fn gm_program(preset: &str) -> u8 {
    let p = preset.to_ascii_lowercase();
    if p.contains("bass") {
        38 // Synth Bass 1
    } else if p.contains("square") || p.contains("chip") {
        80 // Lead 1 (square)
    } else if p.contains("lead") {
        81 // Lead 2 (sawtooth)
    } else if p.contains("pad") {
        89 // Pad 2 (warm)
    } else if p.contains("brass") {
        62 // Synth Brass 1
    } else if p.contains("pluck") {
        45 // Pizzicato Strings
    } else {
        4 // Electric Piano 1
    }
}

/// Our synth preset closest to a General MIDI program.
fn preset_for_program(program: u8) -> &'static str {
    match program {
        32..=39 => "Fat Bass",
        80 => "Chip Square",
        81..=87 => "Bright Lead",
        88..=95 | 48..=55 => "Soft Pad",
        56..=63 => "Brass Stab",
        24..=31 | 45..=47 => "Pluck",
        _ => "Warm Keys",
    }
}

/// A note as absolute ticks, for sorting into a track.
struct Timed {
    tick: u64,
    /// Note-offs sort before note-ons at the same tick.
    on: bool,
    key: u8,
    velocity: u8,
}

/// Every note a track plays, cut at clip ends (as the engine plays them),
/// in beats from the song start.
fn played_notes(track: &Track) -> Vec<(f64, f64, u8, u8)> {
    let mut out = Vec::new();
    for clip in &track.clips {
        let end = clip.start_beats + clip.length_beats;
        for n in &clip.notes {
            if n.start_beats >= clip.length_beats {
                continue;
            }
            let start = clip.start_beats + n.start_beats;
            out.push((
                start,
                (start + n.length_beats).min(end),
                n.pitch,
                n.velocity,
            ));
        }
    }
    out
}

/// The song as a type-1 MIDI file: a tempo track, then one track per
/// instrument track (audio tracks are skipped). Drums use channel 10.
pub fn export_midi(project: &Project) -> Vec<u8> {
    let ts = project.time_signature;
    let ticks_per_beat = f64::from(PPQ) * quarters_per_beat(ts);
    let to_tick = |beats: f64| (beats.max(0.0) * ticks_per_beat).round() as u64;

    let mut tracks: Vec<Vec<TrackEvent<'_>>> = Vec::new();
    let quarter_bpm = project.tempo_bpm * quarters_per_beat(ts);
    let us_per_quarter = (60_000_000.0 / quarter_bpm)
        .round()
        .clamp(1.0, 16_777_215.0) as u32;
    let meta = |m| TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(m),
    };
    tracks.push(vec![
        meta(MetaMessage::TrackName(project.name.as_bytes())),
        meta(MetaMessage::Tempo(u24::new(us_per_quarter))),
        meta(MetaMessage::TimeSignature(
            ts.numerator,
            ts.denominator.max(1).trailing_zeros() as u8,
            24,
            8,
        )),
        meta(MetaMessage::EndOfTrack),
    ]);

    let mut next_channel = 0u8;
    for track in project
        .tracks
        .iter()
        .filter(|t| !t.instrument.kind.is_audio())
    {
        let drums = track.instrument.kind == InstrumentKind::Drums;
        let channel = if drums {
            DRUM_CHANNEL
        } else {
            let c = next_channel;
            next_channel = (next_channel + 1) % 16;
            if next_channel == DRUM_CHANNEL {
                next_channel += 1;
            }
            c
        };
        let mut timed: Vec<Timed> = Vec::new();
        for (start, end, key, velocity) in played_notes(track) {
            let (on, off) = (to_tick(start), to_tick(end));
            let key = key.min(127);
            timed.push(Timed {
                tick: on,
                on: true,
                key,
                velocity: velocity.clamp(1, 127),
            });
            timed.push(Timed {
                tick: off.max(on + 1),
                on: false,
                key,
                velocity: 0,
            });
        }
        timed.sort_by_key(|t| (t.tick, t.on));

        let ch = u4::new(channel);
        let mut events = vec![meta(MetaMessage::TrackName(track.name.as_bytes()))];
        if !drums {
            events.push(TrackEvent {
                delta: u28::new(0),
                kind: TrackEventKind::Midi {
                    channel: ch,
                    message: MidiMessage::ProgramChange {
                        program: u7::new(gm_program(&track.instrument.preset)),
                    },
                },
            });
        }
        let mut last = 0u64;
        for t in timed {
            let delta = u28::new((t.tick - last).min(0x0FFF_FFFF) as u32);
            last = t.tick;
            let message = if t.on {
                MidiMessage::NoteOn {
                    key: u7::new(t.key),
                    vel: u7::new(t.velocity),
                }
            } else {
                MidiMessage::NoteOff {
                    key: u7::new(t.key),
                    vel: u7::new(64),
                }
            };
            events.push(TrackEvent {
                delta,
                kind: TrackEventKind::Midi {
                    channel: ch,
                    message,
                },
            });
        }
        events.push(meta(MetaMessage::EndOfTrack));
        tracks.push(events);
    }

    let smf = Smf {
        header: Header::new(Format::Parallel, Timing::Metrical(u15::new(PPQ))),
        tracks,
    };
    let mut out = Vec::new();
    // Writing to a Vec can't fail.
    let _ = smf.write_std(&mut out);
    out
}

/// Notes collected for one part while reading.
#[derive(Default)]
struct PartBuilder {
    program: Option<u8>,
    /// (start tick, end tick, key, velocity)
    notes: Vec<(u64, u64, u8, u8)>,
}

/// Reads a MIDI file. Each (track, channel) pair with notes becomes a part;
/// channel 10 parts are drums. Uses the first tempo and time signature.
pub fn import_midi(bytes: &[u8]) -> Result<ImportedSong, NotationError> {
    let smf = Smf::parse(bytes).map_err(|e| NotationError::BadMidi(e.to_string()))?;
    let ppq = match smf.header.timing {
        Timing::Metrical(t) => f64::from(t.as_int().max(1)),
        Timing::Timecode(..) => return Err(NotationError::SmpteTiming),
    };
    let mut us_per_quarter: Option<u32> = None;
    let mut time_signature: Option<TimeSignature> = None;
    let mut title: Option<String> = None;
    // Keyed by (track index, channel) so parts keep the file's order.
    let mut parts: BTreeMap<(usize, u8), PartBuilder> = BTreeMap::new();
    let mut track_names: Vec<Option<String>> = Vec::new();

    for (ti, track) in smf.tracks.iter().enumerate() {
        let mut tick = 0u64;
        let mut name = None;
        // Sounding notes: (channel, key) -> start ticks with velocities.
        let mut open: BTreeMap<(u8, u8), Vec<(u64, u8)>> = BTreeMap::new();
        for ev in track {
            tick += u64::from(ev.delta.as_int());
            match ev.kind {
                TrackEventKind::Meta(MetaMessage::Tempo(t)) if us_per_quarter.is_none() => {
                    us_per_quarter = Some(t.as_int());
                }
                TrackEventKind::Meta(MetaMessage::TimeSignature(n, d, ..))
                    if time_signature.is_none() =>
                {
                    time_signature = Some(TimeSignature {
                        numerator: n.max(1),
                        denominator: 1u8.checked_shl(u32::from(d)).unwrap_or(4),
                    });
                }
                TrackEventKind::Meta(MetaMessage::TrackName(n)) if name.is_none() => {
                    let s = String::from_utf8_lossy(n).trim().to_owned();
                    if !s.is_empty() {
                        name = Some(s);
                    }
                }
                TrackEventKind::Midi { channel, message } => {
                    let ch = channel.as_int();
                    match message {
                        MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                            open.entry((ch, key.as_int()))
                                .or_default()
                                .push((tick, vel.as_int()));
                        }
                        MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                            if let Some(starts) = open.get_mut(&(ch, key.as_int()))
                                && !starts.is_empty()
                            {
                                let (start, vel) = starts.remove(0);
                                parts.entry((ti, ch)).or_default().notes.push((
                                    start,
                                    tick,
                                    key.as_int(),
                                    vel,
                                ));
                            }
                        }
                        MidiMessage::ProgramChange { program } => {
                            parts.entry((ti, ch)).or_default().program = Some(program.as_int());
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        // Notes never released end where the track ends.
        for ((ch, key), starts) in open {
            for (start, vel) in starts {
                parts.entry((ti, ch)).or_default().notes.push((
                    start,
                    tick.max(start + 1),
                    key,
                    vel,
                ));
            }
        }
        if ti == 0 && smf.tracks.len() > 1 && !parts.keys().any(|k| k.0 == 0) {
            // In type-1 files the first track's name is the song title.
            title = name.clone();
        }
        track_names.push(name);
    }

    let ts = time_signature.unwrap_or_default();
    let beats_per_tick = 1.0 / ppq / quarters_per_beat(ts);
    let beats_per_bar = f64::from(ts.numerator.max(1));
    let channels_per_track = |ti: usize| parts.keys().filter(|k| k.0 == ti).count();
    let mut out = Vec::new();
    for (&(ti, ch), part) in &parts {
        if part.notes.is_empty() {
            continue;
        }
        let drums = ch == DRUM_CHANNEL;
        let base = track_names.get(ti).cloned().flatten().unwrap_or_else(|| {
            if drums {
                "Drums".into()
            } else {
                format!("Track {}", out.len() + 1)
            }
        });
        let name = if channels_per_track(ti) > 1 && !drums {
            format!("{base} {}", ch + 1)
        } else {
            base
        };
        let mut notes: Vec<NoteInput> = part
            .notes
            .iter()
            .map(|&(s, e, key, vel)| NoteInput {
                pitch: key.min(127),
                start_beats: s as f64 * beats_per_tick,
                length_beats: ((e.saturating_sub(s)) as f64 * beats_per_tick).max(MIN_LENGTH_BEATS),
                velocity: vel.clamp(1, 127),
                id: None,
            })
            .collect();
        notes.sort_by(|a, b| {
            a.start_beats
                .total_cmp(&b.start_beats)
                .then(a.pitch.cmp(&b.pitch))
        });
        let end = notes
            .iter()
            .map(|n| n.start_beats + n.length_beats)
            .fold(0.0, f64::max);
        out.push(ImportedPart {
            name,
            drums,
            preset: (!drums).then(|| preset_for_program(part.program.unwrap_or(0))),
            notes,
            length_beats: whole_bars(end, beats_per_bar),
        });
    }
    if out.is_empty() {
        return Err(NotationError::NoNotes);
    }
    Ok(ImportedSong {
        title,
        tempo_bpm: us_per_quarter
            .map(|us| 60_000_000.0 / f64::from(us.max(1)) / quarters_per_beat(ts)),
        time_signature,
        parts: out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_model::Command;

    fn song() -> Project {
        let mut p = Project {
            name: "Boss".into(),
            tempo_bpm: 140.0,
            ..Project::default()
        };
        let n = |pitch, start, len| NoteInput {
            pitch,
            start_beats: start,
            length_beats: len,
            velocity: 90,
            id: None,
        };
        Command::CreateClip {
            track_id: 1,
            start_beats: 4.0,
            length_beats: 4.0,
            name: None,
            notes: vec![
                n(60, 0.0, 1.0),
                n(64, 0.0, 1.0),
                n(67, 1.5, 0.5),
                n(72, 3.5, 2.0),
            ],
        }
        .apply(&mut p)
        .expect("keys");
        Command::CreateClip {
            track_id: 3,
            start_beats: 0.0,
            length_beats: 4.0,
            name: None,
            notes: vec![n(36, 0.0, 0.25), n(38, 1.0, 0.25), n(36, 2.0, 0.25)],
        }
        .apply(&mut p)
        .expect("drums");
        p
    }

    #[test]
    fn round_trips_notes_tempo_and_meter() {
        let bytes = export_midi(&song());
        let back = import_midi(&bytes).expect("parse");
        assert_eq!(back.title.as_deref(), Some("Boss"));
        assert!((back.tempo_bpm.expect("tempo") - 140.0).abs() < 0.01);
        assert_eq!(back.time_signature, Some(TimeSignature::default()));
        assert_eq!(back.parts.len(), 2);
        let keys = &back.parts[0];
        assert_eq!(keys.name, "Keys");
        assert!(!keys.drums);
        assert_eq!(keys.preset, Some("Warm Keys"));
        let starts: Vec<(f64, u8)> = keys
            .notes
            .iter()
            .map(|n| (n.start_beats, n.pitch))
            .collect();
        assert_eq!(starts, vec![(4.0, 60), (4.0, 64), (5.5, 67), (7.5, 72)]);
        // The last note is cut at the clip end, as it plays.
        assert!((keys.notes[3].length_beats - 0.5).abs() < 1e-9);
        assert_eq!(keys.length_beats, 8.0);
        let drums = &back.parts[1];
        assert!(drums.drums);
        assert_eq!(drums.notes.len(), 3);
        assert_eq!(drums.notes[1].pitch, 38);
    }

    #[test]
    fn compound_meter_counts_eighth_note_beats() {
        let mut p = song();
        Command::SetTimeSignature {
            numerator: 6,
            denominator: 8,
        }
        .apply(&mut p)
        .expect("6/8");
        let back = import_midi(&export_midi(&p)).expect("parse");
        assert_eq!(
            back.time_signature,
            Some(TimeSignature {
                numerator: 6,
                denominator: 8
            })
        );
        assert!((back.tempo_bpm.expect("tempo") - 140.0).abs() < 0.01);
        assert_eq!(back.parts[0].notes[2].start_beats, 5.5);
    }

    #[test]
    fn rejects_junk_and_empty_files() {
        assert!(matches!(
            import_midi(b"nope"),
            Err(NotationError::BadMidi(_))
        ));
        let empty = export_midi(&Project::default());
        assert!(matches!(import_midi(&empty), Err(NotationError::NoNotes)));
    }
}
