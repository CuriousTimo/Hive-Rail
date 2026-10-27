use thiserror::Error;

const HEADER_LENGTH: usize = 8;
const STATIONARY_GROUP: u8 = 3;
const STATIONARY_IDENTIFY_ACTION: u8 = 8;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RocNetError {
    #[error("RocNet frame is shorter than its {HEADER_LENGTH}-byte header")]
    ShortHeader,
    #[error("RocNet frame declares {declared} data bytes but contains {actual}")]
    DataLengthMismatch { declared: usize, actual: usize },
    #[error("ASCII HEX RocNet frame must start with '@'")]
    MissingAsciiStart,
    #[error("ASCII HEX RocNet frame must contain complete hexadecimal byte pairs")]
    InvalidAsciiLength,
    #[error("invalid hexadecimal byte at offset {offset}")]
    InvalidAsciiByte { offset: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RocNetFrame {
    bytes: Vec<u8>,
}

impl RocNetFrame {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, RocNetError> {
        if bytes.len() < HEADER_LENGTH {
            return Err(RocNetError::ShortHeader);
        }

        let declared = usize::from(bytes[7]);
        let actual = bytes.len() - HEADER_LENGTH;
        if declared != actual {
            return Err(RocNetError::DataLengthMismatch { declared, actual });
        }

        Ok(Self {
            bytes: bytes.to_vec(),
        })
    }

    pub fn from_ascii_hex(input: &str) -> Result<Self, RocNetError> {
        let encoded = input
            .trim()
            .strip_prefix('@')
            .ok_or(RocNetError::MissingAsciiStart)?;
        if encoded.is_empty() || encoded.len() % 2 != 0 {
            return Err(RocNetError::InvalidAsciiLength);
        }

        let mut bytes = Vec::with_capacity(encoded.len() / 2);
        for (offset, pair) in encoded.as_bytes().as_chunks::<2>().0.iter().enumerate() {
            let pair =
                std::str::from_utf8(pair).map_err(|_| RocNetError::InvalidAsciiByte { offset })?;
            let byte = u8::from_str_radix(pair, 16)
                .map_err(|_| RocNetError::InvalidAsciiByte { offset })?;
            bytes.push(byte);
        }
        Self::from_bytes(&bytes)
    }

    pub fn stationary_identify_request(network_id: u8, sender_id: u16) -> Self {
        let [sender_high, sender_low] = sender_id.to_be_bytes();
        Self {
            bytes: vec![
                network_id,
                0,
                0,
                sender_high,
                sender_low,
                STATIONARY_GROUP,
                STATIONARY_IDENTIFY_ACTION,
                0,
            ],
        }
    }

    pub fn to_ascii_hex(&self) -> String {
        let encoded = self
            .bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>();
        format!("@{encoded}")
    }

    pub fn network_id(&self) -> u8 {
        self.bytes[0]
    }

    pub fn recipient_id(&self) -> u16 {
        u16::from_be_bytes([self.bytes[1], self.bytes[2]])
    }

    pub fn sender_id(&self) -> u16 {
        u16::from_be_bytes([self.bytes[3], self.bytes[4]])
    }

    pub fn group(&self) -> u8 {
        self.bytes[5]
    }

    pub fn code(&self) -> u8 {
        self.bytes[6]
    }

    pub fn data(&self) -> &[u8] {
        &self.bytes[HEADER_LENGTH..]
    }
}

#[cfg(test)]
mod tests {
    use super::{RocNetError, RocNetFrame};

    #[test]
    fn parses_and_preserves_reference_broadcast_frame() {
        let frame = RocNetFrame::from_ascii_hex("@0000000001020203000000").unwrap();

        assert_eq!(frame.to_ascii_hex(), "@0000000001020203000000");
        assert_eq!(frame.network_id(), 0);
        assert_eq!(frame.recipient_id(), 0);
        assert_eq!(frame.sender_id(), 1);
        assert_eq!(frame.group(), 2);
        assert_eq!(frame.code(), 2);
        assert_eq!(frame.data(), &[0, 0, 0]);
    }

    #[test]
    fn parses_and_preserves_reference_unicast_frame() {
        let frame = RocNetFrame::from_ascii_hex("@0001230001020203304140").unwrap();

        assert_eq!(frame.recipient_id(), 291);
        assert_eq!(frame.sender_id(), 1);
        assert_eq!(frame.data(), &[0x30, 0x41, 0x40]);
        assert_eq!(
            RocNetFrame::from_ascii_hex(&frame.to_ascii_hex()).unwrap(),
            frame
        );
    }

    #[test]
    fn creates_stationary_identify_broadcast() {
        let frame = RocNetFrame::stationary_identify_request(0, 42);

        assert_eq!(frame.network_id(), 0);
        assert_eq!(frame.recipient_id(), 0);
        assert_eq!(frame.sender_id(), 42);
        assert_eq!(frame.group(), 3);
        assert_eq!(frame.code(), 8);
        assert!(frame.data().is_empty());
    }

    #[test]
    fn rejects_invalid_lengths_and_hex() {
        assert_eq!(
            RocNetFrame::from_bytes(&[0; 7]),
            Err(RocNetError::ShortHeader)
        );
        assert!(matches!(
            RocNetFrame::from_bytes(&[0, 0, 0, 0, 0, 0, 0, 1]),
            Err(RocNetError::DataLengthMismatch { .. })
        ));
        assert!(matches!(
            RocNetFrame::from_ascii_hex("0000"),
            Err(RocNetError::MissingAsciiStart)
        ));
        assert!(matches!(
            RocNetFrame::from_ascii_hex("@0"),
            Err(RocNetError::InvalidAsciiLength)
        ));
        assert!(matches!(
            RocNetFrame::from_ascii_hex("@GG"),
            Err(RocNetError::InvalidAsciiByte { .. })
        ));
    }
}
