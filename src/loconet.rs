use crate::power::PowerRequest;
use thiserror::Error;

const OPC_BUSY: u8 = 0x81;
const OPC_GPOFF: u8 = 0x82;
const OPC_GPON: u8 = 0x83;
const OPC_IDLE: u8 = 0x85;
const OPC_LOCO_SPD: u8 = 0xA0;
const OPC_LOCO_DIRF: u8 = 0xA1;
const OPC_SW_REQ: u8 = 0xB0;
const OPC_INPUT_REP: u8 = 0xB2;
const OPC_LONG_ACK: u8 = 0xB4;
const OPC_MOVE_SLOTS: u8 = 0xBA;
const OPC_RQ_SL_DATA: u8 = 0xBB;
const OPC_LOCO_ADR: u8 = 0xBF;
const OPC_SL_RD_DATA: u8 = 0xE7;
const OPC_WR_SL_DATA: u8 = 0xEF;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LocoNetError {
    #[error("LocoNet message is empty")]
    Empty,
    #[error("first byte {0:#04X} is not a LocoNet opcode")]
    MissingOpcode(u8),
    #[error("LocoNet message should be {expected} bytes but has {actual}")]
    LengthMismatch { expected: usize, actual: usize },
    #[error("data byte at offset {0} has bit 7 set")]
    DataBit(usize),
    #[error("LocoNet checksum is invalid")]
    Checksum,
    #[error("invalid hexadecimal byte {0:?}")]
    InvalidHex(String),
}

/// One complete LocoNet message, including its checksum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocoNetMessage {
    bytes: Vec<u8>,
}

impl LocoNetMessage {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, LocoNetError> {
        let opcode = *bytes.first().ok_or(LocoNetError::Empty)?;
        if opcode & 0x80 == 0 {
            return Err(LocoNetError::MissingOpcode(opcode));
        }

        // Opcode bits 6 and 5 give the length; 0b11 means the next byte is the count.
        let expected = match (opcode >> 5) & 0x03 {
            0 => 2,
            1 => 4,
            2 => 6,
            _ => usize::from(*bytes.get(1).ok_or(LocoNetError::LengthMismatch {
                expected: 2,
                actual: bytes.len(),
            })?),
        };
        if bytes.len() != expected {
            return Err(LocoNetError::LengthMismatch {
                expected,
                actual: bytes.len(),
            });
        }
        if let Some(offset) = bytes.iter().skip(1).position(|byte| byte & 0x80 != 0) {
            return Err(LocoNetError::DataBit(offset + 1));
        }
        if bytes.iter().fold(0, |sum, byte| sum ^ byte) != 0xFF {
            return Err(LocoNetError::Checksum);
        }

        Ok(Self {
            bytes: bytes.to_vec(),
        })
    }

