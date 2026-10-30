use crate::power::PowerRequest;
use thiserror::Error;

const OPC_BUSY: u8 = 0x81;
const OPC_GPOFF: u8 = 0x82;
const OPC_GPON: u8 = 0x83;
const OPC_IDLE: u8 = 0x85;
const OPC_LOCO_SPD: u8 = 0xA0;
const OPC_LOCO_DIRF: u8 = 0xA1;
const OPC_LOCO_SND: u8 = 0xA2;
const OPC_LOCO_F9F12: u8 = 0xA3;
const OPC_SW_REQ: u8 = 0xB0;
const OPC_INPUT_REP: u8 = 0xB2;
const OPC_LONG_ACK: u8 = 0xB4;
const OPC_MOVE_SLOTS: u8 = 0xBA;
const OPC_RQ_SL_DATA: u8 = 0xBB;
const OPC_LOCO_ADR: u8 = 0xBF;
const OPC_SL_RD_DATA: u8 = 0xE7;
const OPC_WR_SL_DATA: u8 = 0xEF;
const FAST_CLOCK_SLOT: u8 = 0x7B;
/// Throttle IDs `7F 7x` are reserved for PCs; Rocrail uses `7F 70` and JMRI `7F 71`.
pub const HIVE_RAIL_CLOCK_ID: [u8; 2] = [0x7F, 0x7E];

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

