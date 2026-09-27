#![no_std]

use enum_iterator::Sequence;
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
    Unfret(GuitarString, Fret),
    UnfretFast(GuitarString, Fret),
    Dampen(GuitarString, Fret),
    FretCalibration(GuitarString, Fret),
    Reset,
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
            | Message::FretCalibration(_, fret) => Some(*fret),
            _ => None,
        }
    }

    pub fn get_string(&self) -> Option<GuitarString> {
        match self {
            Message::Pluck(guitar_string)
            | Message::PluckTechnique(guitar_string, _)
            | Message::PluckPresence(guitar_string)
            | Message::PluckVolume(guitar_string, _)
            | Message::PluckEnable(guitar_string)
            | Message::PluckDisable(guitar_string)
            | Message::FretFast(guitar_string, _)
            | Message::FretQuiet(guitar_string, _)
            | Message::Unfret(guitar_string, _)
            | Message::UnfretFast(guitar_string, _)
            | Message::Dampen(guitar_string, _)
            | Message::FretCalibration(guitar_string, _) => Some(*guitar_string),
            _ => None,
        }
    }

    pub fn get_string_and_fret(&self) -> Option<(GuitarString, Fret)> {
        match self {
            Message::FretFast(guitar_string, fret)
            | Message::FretQuiet(guitar_string, fret)
            | Message::Unfret(guitar_string, fret)
            | Message::UnfretFast(guitar_string, fret)
            | Message::Dampen(guitar_string, fret)
            | Message::FretCalibration(guitar_string, fret) => Some((*guitar_string, *fret)),
            _ => None,
        }
    }
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

#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format, Serialize, Deserialize, Sequence, Hash)]
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

#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format, Serialize, Deserialize, Hash)]
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
