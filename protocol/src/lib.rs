#![no_std]

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format, Serialize, Deserialize, Default)]
pub enum PluckTechnique {
    #[default]
    Soft,
    Hard,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format, Serialize, Deserialize)]
pub enum Message {
    FretPresence(Fret),
    PluckPresence(GuitarString),
    ConfirmPresence,
    Pluck(GuitarString),
    PluckVolume(GuitarString, PluckVolume),
    PluckEnable(GuitarString),
    PluckDisable(GuitarString),
    FretFast(GuitarString, Fret),
    FretQuiet(GuitarString, Fret),
    FretAdaptive(GuitarString, Fret),
    Unfret(GuitarString, Fret),
    UnfretFast(GuitarString, Fret),
    Dampen(GuitarString, Fret),
    FretCalibration(GuitarString, Fret),
    Config(Fret, ConfigValue),
    Reset,
    PluckSpeed(GuitarString, u16),
    PluckTechnique(GuitarString, PluckTechnique),
}

impl Message {
    pub fn get_fret(&self) -> Option<Fret> {
        match self {
            Message::FretPresence(fret)
            | Message::FretFast(_, fret)
            | Message::FretQuiet(_, fret)
            | Message::Unfret(_, fret)
            | Message::UnfretFast(_, fret)
            | Message::Dampen(_, fret)
            | Message::FretAdaptive(_, fret)
            | Message::FretCalibration(_, fret)
            | Message::Config(fret, _) => Some(*fret),
            _ => None,
        }
    }

    pub fn get_string(&self) -> Option<GuitarString> {
        match self {
            Message::Pluck(guitar_string)
            | Message::PluckTechnique(guitar_string, _)
            | Message::PluckSpeed(guitar_string, _)
            | Message::PluckPresence(guitar_string)
            | Message::PluckVolume(guitar_string, _)
            | Message::PluckEnable(guitar_string)
            | Message::PluckDisable(guitar_string)
            | Message::FretFast(guitar_string, _)
            | Message::FretQuiet(guitar_string, _)
            | Message::FretAdaptive(guitar_string, _)
            | Message::Unfret(guitar_string, _)
            | Message::UnfretFast(guitar_string, _)
            | Message::Dampen(guitar_string, _)
            | Message::FretCalibration(guitar_string, _) => Some(*guitar_string),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format, Serialize, Deserialize)]
pub struct Percentage {
    value: u8,
}

impl From<u8> for Percentage {
    fn from(value: u8) -> Self {
        Percentage::new(value)
    }
}

impl From<Percentage> for u8 {
    fn from(value: Percentage) -> Self {
        value.get_value()
    }
}

impl Percentage {
    pub const fn new(value: u8) -> Self {
        assert!(value <= 100);
        Self { value }
    }

    pub fn get_value(&self) -> u8 {
        self.value
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format, Serialize, Deserialize)]
pub struct Duration {
    value_ms: u16,
}

impl Duration {
    pub const fn new(value_ms: u16) -> Self {
        Self { value_ms }
    }

    pub fn get_value_ms(&self) -> u16 {
        self.value_ms
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format, Serialize, Deserialize)]
pub enum ConfigValue {
    MaxForce(Percentage),
    HoldForce(Percentage),
    DampenForce(Percentage),
    MarginalForce(Percentage),
    ReleaseDuration(Duration),
    DampenToFretRampDuration(Duration),
    FretFastMaxForceDuration(Duration),
    FretQuietPhase1Duration(Duration),
    FretQuietPhase2Duration(Duration),
    FretAdaptivePhase1Duration(Duration),
    FretAdaptivePhase2Duration(Duration),
    FretAdaptivePhase3Durtaion(Duration),
    FretAdaptivePhase1Force(Percentage),
    FretAdaptivePhase2Force(Percentage),
    FretAdaptivePhase3Force(Percentage),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format, Serialize, Deserialize)]
pub struct PluckVolume {
    volume: u8,
}

impl PluckVolume {
    pub const fn volume(&self) -> u8 {
        self.volume
    }

    pub const fn max_volume() -> u8 {
        u8::MAX
    }
}

impl From<u8> for PluckVolume {
    fn from(value: u8) -> Self {
        Self { volume: value }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format, Serialize, Deserialize)]
#[repr(u8)]
pub enum GuitarString {
    E,
    A,
    D,
    G,
    B,
    #[allow(non_camel_case_types)]
    e,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format, Serialize, Deserialize)]
#[repr(u8)]
pub enum Fret {
    NoFret,
    Fret1,
    Fret2,
    Fret3,
    Fret4,
    Fret5,
    Fret6,
    Fret7,
    Fret8,
    Fret9,
    Fret10,
    Fret11,
    Fret12,
    Fret13,
    Fret14,
    Fret15,
    Fret16,
    Fret17,
    Fret18,
}

pub const MAX_FRAME_SIZE: usize = 8;
const SENTINEL_BYTE: u8 = 0x00;

pub struct Parser {
    message_buffer: [u8; MAX_FRAME_SIZE],
    received_bytes: usize,
}

#[derive(defmt::Format, Debug)]
pub enum ParserError {
    EmptyFrame,
    FrameTooLong,
    DecodeError(postcard::Error, [u8; MAX_FRAME_SIZE], usize),
}

impl Message {
    pub fn encode<'a>(&self, buffer: &'a mut [u8]) -> postcard::Result<&'a mut [u8]> {
        postcard::to_slice_cobs(self, buffer)
    }
}

impl Parser {
    pub fn new() -> Self {
        Parser {
            message_buffer: [0; _],
            received_bytes: 0,
        }
    }

    pub fn consume(&mut self, byte: u8) -> Result<Option<Message>, ParserError> {
        self.message_buffer[self.received_bytes] = byte;
        self.received_bytes += 1;

        if byte == SENTINEL_BYTE {
            if self.received_bytes > 1 {
                let received_bytes = self.received_bytes;
                self.received_bytes = 0;
                let message = postcard::from_bytes_cobs(&mut self.message_buffer)
                    .map_err(|e| ParserError::DecodeError(e, self.message_buffer, received_bytes))?;

                return Ok(Some(message));
            } else {
                let err = ParserError::EmptyFrame;
                self.received_bytes = 0;
                return Err(err);
            }
        } else if self.received_bytes == MAX_FRAME_SIZE {
            self.received_bytes = 0;
            return Err(ParserError::FrameTooLong);
        }

        Ok(None)
    }
}
