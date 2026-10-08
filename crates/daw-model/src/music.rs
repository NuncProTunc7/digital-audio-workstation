//! Keys, scales, and chords: the song's harmony, as data the UI, the
//! notation exporters, and Claude share.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Note names by pitch class (0 = C), spelled with sharps.
pub const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// A scale or mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Bright, happy (Ionian).
    Major,
    /// Sad, serious (natural minor, Aeolian).
    Minor,
    /// Minor with a raised 6th: mysterious, folky; great for adventure.
    Dorian,
    /// Minor with a lowered 2nd: dark, exotic, tense.
    Phrygian,
    /// Major with a raised 4th: dreamy, magical.
    Lydian,
    /// Major with a lowered 7th: heroic, open, rock.
    Mixolydian,
    /// Minor with a raised 7th: dramatic, classical, villain themes.
    HarmonicMinor,
    /// Five notes of the major scale: simple, folk, can't sound wrong.
    MajorPentatonic,
    /// Five notes of the minor scale: blues, rock, Asian-flavored.
    MinorPentatonic,
}

impl Mode {
    pub const ALL: [Mode; 9] = [
        Mode::Major,
        Mode::Minor,
        Mode::Dorian,
        Mode::Phrygian,
        Mode::Lydian,
        Mode::Mixolydian,
        Mode::HarmonicMinor,
        Mode::MajorPentatonic,
        Mode::MinorPentatonic,
    ];

    /// Semitones above the tonic of each scale note.
    pub fn intervals(self) -> &'static [u8] {
        match self {
            Mode::Major => &[0, 2, 4, 5, 7, 9, 11],
            Mode::Minor => &[0, 2, 3, 5, 7, 8, 10],
            Mode::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Mode::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            Mode::Lydian => &[0, 2, 4, 6, 7, 9, 11],
            Mode::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            Mode::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            Mode::MajorPentatonic => &[0, 2, 4, 7, 9],
            Mode::MinorPentatonic => &[0, 3, 5, 7, 10],
        }
    }

    /// How far above its relative major's tonic this mode starts, which
    /// gives the key signature (A minor shares C major's).
    fn relative_major_offset(self) -> u8 {
        match self {
            Mode::Major | Mode::MajorPentatonic => 0,
            Mode::Dorian => 2,
            Mode::Phrygian => 4,
            Mode::Lydian => 5,
            Mode::Mixolydian => 7,
            Mode::Minor | Mode::HarmonicMinor | Mode::MinorPentatonic => 9,
        }
    }

    /// Minor-sounding modes (written as minor keys in MIDI and MusicXML).
    pub fn is_minor(self) -> bool {
        matches!(
            self,
            Mode::Minor
                | Mode::Dorian
                | Mode::Phrygian
                | Mode::HarmonicMinor
                | Mode::MinorPentatonic
        )
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Major => "major",
            Mode::Minor => "minor",
            Mode::Dorian => "dorian",
            Mode::Phrygian => "phrygian",
            Mode::Lydian => "lydian",
            Mode::Mixolydian => "mixolydian",
            Mode::HarmonicMinor => "harmonic minor",
            Mode::MajorPentatonic => "major pentatonic",
            Mode::MinorPentatonic => "minor pentatonic",
        }
    }
}

/// The song's key: a tonic and a mode, e.g. A minor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Key {
    /// Pitch class of the tonic: 0 = C, 1 = C#, ... 9 = A, 11 = B.
    pub tonic: u8,
    pub mode: Mode,
}

impl Key {
    /// "A minor", "D dorian".
    pub fn name(&self) -> String {
        format!(
            "{} {}",
            NOTE_NAMES[usize::from(self.tonic % 12)],
            self.mode.name()
        )
    }

    /// Pitch classes in the scale, starting at the tonic.
    pub fn scale(&self) -> Vec<u8> {
        self.mode
            .intervals()
            .iter()
            .map(|i| (self.tonic + i) % 12)
            .collect()
    }

    /// Whether MIDI pitch `pitch` is in the scale.
    pub fn contains(&self, pitch: u8) -> bool {
        self.scale().contains(&(pitch % 12))
    }

    /// Sharps (positive) or flats (negative) in the key signature.
    pub fn fifths(&self) -> i8 {
        let major = (self.tonic % 12 + 12 - self.mode.relative_major_offset()) % 12;
        // C G D A E B F# | Db Ab Eb Bb F
        match major {
            0 => 0,
            7 => 1,
            2 => 2,
            9 => 3,
            4 => 4,
            11 => 5,
            6 => 6,
            1 => -5,
            8 => -4,
            3 => -3,
            10 => -2,
            _ => -1, // F
        }
    }
}

/// What kind of chord: its intervals above the root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChordQuality {
    Major,
    Minor,
    Diminished,
    Augmented,
    Sus2,
    Sus4,
    Major7,
    Minor7,
    Dominant7,
    HalfDiminished7,
    Power,
}

