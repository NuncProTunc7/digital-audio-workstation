//! Importing and exporting MIDI files and sheet music.

use std::path::Path;

use daw_model::{Command, InstrumentKind, NoteInput, Project, TrackId};
use daw_notation::ImportedSong;

use crate::host::{Host, sync};

/// Turns an imported song into Commands: one new track (with one clip) per
/// part. An empty project also takes the song's tempo, meter, and title.
/// Everything is one undo step. Returns the new track ids.
pub fn import_song<H: Host>(host: &H, song: ImportedSong) -> Result<Vec<TrackId>, String> {
    let mut session = host.session()?;
    // Ids are handed out deterministically, so build the batch against a
    // scratch copy to learn the new tracks' ids, then apply it for real.
    let mut scratch = session.project().clone();
    let empty = scratch.tracks.iter().all(|t| t.clips.is_empty());
    let mut commands = Vec::new();
    let mut apply = |scratch: &mut Project, c: Command| -> Result<(), String> {
        c.clone().apply(scratch).map_err(|e| e.to_string())?;
        commands.push(c);
        Ok(())
    };
    if empty {
        if let Some(bpm) = song.tempo_bpm {
            apply(
                &mut scratch,
                Command::SetTempo {
                    bpm: bpm.clamp(20.0, 999.0),
                },
            )?;
        }
        if let Some(ts) = song.time_signature {
            // Meters this app doesn't support are kept as they were.
            let c = Command::SetTimeSignature {
                numerator: ts.numerator,
                denominator: ts.denominator,
            };
            if c.clone().apply(&mut scratch.clone()).is_ok() {
                apply(&mut scratch, c)?;
            }
        }
        if let Some(title) = song.title.filter(|t| !t.trim().is_empty())
            && scratch.name == "Untitled"
        {
            apply(&mut scratch, Command::RenameProject { name: title })?;
        }
    }
    let mut new_tracks = Vec::new();
    for part in song.parts {
        let kind = if part.drums {
            InstrumentKind::Drums
        } else {
            InstrumentKind::Synth
        };
        apply(
            &mut scratch,
            Command::AddTrack {
                name: part.name.clone(),
                instrument: kind,
                preset: part.preset.map(str::to_owned),
                index: None,
            },
        )?;
        let track_id = scratch
            .tracks
            .last()
            .map(|t| t.id)
            .ok_or("the new track vanished")?;
        new_tracks.push(track_id);
        let notes: Vec<NoteInput> = part.notes;
        apply(
            &mut scratch,
            Command::CreateClip {
                track_id,
                start_beats: 0.0,
                length_beats: part.length_beats.min(daw_model::MAX_BEATS),
                name: Some(part.name),
                notes,
            },
        )?;
    }
    session
        .execute(Command::Batch { commands })
        .map_err(|e| e.to_string())?;
    session.end_gesture();
    sync(host, &session);
    Ok(new_tracks)
}

/// Reads a `.mid` file into the song as new tracks.
pub fn import_midi_file<H: Host>(host: &H, path: &Path) -> Result<Vec<TrackId>, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let song = daw_notation::midi::import_midi(&bytes).map_err(|e| e.to_string())?;
    import_song(host, song)
}

/// Reads a MusicXML score (`.musicxml`, `.xml`, or compressed `.mxl`)
/// into the song as new tracks.
pub fn import_musicxml_file<H: Host>(host: &H, path: &Path) -> Result<Vec<TrackId>, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let song = daw_notation::musicxml::import_musicxml_bytes(&bytes).map_err(|e| e.to_string())?;
    import_song(host, song)
}

/// Reads MusicXML text (for example written by Claude from a photo of
/// sheet music) into the song as new tracks.
pub fn import_musicxml_text<H: Host>(host: &H, xml: &str) -> Result<Vec<TrackId>, String> {
    let song = daw_notation::musicxml::import_musicxml(xml).map_err(|e| e.to_string())?;
    import_song(host, song)
}

/// The song (or some tracks) as MusicXML text.
pub fn sheet_music<H: Host>(host: &H, track_ids: Option<&[TrackId]>) -> Result<String, String> {
    Ok(daw_notation::musicxml::export_musicxml(
        host.session()?.project(),
        track_ids,
    ))
}

/// Writes the song (or some tracks) as a MusicXML file.
pub fn export_musicxml_file<H: Host>(
    host: &H,
    path: &Path,
    track_ids: Option<&[TrackId]>,
) -> Result<(), String> {
    let xml = sheet_music(host, track_ids)?;
    std::fs::write(path, xml).map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// Writes the song's instrument tracks as a `.mid` file.
pub fn export_midi_file<H: Host>(host: &H, path: &Path) -> Result<(), String> {
    let bytes = daw_notation::midi::export_midi(host.session()?.project());
    std::fs::write(path, bytes).map_err(|e| format!("could not write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::tests::TestHost;
    use crate::{Request, handle};

    #[test]
    fn midi_round_trip_adds_tracks_in_one_undo_step() {
        let host = TestHost::default();
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("song.mid");
        {
            let mut s = host.session().expect("s");
            s.execute(Command::SetTempo { bpm: 96.0 }).expect("tempo");
            s.execute(Command::CreateClip {
                track_id: 2,
                start_beats: 0.0,
                length_beats: 4.0,
                name: None,
                notes: vec![NoteInput {
                    pitch: 40,
                    start_beats: 1.0,
                    length_beats: 1.0,
                    velocity: 100,
                    id: None,
                }],
            })
            .expect("clip");
        }
        handle(
            &host,
            Request::ExportMidi {
                path: path.display().to_string(),
            },
        )
        .into_result()
        .expect("export");

        // A fresh, empty song takes the file's tempo and gets its parts.
        let other = TestHost::default();
        let before = other.session().expect("s").project().clone();
        let v = handle(
            &other,
            Request::ImportMidi {
                path: path.display().to_string(),
            },
        )
        .into_result()
        .expect("import");
        let ids = v["new_track_ids"].as_array().expect("ids").len();
        assert_eq!(ids, 1);
        {
            let s = other.session().expect("s");
            let p = s.project();
            assert!((p.tempo_bpm - 96.0).abs() < 0.01);
            let t = p.tracks.last().expect("track");
            assert_eq!(t.name, "Bass");
            assert_eq!(t.instrument.preset, "Fat Bass");
            assert_eq!(t.clips[0].notes[0].pitch, 40);
            assert_eq!(t.clips[0].notes[0].start_beats, 1.0);
        }
        handle(&other, Request::Undo).into_result().expect("undo");
        // Ids are never reused, so only the id counter stays advanced.
        let mut after = other.session().expect("s").project().clone();
        after.next_id = before.next_id;
        assert_eq!(after, before);
    }
}
