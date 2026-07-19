pub const NUMBER_OF_STRINGS: usize = 6;

#[derive(Debug)]
pub struct Score {
    pub global: Global,
    pub strings: [StringPart; NUMBER_OF_STRINGS],
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Global {
    pub tempo: Option<u32>,
    pub time: Option<TimeSignature>,
    pub key: Option<Key>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct TimeSignature {
    pub numerator: u8,
    pub denominator: u8,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Key {
    pub tonic: PitchClass,
    pub major: bool,
}

#[derive(Debug)]
pub struct StringPart {
    pub name: String,
    pub events: Vec<Event>,
}

#[derive(Debug)]
pub enum Event {
    Note(Note),
    Rest(Rest),
}

#[derive(Debug, Clone)]
pub struct Note {
    pub class: PitchClass,
    pub accidental: Accidental,
    pub octave: i8,
    pub duration: Option<u32>,
    pub dynamic: Option<Dynamic>,
}

#[derive(Debug, Clone)]
pub struct Rest {
    pub duration: Option<u32>,
    pub dynamic: Option<Dynamic>,
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub enum Accidental {
    Minus2,
    Minus1,
    #[default]
    Zero,
    Plus1,
    Plus2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dynamic {
    PPP,
    PP,
    P,
    MP,
    MF,
    F,
    FF,
    FFF,
    CrescendoStart,
    DecrescendoStart,
    CrescendoEnd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PitchClass {
    C,
    D,
    E,
    F,
    G,
    A,
    B,
}
