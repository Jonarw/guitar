pub const NUMBER_OF_STRINGS: usize = 6;

#[derive(Debug)]
pub struct LilyScore {
    pub header: Option<Header>,
    pub global: Global,
    pub parts: [LilyPart; NUMBER_OF_STRINGS],
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Global {
    pub key: Option<Key>,
}

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

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Header {
    pub title: Option<String>,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub tonic: PitchClass,
    pub major: bool,
}

#[derive(Debug)]
pub struct LilyPart {
    pub name: String,
    pub events: Vec<Event>,
}

#[derive(Debug)]
pub enum Event {
    Note(Note),
    Rest(Rest),
    TimeSignature(TimeSignature),
    Tempo(Tempo),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tuplet {
    pub num: u8,
    pub den: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteDuration {
    pub ratio: u16,
    pub augmentation: u8,
    pub tuplet: Option<Tuplet>,
}

impl Default for NoteDuration {
    fn default() -> Self {
        Self {
            ratio: 4,
            augmentation: 0,
            tuplet: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Note {
    pub class: PitchClass,
    pub accidental: Accidental,
    pub octave: i8,
    pub duration: Option<NoteDuration>,
    pub dynamic: Option<Dynamic>,
    pub articulation: Option<Articulation>,
    pub crescendo: Option<Crescendo>,
}

#[derive(Debug, Clone, Copy)]
pub struct Rest {
    pub duration: Option<NoteDuration>,
    pub dynamic: Option<Dynamic>,
    pub articulation: Option<Articulation>,
    pub crescendo: Option<Crescendo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accidental {
    DoubleFlat,
    Flat,
    None,
    Sharp,
    DoubleSharp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Articulation {
    Tenuto,
    Portato,
    Staccato,
    Staccatissimo,
    Marcato,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Crescendo {
    CrescendoStart,
    DecrescendoStart,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dynamic {
    PPP,
    PP,
    P,
    MP,
    MF,
    F,
    FF,
    FFF,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PitchClass {
    C,
    D,
    E,
    F,
    G,
    A,
    B,
}