    /// Parses the space-separated hex used by LocoNet over TCP, for example `83 7C`.
    pub fn from_hex(text: &str) -> Result<Self, LocoNetError> {
        let bytes = text
            .split_whitespace()
            .map(|token| {
                u8::from_str_radix(token, 16).map_err(|_| LocoNetError::InvalidHex(token.into()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::from_bytes(&bytes)
    }

    pub fn power_request(request: PowerRequest) -> Self {
        let opcode = match request {
            PowerRequest::On => OPC_GPON,
            PowerRequest::Off => OPC_GPOFF,
            PowerRequest::Halt => OPC_IDLE,
        };
        Self::with_checksum(&[opcode])
    }

    fn with_checksum(body: &[u8]) -> Self {
        let checksum = body.iter().fold(0xFF, |sum, byte| sum ^ byte);
        let mut bytes = body.to_vec();
        bytes.push(checksum);
        Self { bytes }
    }

    pub fn opcode(&self) -> u8 {
        self.bytes[0]
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn to_hex(&self) -> String {
        self.bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn power_request_kind(&self) -> Option<PowerRequest> {
        match self.opcode() {
            OPC_GPON => Some(PowerRequest::On),
            OPC_GPOFF => Some(PowerRequest::Off),
            OPC_IDLE => Some(PowerRequest::Halt),
            _ => None,
        }
    }

    /// Short decoded form for the CLI log.
    pub fn describe(&self) -> String {
        let b = &self.bytes;
        match self.opcode() {
            OPC_BUSY => "BUSY".into(),
            OPC_GPOFF => "GPOFF       power off".into(),
            OPC_GPON => "GPON        power on".into(),
            OPC_IDLE => "IDLE        emergency stop".into(),
            OPC_LOCO_SPD => format!("LOCO_SPD    slot {} speed {}", b[1], b[2]),
            OPC_LOCO_DIRF => format!(
                "LOCO_DIRF   slot {} {}",
                b[1],
                if b[2] & 0x20 != 0 {
                    "forward"
                } else {
                    "reverse"
                }
            ),
            OPC_SW_REQ => format!(
                "SW_REQ      switch {} {} {}",
                eleven_bit_address(b[1], b[2]) + 1,
                if b[2] & 0x20 != 0 { "closed" } else { "thrown" },
                if b[2] & 0x10 != 0 { "on" } else { "off" }
            ),
            OPC_INPUT_REP => format!(
                "INPUT_REP   sensor {} {}",
                (eleven_bit_address(b[1], b[2]) << 1 | u16::from((b[2] >> 5) & 1)) + 1,
                if b[2] & 0x10 != 0 { "high" } else { "low" }
            ),
            OPC_LONG_ACK => format!("LONG_ACK    opcode {:02X} code {:02X}", b[1] | 0x80, b[2]),
            OPC_MOVE_SLOTS => format!("MOVE_SLOTS  {} -> {}", b[1], b[2]),
            OPC_RQ_SL_DATA => format!("RQ_SL_DATA  slot {}", b[1]),
            OPC_LOCO_ADR => format!(
                "LOCO_ADR    address {}",
                u16::from(b[1]) << 7 | u16::from(b[2])
            ),
            OPC_SL_RD_DATA => format!("SL_RD_DATA  slot {}", b[2]),
            OPC_WR_SL_DATA => format!("WR_SL_DATA  slot {}", b[2]),
            other => format!("opcode {other:02X}"),
        }
    }
}

/// Splits a raw LocoNet byte stream (binary LocoNet over TCP or serial) into messages.
#[derive(Debug, Default)]
pub struct StreamDecoder {
    buffer: Vec<u8>,
}

impl StreamDecoder {
    /// Adds received bytes and returns every complete message, valid or not.
    pub fn push(&mut self, data: &[u8]) -> Vec<Result<LocoNetMessage, LocoNetError>> {
        self.buffer.extend_from_slice(data);
        let mut messages = Vec::new();
        loop {
            // Bytes before an opcode belong to a lost message; LocoNet receivers skip them.
            match self.buffer.iter().position(|byte| byte & 0x80 != 0) {
                Some(start) => {
                    self.buffer.drain(..start);
                }
                None => {
                    self.buffer.clear();
                    break;
                }
            }
            let length = match (self.buffer[0] >> 5) & 0x03 {
                0 => 2,
                1 => 4,
                2 => 6,
                _ => match self.buffer.get(1) {
                    Some(&count) => usize::from(count).max(2),
                    None => break,
                },
            };
            // A new opcode before the end means this message was cut off.
            if let Some(next) = self
                .buffer
                .iter()
                .take(length)
                .skip(1)
                .position(|byte| byte & 0x80 != 0)
            {
                let broken: Vec<u8> = self.buffer.drain(..=next).collect();
                messages.push(Err(LocoNetError::LengthMismatch {
                    expected: length,
                    actual: broken.len(),
                }));
                continue;
            }
            if self.buffer.len() < length {
                break;
            }
            let bytes: Vec<u8> = self.buffer.drain(..length).collect();
            messages.push(LocoNetMessage::from_bytes(&bytes));
        }
        messages
    }
}

fn eleven_bit_address(low: u8, high: u8) -> u16 {
    u16::from(low & 0x7F) | u16::from(high & 0x0F) << 7
}

#[cfg(test)]
mod tests {
    use super::{LocoNetError, LocoNetMessage, StreamDecoder};

    #[test]
    fn stream_decoder_splits_joins_and_resynchronises() {
        let mut decoder = StreamDecoder::default();
        // Two messages arriving together, the second split over two reads.
        let first = decoder.push(&[0x83, 0x7C, 0xA0, 0x05]);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].as_ref().unwrap().to_hex(), "83 7C");
        let second = decoder.push(&[0x28, 0x72]);
        assert_eq!(second[0].as_ref().unwrap().to_hex(), "A0 05 28 72");

        // Leading garbage is skipped; a message cut off by a new opcode is reported.
        let resync = decoder.push(&[0x12, 0xA0, 0x05, 0x82, 0x7D]);
        assert!(matches!(
            resync[0],
            Err(LocoNetError::LengthMismatch { .. })
        ));
        assert_eq!(resync[1].as_ref().unwrap().to_hex(), "82 7D");

        // Variable-length message waits for all of its bytes.
        let slot = [
            0xE7, 0x0E, 0x05, 0x23, 0x0C, 0x00, 0x20, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1B,
        ];
        assert!(decoder.push(&slot[..6]).is_empty());
        assert_eq!(
            decoder.push(&slot[6..])[0].as_ref().unwrap().describe(),
            "SL_RD_DATA  slot 5"
        );
    }
    use crate::power::PowerRequest;

    #[test]
    fn power_messages_match_the_personal_edition_checksums() {
        assert_eq!(
            LocoNetMessage::power_request(PowerRequest::On).to_hex(),
            "83 7C"
        );
        assert_eq!(
            LocoNetMessage::power_request(PowerRequest::Off).to_hex(),
            "82 7D"
        );
        assert_eq!(
            LocoNetMessage::power_request(PowerRequest::Halt).to_hex(),
            "85 7A"
        );
        assert_eq!(
            LocoNetMessage::from_hex("83 7C")
                .unwrap()
                .power_request_kind(),
            Some(PowerRequest::On)
        );
    }

    #[test]
    fn validates_length_data_bits_and_checksum() {
        assert_eq!(LocoNetMessage::from_bytes(&[]), Err(LocoNetError::Empty));
        assert_eq!(
            LocoNetMessage::from_bytes(&[0x7C]),
            Err(LocoNetError::MissingOpcode(0x7C))
        );
        assert!(matches!(
            LocoNetMessage::from_bytes(&[0xA0, 0x05, 0x28]),
            Err(LocoNetError::LengthMismatch {
                expected: 4,
                actual: 3
            })
        ));
        assert_eq!(
            LocoNetMessage::from_bytes(&[0x83, 0x7D]),
            Err(LocoNetError::Checksum)
        );
        assert_eq!(
            LocoNetMessage::from_bytes(&[0xA0, 0x85, 0x28, 0x72]),
            Err(LocoNetError::DataBit(1))
        );
        assert!(matches!(
            LocoNetMessage::from_hex("83 ZZ"),
            Err(LocoNetError::InvalidHex(_))
        ));
    }

    #[test]
    fn decodes_messages_for_the_cli_log() {
        let sensor = LocoNetMessage::from_hex("B2 21 50 3C").unwrap();
        assert_eq!(sensor.describe(), "INPUT_REP   sensor 67 high");

        let request = LocoNetMessage::from_hex("BF 00 0C 4C").unwrap();
        assert_eq!(request.describe(), "LOCO_ADR    address 12");

        let slot_read =
            LocoNetMessage::from_hex("E7 0E 05 23 0C 00 20 07 00 00 00 00 00 1B").unwrap();
        assert_eq!(slot_read.describe(), "SL_RD_DATA  slot 5");
    }
}
