use crate::power::PowerRequest;
use thiserror::Error;

const HEADER_LENGTH: usize = 8;
const COMMAND_STATION_GROUP: u8 = 1;
const CS_QUERY_ACTION: u8 = 1;
const CS_TRACK_POWER_ACTION: u8 = 2;
const CS_HALT_ACTION: u8 = 3;
const MOBILE_GROUP: u8 = 2;
const MOBILE_VELOCITY_ACTION: u8 = 2;
const STATIONARY_GROUP: u8 = 3;
const STATIONARY_IDENTIFY_ACTION: u8 = 8;
const CLOCK_GROUP: u8 = 7;
const CLOCK_SET_ACTION: u8 = 1;
const CLOCK_SYNC_ACTION: u8 = 2;

/// RocNet group 7 clock action: `set` changes the clock, `sync` restores phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockAction {
    Set,
    Sync,
}

/// Group 7 clock data. A divider of 0 means the clock is paused (Hive-Rail convention,
/// matching LocoNet's rate 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockUpdate {
    pub action: ClockAction,
    pub hour: u8,
    pub minute: u8,
    pub divider: u8,
}

/// Bits 6 and 5 of the code byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    Request,
    Event,
    Reply,
}

impl FrameType {
    fn bits(self) -> u8 {
        match self {
            Self::Request => 0,
            Self::Event => 1,
            Self::Reply => 2,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Event => "event",
            Self::Reply => "reply",
        }
    }
}

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

    fn new(
        network_id: u8,
        recipient_id: u16,
        sender_id: u16,
        group: u8,
        frame_type: FrameType,
        action: u8,
        data: &[u8],
    ) -> Self {
        let [recipient_high, recipient_low] = recipient_id.to_be_bytes();
        let [sender_high, sender_low] = sender_id.to_be_bytes();
        let mut bytes = vec![
            network_id,
            recipient_high,
            recipient_low,
            sender_high,
            sender_low,
            group,
            frame_type.bits() << 5 | action & 0x1F,
            u8::try_from(data.len()).expect("RocNet data is at most 255 bytes"),
        ];
        bytes.extend_from_slice(data);
        Self { bytes }
    }

    pub fn stationary_identify_request(network_id: u8, sender_id: u16) -> Self {
        Self::new(
            network_id,
            0,
            sender_id,
            STATIONARY_GROUP,
            FrameType::Request,
            STATIONARY_IDENTIFY_ACTION,
            &[],
        )
    }

    /// Command station track power or halt request to every node.
    pub fn power_request(sender_id: u16, request: PowerRequest) -> Self {
        let (action, data): (u8, &[u8]) = match request {
            PowerRequest::On => (CS_TRACK_POWER_ACTION, &[1]),
            PowerRequest::Off => (CS_TRACK_POWER_ACTION, &[0]),
            PowerRequest::Halt => (CS_HALT_ACTION, &[]),
        };
        Self::new(
            0,
            0,
            sender_id,
            COMMAND_STATION_GROUP,
            FrameType::Request,
            action,
            data,
        )
    }

    /// Command station status event: bit 0 power, bit 1 idle, bit 2 programming track busy.
    pub fn power_status_event(sender_id: u16, status: u8) -> Self {
        Self::new(
            0,
            0,
            sender_id,
            COMMAND_STATION_GROUP,
            FrameType::Event,
            CS_QUERY_ACTION,
            &[status],
        )
    }

    /// Group 7 clock: year (2 bytes), month, day, hour, minute, temperature, divider,
    /// brightness. Hive-Rail keeps no date, temperature or brightness, so those are 0.
    pub fn clock(sender_id: u16, update: ClockUpdate) -> Self {
        let action = match update.action {
            ClockAction::Set => CLOCK_SET_ACTION,
            ClockAction::Sync => CLOCK_SYNC_ACTION,
        };
        Self::new(
            0,
            0,
            sender_id,
            CLOCK_GROUP,
            FrameType::Request,
            action,
            &[0, 0, 0, 0, update.hour, update.minute, 0, update.divider, 0],
        )
    }

    pub fn clock_update(&self) -> Option<ClockUpdate> {
        if self.group() != CLOCK_GROUP {
            return None;
        }
        let action = match self.action() {
            CLOCK_SET_ACTION => ClockAction::Set,
            CLOCK_SYNC_ACTION => ClockAction::Sync,
            _ => return None,
        };
        let data = self.data();
        (data.len() >= 8 && data[4] < 24 && data[5] < 60).then(|| ClockUpdate {
            action,
            hour: data[4],
            minute: data[5],
            divider: data[7],
        })
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

    pub fn frame_type(&self) -> Option<FrameType> {
        match (self.code() >> 5) & 0x03 {
            0 => Some(FrameType::Request),
            1 => Some(FrameType::Event),
            2 => Some(FrameType::Reply),
            _ => None,
        }
    }

    pub fn action(&self) -> u8 {
        self.code() & 0x1F
    }

    pub fn power_request_kind(&self) -> Option<PowerRequest> {
        if self.group() != COMMAND_STATION_GROUP || self.frame_type() != Some(FrameType::Request) {
            return None;
        }
        match (self.action(), self.data().first()) {
            (CS_TRACK_POWER_ACTION, Some(1)) => Some(PowerRequest::On),
            (CS_TRACK_POWER_ACTION, Some(0)) => Some(PowerRequest::Off),
            (CS_HALT_ACTION, _) => Some(PowerRequest::Halt),
            _ => None,
        }
    }

    /// Short decoded form for the CLI log.
    pub fn describe(&self) -> String {
        let kind = self.frame_type().map_or("invalid type", FrameType::label);
        let first = self.data().first().copied();
        match (self.group(), self.action(), self.frame_type()) {
            (COMMAND_STATION_GROUP, CS_TRACK_POWER_ACTION, Some(FrameType::Request)) => format!(
                "cs track power {kind} {}",
                if first == Some(1) { "on" } else { "off" }
            ),
            (COMMAND_STATION_GROUP, CS_HALT_ACTION, Some(FrameType::Request)) => {
                format!("cs halt {kind}")
            }
            (COMMAND_STATION_GROUP, CS_QUERY_ACTION, Some(FrameType::Request)) => {
                format!("cs query {kind}")
            }
            (
                COMMAND_STATION_GROUP,
                CS_QUERY_ACTION | CS_TRACK_POWER_ACTION | CS_HALT_ACTION,
                _,
            ) => format!(
                "cs status {kind}: {}",
                describe_status(first.unwrap_or_default())
            ),
            (MOBILE_GROUP, MOBILE_VELOCITY_ACTION, _) => format!(
                "mobile velocity {kind} loco {} speed {}",
                self.recipient_id(),
                first.unwrap_or_default()
            ),
            (CLOCK_GROUP, _, _) => match self.clock_update() {
                Some(clock) => format!(
                    "clock {} {:02}:{:02} {}",
                    match clock.action {
                        ClockAction::Set => "set",
                        ClockAction::Sync => "sync",
                    },
                    clock.hour,
                    clock.minute,
                    match clock.divider {
                        0 => "paused".to_owned(),
                        divider => format!("x{divider}"),
                    }
                ),
                None => format!("clock action {} {kind}", self.action()),
            },
            (STATIONARY_GROUP, STATIONARY_IDENTIFY_ACTION, _) => {
                format!("stationary identify {kind}")
            }
            (group, action, _) => format!("group {group} action {action} {kind}"),
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

fn describe_status(status: u8) -> String {
    let mut text = String::from(if status & 0x01 != 0 {
        "power on"
    } else {
        "power off"
    });
    if status & 0x02 != 0 {
        text.push_str(", idle");
    }
    if status & 0x04 != 0 {
        text.push_str(", programming track busy");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{ClockAction, ClockUpdate, FrameType, RocNetError, RocNetFrame};
    use crate::power::PowerRequest;

    #[test]
    fn clock_frames_round_trip() {
        let update = ClockUpdate {
            action: ClockAction::Sync,
            hour: 6,
            minute: 42,
            divider: 4,
        };
        let frame = RocNetFrame::clock(101, update);
        assert_eq!(
            frame.to_hex(),
            "00 00 00 00 65 07 02 09 00 00 00 00 06 2A 00 04 00"
        );
        assert_eq!(frame.clock_update(), Some(update));
        assert_eq!(frame.describe(), "clock sync 06:42 x4");
    }

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
    fn builds_the_reference_track_power_on_frame() {
        // Example from the RocNet protocol page: track power on sent by the server (ID 1).
        let reference = RocNetFrame::from_ascii_hex("@000000000101020101").unwrap();
        let built = RocNetFrame::power_request(1, PowerRequest::On);

        assert_eq!(built, reference);
        assert_eq!(built.frame_type(), Some(FrameType::Request));
        assert_eq!(built.action(), 2);
        assert_eq!(built.power_request_kind(), Some(PowerRequest::On));
        assert_eq!(built.describe(), "cs track power request on");
        assert_eq!(built.to_hex(), "00 00 00 00 01 01 02 01 01");
    }

    #[test]
    fn power_off_halt_and_status_event_round_trip() {
        let off = RocNetFrame::power_request(101, PowerRequest::Off);
        assert_eq!(off.power_request_kind(), Some(PowerRequest::Off));
        assert_eq!(off.describe(), "cs track power request off");

        let halt = RocNetFrame::power_request(101, PowerRequest::Halt);
        assert_eq!(halt.power_request_kind(), Some(PowerRequest::Halt));

        let status = RocNetFrame::power_status_event(101, 0x03);
        assert_eq!(status.to_hex(), "00 00 00 00 65 01 21 01 03");
        assert_eq!(status.frame_type(), Some(FrameType::Event));
        assert_eq!(status.power_request_kind(), None);
        assert_eq!(status.describe(), "cs status event: power on, idle");
        assert_eq!(RocNetFrame::from_bytes(status.as_bytes()).unwrap(), status);
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