/// Fast clock as carried in slot 123. Minutes are stored as `(255 - (60 - m)) & 0x7F`
/// and hours as `(256 - (24 - h)) & 0x7F`, as JMRI does and Rocrail's recording confirms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FastClockSlot {
    /// 0 freezes the clock.
    pub rate: u8,
    pub hour: u8,
    pub minute: u8,
    /// Clock control bit 6: the slot holds valid clock information.
    pub valid: bool,
    pub setter: [u8; 2],
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

    /// Writes slot 123 so every throttle and central follows Hive-Rail's clock.
    pub fn fast_clock_write(hour: u8, minute: u8, rate: u8, track_status: u8) -> Self {
        Self::fast_clock_slot(OPC_WR_SL_DATA, hour, minute, rate, track_status)
    }

    /// Answers a client's `RQ_SL_DATA` for slot 123; Rocrail reads the clock this way.
    pub fn fast_clock_read(hour: u8, minute: u8, rate: u8, track_status: u8) -> Self {
        Self::fast_clock_slot(OPC_SL_RD_DATA, hour, minute, rate, track_status)
    }

    /// Whether this is a client's request for the fast clock slot.
    pub fn is_fast_clock_request(&self) -> bool {
        self.opcode() == OPC_RQ_SL_DATA && self.bytes[1] == FAST_CLOCK_SLOT
    }

    fn fast_clock_slot(opcode: u8, hour: u8, minute: u8, rate: u8, track_status: u8) -> Self {
        let [id1, id2] = HIVE_RAIL_CLOCK_ID;
        Self::with_checksum(&[
            opcode,
            0x0E,
            FAST_CLOCK_SLOT,
            rate & 0x7F,
            0x7F,
            0x7F,
            (255 - (60 - minute % 60)) & 0x7F,
            track_status & 0x7F,
            (256 - u16::from(24 - hour % 24)) as u8 & 0x7F,
            0x00,
            0x70,
            id1,
            id2,
        ])
    }

    /// A loco slot as the command station reports it: the answer to `LOCO_ADR`,
    /// `RQ_SL_DATA` and `MOVE_SLOTS`.
    pub fn loco_slot_read(
        slot: u8,
        status: u8,
        address: u16,
        speed: u8,
        dirf: u8,
        track_status: u8,
        sound: u8,
    ) -> Self {
        Self::with_checksum(&[
            OPC_SL_RD_DATA,
            0x0E,
            slot & 0x7F,
            status & 0x7F,
            (address & 0x7F) as u8,
            speed & 0x7F,
            dirf & 0x7F,
            track_status & 0x7F,
            0x00,
            (address >> 7 & 0x7F) as u8,
            sound & 0x7F,
            0x00,
            0x00,
        ])
    }

    /// Asks the slot master for a slot holding this loco address (`OPC_LOCO_ADR`).
    pub fn loco_address(address: u16) -> Self {
        Self::with_checksum(&[0xBF, (address >> 7 & 0x7F) as u8, (address & 0x7F) as u8])
    }

    /// A null move (source and destination equal) takes a slot: it becomes IN_USE.
    pub fn null_move(slot: u8) -> Self {
        Self::with_checksum(&[0xBA, slot & 0x7F, slot & 0x7F])
    }

    /// Writes a slot's status byte: busy bits `01` (COMMON) release a slot the slot master
    /// still refreshes, `00` (FREE) one it no longer refreshes.
    pub fn slot_status(slot: u8, status: u8) -> Self {
        Self::with_checksum(&[0xB5, slot & 0x7F, status & 0x7F])
    }

    /// Writes a slot (`OPC_WR_SL_DATA`) from the slot master's data for it, with this
    /// speed, direction and F0 to F8, and this throttle ID. A throttle marks the slot it
    /// holds with its ID this way (JMRI does), so it knows the slot as its own later, for
    /// example after a restart. `data` is the slot's `SL_RD_DATA` message.
    pub fn slot_write(data: &[u8], speed: u8, dirf: u8, snd: u8, id: [u8; 2]) -> Self {
        let mut body = [0_u8; 13];
        body.copy_from_slice(&data[..13]);
        body[0] = 0xEF;
        body[5] = speed & 0x7F;
        body[6] = dirf & 0x7F;
        body[10] = (data[10] & 0x70) | (snd & 0x0F);
        body[11] = id[0] & 0x7F;
        body[12] = id[1] & 0x7F;
        Self::with_checksum(&body)
    }

    pub fn loco_speed(slot: u8, speed: u8) -> Self {
        Self::with_checksum(&[0xA0, slot & 0x7F, speed & 0x7F])
    }

    /// Direction (bit 5) and F0 to F4.
    pub fn loco_dirf(slot: u8, dirf: u8) -> Self {
        Self::with_checksum(&[0xA1, slot & 0x7F, dirf & 0x7F])
    }

    /// F5 to F8.
    pub fn loco_sound(slot: u8, snd: u8) -> Self {
        Self::with_checksum(&[0xA2, slot & 0x7F, snd & 0x0F])
    }

    /// Messages about slots, whose numbers belong to one LocoNet connection; they are
    /// translated between connections, never passed on as they are.
    pub fn is_slot_message(&self) -> bool {
        matches!(
            self.opcode(),
            0xA0..=0xA3
                | 0xB4
                | 0xB5
                | 0xBA
                | 0xBB
                | 0xBF
                | 0xD4
                | 0xD5
                | 0xE6
                | 0xE7
                | 0xEE
                | 0xEF
        )
    }

    /// Asks the slot master for a slot's contents.
    pub fn request_slot_data(slot: u8) -> Self {
        Self::with_checksum(&[OPC_RQ_SL_DATA, slot & 0x7F, 0x00])
    }

    /// Acknowledges or refuses a request; `code` 0 means refused.
    pub fn long_ack(opcode: u8, code: u8) -> Self {
        Self::with_checksum(&[OPC_LONG_ACK, opcode & 0x7F, code & 0x7F])
    }

    /// Slot 123 contents from a slot read or write.
    pub fn fast_clock(&self) -> Option<FastClockSlot> {
        let b = &self.bytes;
        if !matches!(self.opcode(), OPC_SL_RD_DATA | OPC_WR_SL_DATA)
            || b.len() != 14
            || b[2] != FAST_CLOCK_SLOT
        {
            return None;
        }
        Some(FastClockSlot {
            rate: b[3],
            minute: (60 - ((255 - b[6]) & 0x7F) % 60) % 60,
            hour: (24 - ((256 - u16::from(b[8])) as u8 & 0x7F) % 24) % 24,
            valid: b[10] & 0x40 != 0,
            setter: [b[11], b[12]],
        })
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
        if let Some(clock) = self.fast_clock() {
            let name = if self.opcode() == OPC_WR_SL_DATA {
                "WR_SL_DATA"
            } else {
                "SL_RD_DATA"
            };
            let state = match (clock.valid, clock.rate) {
                (false, _) => "not set".to_owned(),
                (true, 0) => "frozen".to_owned(),
                (true, rate) => format!("x{rate}"),
            };
            return format!(
                "{name}  fast clock {:02}:{:02} {state}, set by {:02X} {:02X}",
                clock.hour, clock.minute, clock.setter[0], clock.setter[1]
            );
        }
        match self.opcode() {
            OPC_BUSY => "BUSY".into(),
            OPC_GPOFF => "GPOFF       power off".into(),
            OPC_GPON => "GPON        power on".into(),
            OPC_IDLE => "IDLE        emergency stop".into(),
            OPC_LOCO_SPD => format!("LOCO_SPD    slot {} speed {}", b[1], b[2]),
            // DIRF: 0 0 DIR F0 F4 F3 F2 F1; DIR set means reverse.
            OPC_LOCO_DIRF => format!(
                "LOCO_DIRF   slot {} {}, F0-F4 on: {}",
                b[1],
                if b[2] & 0x20 != 0 {
                    "reverse"
                } else {
                    "forward"
                },
                functions_on(
                    &[(0x10, 0), (0x01, 1), (0x02, 2), (0x04, 3), (0x08, 4)],
                    b[2]
                )
            ),
            OPC_LOCO_SND => format!(
                "LOCO_SND    slot {}, F5-F8 on: {}",
                b[1],
                functions_on(&[(0x01, 5), (0x02, 6), (0x04, 7), (0x08, 8)], b[2])
            ),
            OPC_LOCO_F9F12 => format!(
                "LOCO_F9F12  slot {}, F9-F12 on: {}",
                b[1],
                functions_on(&[(0x01, 9), (0x02, 10), (0x04, 11), (0x08, 12)], b[2])
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

/// Lists the functions whose bit is set, as `F1 F3`, or `none`.
fn functions_on(bits: &[(u8, u8)], value: u8) -> String {
    let on: Vec<String> = bits
        .iter()
        .filter(|(mask, _)| value & mask != 0)
        .map(|(_, number)| format!("F{number}"))
        .collect();
    if on.is_empty() {
        "none".into()
    } else {
        on.join(" ")
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

        // Recorded from Rocrail over LocoNet TCP: slot 1 forward with F1 and F2 on.
        let dirf = LocoNetMessage::from_hex("A1 01 03 5C").unwrap();
        assert_eq!(
            dirf.describe(),
            "LOCO_DIRF   slot 1 forward, F0-F4 on: F1 F2"
        );

        let reverse = LocoNetMessage::from_hex("A1 01 30 6F").unwrap();
        assert_eq!(
            reverse.describe(),
            "LOCO_DIRF   slot 1 reverse, F0-F4 on: F0"
        );

        let sound = LocoNetMessage::from_hex("A2 01 00 5C").unwrap();
        assert_eq!(sound.describe(), "LOCO_SND    slot 1, F5-F8 on: none");

        let f9 = LocoNetMessage::from_hex("A3 01 05 58").unwrap();
        assert_eq!(f9.describe(), "LOCO_F9F12  slot 1, F9-F12 on: F9 F11");
    }

    #[test]
    fn fast_clock_matches_rocrail_recording() {
        // Recorded from Rocrail over LocoNet TCP at 14:23:28, 14:23:59 and 14:25:01.
        for (hex, minute) in [
            ("EF 0E 7B 01 7F 7F 5A 04 76 00 70 7F 70 33", 23),
            ("EF 0E 7B 01 7F 7F 5B 04 76 00 70 7F 70 32", 24),
            ("EF 0E 7B 01 7F 7F 5C 04 76 00 70 7F 70 35", 25),
        ] {
            let clock = LocoNetMessage::from_hex(hex).unwrap().fast_clock().unwrap();
            assert_eq!((clock.hour, clock.minute, clock.rate), (14, minute, 1));
            assert!(clock.valid);
            assert_eq!(clock.setter, [0x7F, 0x70]);
        }

        // Rocrail's slot server before any clock was set.
        let unset = LocoNetMessage::from_hex("E7 0E 7B 00 7F 7F 00 04 00 00 70 7F 70 16").unwrap();
        assert_eq!(
            unset.describe(),
            "SL_RD_DATA  fast clock 00:53 frozen, set by 7F 70"
        );

        // Hive-Rail's write differs from Rocrail's only in the setter ID.
        let read = LocoNetMessage::fast_clock_read(9, 5, 4, 0x07);
        assert_eq!(
            read.describe(),
            "SL_RD_DATA  fast clock 09:05 x4, set by 7F 7E"
        );
        assert!(LocoNetMessage::from_hex("BB 7B 00 3F")
            .unwrap()
            .is_fast_clock_request());
        assert!(!LocoNetMessage::from_hex("BB 01 00 45")
            .unwrap()
            .is_fast_clock_request());

        let ours = LocoNetMessage::fast_clock_write(14, 23, 1, 0x04);
        assert_eq!(ours.to_hex(), "EF 0E 7B 01 7F 7F 5A 04 76 00 70 7F 7E 3D");
        for minute in 0..60 {
            let clock = LocoNetMessage::fast_clock_write(23, minute, 4, 0x07)
                .fast_clock()
                .unwrap();
            assert_eq!((clock.hour, clock.minute), (23, minute));
        }
        assert!(LocoNetMessage::from_bytes(ours.as_bytes()).is_ok());
        assert_eq!(ours.fast_clock().unwrap().setter, super::HIVE_RAIL_CLOCK_ID);
    }
}
