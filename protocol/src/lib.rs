#![no_std]

use num_enum::{IntoPrimitive, TryFromPrimitive};

#[derive(defmt::Format)]
pub struct MessageFrame {
    pub action: MessageAction,
    pub string: GuitarString,
    pub fret: Fret,
    pub pluck_volume: PluckVolume,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format)]
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

#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format, TryFromPrimitive, IntoPrimitive)]
#[repr(u8)]
pub enum MessageAction {
    Presence,
    ConfirmPresence,
    Pluck,
    PluckVolume,
    PluckEnable,
    PluckDisable,
    FretFast,
    FretQuiet,
    Unfret,
    Dampen,
    FretCalibration,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format, TryFromPrimitive, IntoPrimitive)]
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

#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format, TryFromPrimitive, IntoPrimitive)]
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

#[derive(defmt::Format, Debug)]
pub enum DecodeError {
    CobsError(cobs::DecodeError),
    WrongFrameSize(usize),
    InvalidAction,
    InvalidString,
    InvalidFret,
    ChecksumError(u8, u8),
}

impl MessageFrame {
    pub const ENCODED_BYTE_SIZE: usize = 4;
    pub const COBS_BYTE_SIZE: usize = Self::ENCODED_BYTE_SIZE + 2; // 1 stuffing byte, 1 sentinel byte
    pub const SENTINEL_BYTE: u8 = 0xFF;

    fn get_checksum(&self) -> u8 {
        self.pluck_volume
            .volume
            .wrapping_add(self.action.into())
            .wrapping_add(self.string.into())
            .wrapping_add(self.fret.into())
    }

    pub fn new(action: MessageAction, string: GuitarString, fret: Fret, pluck_volume: PluckVolume) -> MessageFrame {
        Self {
            action,
            string,
            fret,
            pluck_volume,
        }
    }

    pub fn verify_checksum(&self, checksum: u8) -> bool {
        checksum == self.get_checksum()
    }

    pub fn encode(&self) -> [u8; Self::ENCODED_BYTE_SIZE] {
        let mut raw_bytes = [0; Self::ENCODED_BYTE_SIZE];

        raw_bytes[0] = self.action.into();
        raw_bytes[1] = self.string.into();
        raw_bytes[2] = match self.action {
            MessageAction::PluckVolume => self.pluck_volume.volume,
            _ => self.fret.into(),
        };

        raw_bytes[3] = self.get_checksum();
        raw_bytes
    }

    pub fn cobs_encode(&self) -> [u8; Self::COBS_BYTE_SIZE] {
        let raw = self.encode();
        let mut ret = [0xFF; _];
        cobs::encode_with_sentinel(&raw, &mut ret, Self::SENTINEL_BYTE);
        ret
    }

    pub fn cobs_decode(mut bytes: [u8; Self::COBS_BYTE_SIZE]) -> Result<Self, DecodeError> {
        let decoded_bytes =
            cobs::decode_in_place_with_sentinel(&mut bytes, 0xFF).map_err(|e| DecodeError::CobsError(e))?;

        if decoded_bytes != Self::ENCODED_BYTE_SIZE {
            return Err(DecodeError::WrongFrameSize(decoded_bytes));
        }

        Self::decode(bytes[0..Self::ENCODED_BYTE_SIZE].try_into().unwrap())
    }

    pub fn decode(bytes: [u8; Self::ENCODED_BYTE_SIZE]) -> Result<Self, DecodeError> {
        let action = bytes[0].try_into().map_err(|_| DecodeError::InvalidAction)?;
        let string = bytes[1].try_into().map_err(|_| DecodeError::InvalidString)?;

        let (fret, pluck_volume) = match action {
            MessageAction::PluckVolume => (Fret::NoFret, bytes[2].into()),
            _ => (bytes[2].try_into().map_err(|_| DecodeError::InvalidFret)?, 0.into()),
        };

        let ret = Self {
            action,
            string,
            fret,
            pluck_volume,
        };

        let checksum = ret.get_checksum();
        if checksum != bytes[3] {
            return Err(DecodeError::ChecksumError(checksum, bytes[3]));
        }

        Ok(ret)
    }
}

pub struct Parser {
    message_buffer: [u8; MessageFrame::COBS_BYTE_SIZE],
    received_bytes: usize,
}

#[derive(defmt::Format, Debug)]
pub enum ParserError {
    FrameTooShort(usize),
    FrameTooLong,
    DeocdeError(DecodeError),
}

impl Parser {
    pub fn new() -> Self {
        Parser {
            message_buffer: [0; _],
            received_bytes: 0,
        }
    }

    pub fn consume(&mut self, byte: u8) -> Result<Option<MessageFrame>, ParserError> {
        self.message_buffer[self.received_bytes] = byte;
        self.received_bytes += 1;

        if byte == MessageFrame::SENTINEL_BYTE {
            if self.received_bytes == MessageFrame::COBS_BYTE_SIZE {
                self.received_bytes = 0;
                let message =
                    MessageFrame::cobs_decode(self.message_buffer).map_err(|e| ParserError::DeocdeError(e))?;

                return Ok(Some(message));
            } else {
                let err = ParserError::FrameTooShort(self.received_bytes);
                self.received_bytes = 0;
                return Err(err);
            }
        } else if self.received_bytes == MessageFrame::COBS_BYTE_SIZE {
            self.received_bytes = 0;
            return Err(ParserError::FrameTooLong);
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get_test_message() -> MessageFrame {
        MessageFrame::new(MessageAction::Pluck, GuitarString::D, Fret::Fret6, 0.into())
    }

    #[test]
    fn encode_decode() {
        let message = get_test_message();
        let bytes = message.encode();
        let message2 = MessageFrame::decode(bytes).unwrap();

        assert_eq!(message.action, message2.action);
        assert_eq!(message.string, message2.string);
        assert_eq!(message.fret, message2.fret);
        assert_eq!(message.pluck_volume, message2.pluck_volume);
    }

    #[test]
    fn cobs_encode_decode() {
        let message = get_test_message();
        let bytes = message.cobs_encode();
        let message2 = MessageFrame::cobs_decode(bytes).unwrap();

        assert_eq!(message.action, message2.action);
        assert_eq!(message.string, message2.string);
        assert_eq!(message.fret, message2.fret);
        assert_eq!(message.pluck_volume, message2.pluck_volume);
    }

    #[test]
    fn parser() {
        let message = get_test_message();
        let bytes = message.cobs_encode();

        let mut parser = Parser::new();
        assert!(matches!(parser.consume(bytes[0]), Ok(None)));
        assert!(matches!(parser.consume(bytes[1]), Ok(None)));
        assert!(matches!(parser.consume(bytes[2]), Ok(None)));
        assert!(matches!(parser.consume(bytes[3]), Ok(None)));
        assert!(matches!(parser.consume(bytes[4]), Ok(None)));

        let message2 = parser.consume(bytes[5]).unwrap().unwrap();

        assert_eq!(message.action, message2.action);
        assert_eq!(message.string, message2.string);
        assert_eq!(message.fret, message2.fret);
        assert_eq!(message.pluck_volume, message2.pluck_volume);
    }
}