impl ChordQuality {
    pub const ALL: [ChordQuality; 11] = [
        ChordQuality::Major,
        ChordQuality::Minor,
        ChordQuality::Diminished,
        ChordQuality::Augmented,
        ChordQuality::Sus2,
        ChordQuality::Sus4,
        ChordQuality::Major7,
        ChordQuality::Minor7,
        ChordQuality::Dominant7,
        ChordQuality::HalfDiminished7,
        ChordQuality::Power,
    ];

    /// Semitones above the root.
    pub fn intervals(self) -> &'static [u8] {
        match self {
            ChordQuality::Major => &[0, 4, 7],
            ChordQuality::Minor => &[0, 3, 7],
            ChordQuality::Diminished => &[0, 3, 6],
            ChordQuality::Augmented => &[0, 4, 8],
            ChordQuality::Sus2 => &[0, 2, 7],
            ChordQuality::Sus4 => &[0, 5, 7],
            ChordQuality::Major7 => &[0, 4, 7, 11],
            ChordQuality::Minor7 => &[0, 3, 7, 10],
            ChordQuality::Dominant7 => &[0, 4, 7, 10],
            ChordQuality::HalfDiminished7 => &[0, 3, 6, 10],
            ChordQuality::Power => &[0, 7],
        }
    }

    /// The symbol after the root: "" (C), "m" (Cm), "maj7" (Cmaj7)...
    pub fn symbol(self) -> &'static str {
        match self {
            ChordQuality::Major => "",
            ChordQuality::Minor => "m",
            ChordQuality::Diminished => "dim",
            ChordQuality::Augmented => "aug",
            ChordQuality::Sus2 => "sus2",
            ChordQuality::Sus4 => "sus4",
            ChordQuality::Major7 => "maj7",
            ChordQuality::Minor7 => "m7",
            ChordQuality::Dominant7 => "7",
            ChordQuality::HalfDiminished7 => "m7b5",
            ChordQuality::Power => "5",
        }
    }
}

/// One chord on the chord track. It lasts until the next chord.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Chord {
    pub id: crate::project::Id,
    /// Where it starts, in beats from the song start.
    pub start_beats: f64,
    /// Pitch class of the root: 0 = C ... 11 = B.
    pub root: u8,
    pub quality: ChordQuality,
    /// A different bass note (slash chord, e.g. C/E), as a pitch class.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bass: Option<u8>,
}

impl Chord {
    /// "Am7", "C/E".
    pub fn name(&self) -> String {
        let mut s = format!(
            "{}{}",
            NOTE_NAMES[usize::from(self.root % 12)],
            self.quality.symbol()
        );
        if let Some(b) = self.bass {
            s.push('/');
            s.push_str(NOTE_NAMES[usize::from(b % 12)]);
        }
        s
    }

    /// Pitch classes of the chord tones (the bass note first if it has one).
    pub fn pitch_classes(&self) -> Vec<u8> {
        let mut pcs: Vec<u8> = self
            .quality
            .intervals()
            .iter()
            .map(|i| (self.root + i) % 12)
            .collect();
        if let Some(b) = self.bass
            && !pcs.contains(&(b % 12))
        {
            pcs.insert(0, b % 12);
        }
        pcs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_have_names_and_tones() {
        let c = Chord {
            id: 1,
            start_beats: 0.0,
            root: 9,
            quality: ChordQuality::Minor7,
            bass: None,
        };
        assert_eq!(c.name(), "Am7");
        assert_eq!(c.pitch_classes(), vec![9, 0, 4, 7]);
        let slash = Chord {
            root: 0,
            quality: ChordQuality::Major,
            bass: Some(4),
            ..c
        };
        assert_eq!(slash.name(), "C/E");
        assert_eq!(slash.pitch_classes(), vec![0, 4, 7]);
    }

    #[test]
    fn keys_know_their_scales_and_signatures() {
        let a_minor = Key {
            tonic: 9,
            mode: Mode::Minor,
        };
        assert_eq!(a_minor.name(), "A minor");
        assert_eq!(a_minor.fifths(), 0);
        assert!(a_minor.contains(60) && !a_minor.contains(61));
        let d_dorian = Key {
            tonic: 2,
            mode: Mode::Dorian,
        };
        assert_eq!(d_dorian.fifths(), 0, "D dorian has no sharps or flats");
        assert_eq!(
            Key {
                tonic: 7,
                mode: Mode::Major
            }
            .fifths(),
            1
        );
        assert_eq!(
            Key {
                tonic: 2,
                mode: Mode::Minor
            }
            .fifths(),
            -1,
            "D minor has one flat"
        );
        assert_eq!(
            Key {
                tonic: 4,
                mode: Mode::MinorPentatonic
            }
            .scale(),
            vec![4, 7, 9, 11, 2]
        );
    }
}
