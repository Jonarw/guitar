use std::default;

use bitmask_enum::bitmask;
pub const NUMBER_OF_STRINGS: usize = 6;

/// Parsed LilyPond score model used as the conversion input for the machine score.
#[derive(Debug)]
pub struct LilyScore {
    pub header: Option<Header>,
    pub global: Global,
    pub parts: [LilyPart; NUMBER_OF_STRINGS],
}

/// Score-wide musical settings.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Global {
    pub key: Option<Key>,
}

/// Tempo marking where `note_duration = bpm` (for example quarter note = 120).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Tempo {
    pub note_duration: NoteDuration,
    pub bpm: u16,
}

impl Default for Tempo {
    fn default() -> Self {
        Self {
            note_duration: NoteDuration::default(),
            bpm: 90,
        }
    }
}

/// Human-readable score metadata.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Header {
    pub title: Option<String>,
}

/// Conventional meter, e.g. `4/4` or `3/8`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSignature {
    pub numerator: u8,
    pub denominator: u8,
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self {
            numerator: 4,
            denominator: 4,
        }
    }
}

/// Tonality used by the piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub tonic: PitchClass,
    pub accidental: Accidental,
    pub major: bool,
}

/// One performable part/voice.
#[derive(Debug)]
pub struct LilyPart {
    pub name: String,
    pub events: Vec<Event>,
}

/// Timeline event in a part.
#[derive(Debug, Clone)]
pub enum Event {
    Note(Note),
    Rest(Rest),
    TimeSignature(TimeSignature),
    Tempo(Tempo),
    Tuplet(Tuplet),
}

/// Tuplet ratio (`num` in the time of `den`).
#[derive(Debug, Clone)]
pub struct Tuplet {
    pub num: u8,
    pub den: u8,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestMultiplier {
    pub num: u8,
    pub den: u8,
}

/// Musical duration relative to a whole note.
///
/// `ratio` is the denominator (`4` = quarter), `augmentation` is the number of dots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteDuration {
    pub ratio: u16,
    pub augmentation: u8,
}

impl Default for NoteDuration {
    fn default() -> Self {
        Self {
            ratio: 4,
            augmentation: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub enum NoteOrRest {
    Note(Note),
    Rest(Rest),
}

/// Pitched event.
#[derive(Debug, Clone, Default)]
pub struct Note {
    pub class: PitchClass,
    pub accidental: Accidental,
    pub octave: i8,
    pub duration: Option<NoteDuration>,
    pub dynamic: Option<Dynamic>,
    pub articulation: Articulation,
    pub crescendo: Option<Crescendo>,
    pub tie: bool,
    pub fingering: Option<u16>,
    pub slur: Option<Slur>,
    pub tremolo: Option<Tremolo>,
}

/// Rest event.
#[derive(Debug, Clone, Default)]
pub struct Rest {
    pub duration: Option<NoteDuration>,
    pub dynamic: Option<Dynamic>,
    pub articulation: Articulation,
    pub crescendo: Option<Crescendo>,
    pub multipliers: Vec<u32>,
    pub dividers: Vec<u32>,
    pub slur: Option<Slur>,
}

/// Chromatic alteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Accidental {
    DoubleFlat,
    Flat,
    #[default]
    None,
    Sharp,
    DoubleSharp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slur {
    Start,
    End,
}

/// Articulation marking.
#[bitmask]
#[derive(Default)]
pub enum Articulation {
    Tenuto,
    Portato,
    Staccato,
    Staccatissimo,
    Marcato,
    Accent,
}

/// Crescendo hairpin marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Crescendo {
    CrescendoStart,
    DecrescendoStart,
    End,
}

/// Dynamic marking from very soft to very loud.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dynamic {
    PPP,
    PP,
    P,
    MP,
    #[default]
    MF,
    F,
    FF,
    FFF,
}

/// Diatonic pitch class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PitchClass {
    #[default]
    C,
    D,
    E,
    F,
    G,
    A,
    B,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tremolo {
    pub repetition_duration: Option<u16>,
}
