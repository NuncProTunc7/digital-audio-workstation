//! MusicXML (the format notation apps such as MuseScore read and write).

mod export;
mod import;

pub use export::export_musicxml;
pub use import::{import_musicxml, import_musicxml_bytes, read_mxl};

/// Parses XML, allowing the `<!DOCTYPE>` line every MusicXML file has
/// (external DTDs are never fetched).
pub(crate) fn parse_xml(xml: &str) -> Result<roxmltree::Document<'_>, roxmltree::Error> {
    roxmltree::Document::parse_with_options(
        xml,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..roxmltree::ParsingOptions::default()
        },
    )
}

/// MusicXML durations count in divisions of a quarter note. Four makes the
/// shortest written note a sixteenth, which is also the export grid.
pub(crate) const DIVISIONS: i64 = 4;

/// A General MIDI drum note as written on a five-line percussion staff:
/// display step, octave, and whether it gets an "x" notehead (cymbals).
pub(crate) fn drum_position(note: u8) -> (char, i32, bool) {
    match note {
        35 | 36 => ('F', 4, false),          // kick
        37 => ('C', 5, true),                // rim
        38 | 40 => ('C', 5, false),          // snare
        39 => ('C', 5, true),                // clap
        41 | 43 => ('A', 4, false),          // floor toms
        45 | 47 => ('D', 5, false),          // low/mid toms
        48 | 50 => ('E', 5, false),          // high toms
        42 | 44 | 46 => ('G', 5, true),      // hi-hats
        49 | 52 | 55 | 57 => ('A', 5, true), // crashes
        51 | 53 | 59 => ('F', 5, true),      // rides
        _ => ('C', 5, true),
    }
}
