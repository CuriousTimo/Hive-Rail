//! DCC-EX EX-CommandStation driver over USB serial or TCP (WiFi, port 2560).
//!
//! DCC-EX speaks text commands in angle brackets: `<1>` power on, `<0>` power off,
//! `<!>` emergency stop, `<JC minutes rate>` fast clock. Power changes are broadcast as
//! `<p0>`/`<p1>`. A short is not broadcast as power off: the motor driver cuts the track
//! itself, writes `<* TRACK A POWER OVERLOAD ... *>` and retries until `<* TRACK A NORMAL
//! ... *>`. Without a motor shield DCC-EX has no tracks and answers power commands with
//! nothing at all.

use crate::power::PowerRequest;
use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    net::TcpStream,
    thread,
    time::Duration,
};
use tokio::sync::mpsc;

const BAUD_RATE: u32 = 115_200;
const READ_TIMEOUT: Duration = Duration::from_millis(50);
const RECONNECT_DELAY: Duration = Duration::from_secs(2);

/// One message from DCC-EX, without its angle brackets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DccExMessage {
    /// `<p1>`, `<p0 A>`, `<p1 MAIN>`: `track` is None for the overall state.
    Power {
        on: bool,
        track: Option<String>,
    },
    /// `<iDCC-EX V-5.6.6 / ESP32 / ...>`
    Version(String),
    /// `<jC minutes [rate]>`
    Clock {
        minutes: u16,
        rate: Option<u8>,
    },
    /// `<* TRACK A POWER OVERLOAD ... *>`: the track is off until DCC-EX retries.
    Overload {
        track: char,
        detail: String,
    },
    /// `<* TRACK A NORMAL ... *>`: power restored after an overload.
    TrackNormal {
        track: char,
    },
    /// Any other `<* ... *>` diagnostic.
    Diagnostic(String),
    /// `<= A MAIN>`: the mode of one track (MAIN, MAIN_INV, PROG, DC, NONE, ...).
    TrackMode {
        track: char,
        mode: String,
    },
    /// `<u protocol>`: the Hive-Rail board add-on answering `<U 0>`.
    Board {
        protocol: u16,
    },
    /// `<@ ...>` text for the command station's own display.
    Display(String),
    Other(String),
}

impl DccExMessage {
    pub fn parse(frame: &str) -> Self {
        if let Some(text) = frame.strip_prefix('*') {
            let text = text.trim_end_matches('*').trim();
            let words: Vec<&str> = text.split_whitespace().collect();
            if let ["TRACK", track, rest @ ..] = words.as_slice() {
                if let Some(track) = track.chars().next().filter(|_| track.len() == 1) {
                    if rest.starts_with(&["POWER", "OVERLOAD"])
                        || rest.starts_with(&["FAULT", "PIN"])
                    {
                        return Self::Overload {
                            track,
                            detail: text.to_owned(),
                        };
                    }
                    if rest.first() == Some(&"NORMAL") {
                        return Self::TrackNormal { track };
                    }
                }
            }
            return Self::Diagnostic(text.to_owned());
        }
        if let Some(text) = frame.strip_prefix('@') {
            return Self::Display(text.trim().to_owned());
        }
        if let Some(text) = frame.strip_prefix('i') {
            return Self::Version(text.trim().to_owned());
        }
        if let Some(rest) = frame.strip_prefix('p') {
            let mut parts = rest.split_whitespace();
            match parts.next() {
                Some("0") | Some("1") => {
                    return Self::Power {
                        on: rest.trim_start().starts_with('1'),
                        track: parts.next().map(str::to_owned),
                    };
                }
                _ => return Self::Other(frame.to_owned()),
            }
        }
        if let Some(rest) = frame.strip_prefix('u') {
            if let Ok(protocol) = rest.trim().parse() {
                return Self::Board { protocol };
            }
        }
        if let Some(rest) = frame.strip_prefix('=') {
            let mut parts = rest.split_whitespace();
            if let (Some(track), Some(mode)) = (parts.next(), parts.next()) {
                if let Some(track) = track.chars().next().filter(|_| track.len() == 1) {
                    return Self::TrackMode {
                        track,
                        mode: mode.to_owned(),
                    };
                }
            }
        }
        if let Some(rest) = frame.strip_prefix("jC") {
            let mut numbers = rest.split_whitespace().map(str::parse::<u16>);
            if let Some(Ok(minutes)) = numbers.next() {
                let rate = numbers
                    .next()
                    .and_then(Result::ok)
                    .and_then(|rate| u8::try_from(rate).ok());
                return Self::Clock { minutes, rate };
            }
        }
        Self::Other(frame.to_owned())
    }

    /// Short decoded form for the CLI log.
    pub fn describe(&self) -> String {
        match self {
            Self::Power { on, track } => format!(
                "power {}{}",
                if *on { "on" } else { "off" },
                track
                    .as_deref()
                    .map_or(String::new(), |track| format!(" track {track}"))
            ),
            Self::Version(version) => format!("version {version}"),
            Self::Clock { minutes, rate } => format!(
                "fast clock {:02}:{:02}{}",
                minutes / 60 % 24,
                minutes % 60,
                rate.map_or(String::new(), |rate| format!(" x{rate}"))
            ),
            Self::Overload { track, .. } => format!("OVERLOAD on track {track}"),
            Self::TrackNormal { track } => format!("track {track} normal again"),
            Self::Diagnostic(text) => format!("diag {text}"),
            Self::Display(text) => format!("display {text}"),
            Self::TrackMode { track, mode } => format!("track {track} is {mode}"),
            Self::Board { protocol } => format!("Hive-Rail board add-on, protocol {protocol}"),
            Self::Other(text) => format!("unknown <{text}>"),
        }
    }
}

/// The district's power is the power of its MAIN tracks. DCC-EX reports power per track
/// (`<p1 A>`), and only sends an overall `<p1>`/`<p0>` when all tracks agree, so a MAIN-only
/// power on (EX-RAIL `POWERON`, `<1 MAIN>`) has no overall `<p1>`.
#[derive(Debug, Default)]
pub struct DistrictTracks {
    /// Track letter to (is a MAIN track, has power).
    tracks: BTreeMap<char, (bool, bool)>,
}

impl DistrictTracks {
    /// Updates the track state and returns the district's power if this message tells it.
    pub fn update(&mut self, message: &DccExMessage) -> Option<bool> {
        match message {
            DccExMessage::TrackMode { track, mode } => {
                self.tracks.entry(*track).or_default().0 = mode.starts_with("MAIN");
                None
            }
            DccExMessage::Power {
                on,
                track: Some(track),
            } => {
                let mut letters = track.chars();
                let (Some(letter), None) = (letters.next(), letters.next()) else {
                    return None; // MAIN, PROG and JOIN summarise the per-track lines
                };
                self.tracks.entry(letter).or_default().1 = *on;
                self.main_power()
            }
            // Without known MAIN tracks (no motor shield) the overall state is all there is.
            DccExMessage::Power { on, track: None } => match self.main_power() {
                Some(_) => None,
                None => Some(*on),
            },
            _ => None,
        }
    }

    fn main_power(&self) -> Option<bool> {
        let mut main = self.tracks.values().filter(|(main, _)| *main).peekable();
        main.peek()?;
        Some(main.any(|(_, on)| *on))
    }
}

pub fn power_command(request: PowerRequest) -> &'static str {
    match request {
        // MAIN only, so a local programming track stays independent; off is all tracks.
        PowerRequest::On => "<1 MAIN>",
        PowerRequest::Off => "<0>",
        PowerRequest::Halt => "<!>",
    }
}

/// What the node board's screen and status LEDs show, sent once a second; it is also the
/// board's link heartbeat. Only sent to a board that answered `<U 0>`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BoardStatus {
    pub rocnet_id: u16,
    pub nodes: usize,
    pub locos: usize,
    /// Locos in use on the whole layout (the loco register).
    pub layout_locos: usize,
    pub allocator: bool,
    pub register_in_sync: bool,
    pub register_compared: bool,
    pub emergency_stop: bool,
    /// The last loco driven on this module: address, LocoNet speed step, forward, F0-F4.
    pub loco: Option<(u16, u8, bool, u8)>,
}

impl BoardStatus {
    pub fn command(&self) -> String {
        let flags = u8::from(self.allocator)
            | u8::from(self.register_in_sync) << 1
            | u8::from(self.emergency_stop) << 2
            | u8::from(self.register_compared) << 3;
        let (address, speed, forward, functions) = self.loco.unwrap_or((0, 0, true, 0));
        // DCC-EX takes at most 9 parameters, so F0-F4 and the direction share one.
        let dirf = functions & 0x1F | u8::from(forward) << 7;
        format!(
            "<U 1 {} {} {} {} {flags} {address} {speed} {dirf}>",
            self.rocnet_id, self.nodes, self.locos, self.layout_locos
        )
    }
}

pub fn clock_command(minute_of_day: u16, rate: u8) -> String {
    format!("<JC {minute_of_day} {rate}>")
}

/// Splits DCC-EX output into frames. Diagnostics (`<* ... *>`) may span lines and
/// contain `>`; everything outside angle brackets is ignored.
#[derive(Debug, Default)]
pub struct FrameDecoder {
    buffer: String,
}

impl FrameDecoder {
    pub fn push(&mut self, text: &str) -> Vec<String> {
        self.buffer.push_str(text);
        let mut frames = Vec::new();
        loop {
            let Some(start) = self.buffer.find('<') else {
                self.buffer.clear();
                break;
            };
            self.buffer.drain(..start);
            let end = if self.buffer.starts_with("<*") {
                self.buffer.find("*>").map(|end| end + 2)
            } else {
                self.buffer.find('>').map(|end| end + 1)
            };
            let Some(end) = end else { break };
            let frame: String = self.buffer.drain(..end).collect();
            frames.push(frame[1..frame.len() - 1].trim().to_owned());
        }
        frames
    }
}

pub enum DccExEvent {
    Connected(String),
    Disconnected(String),
    Received(DccExMessage),
}

#[derive(Clone)]
pub struct DccExHandle {
    commands: mpsc::UnboundedSender<String>,
}

impl DccExHandle {
    /// Queues a command; it is dropped if DCC-EX is not connected at the moment.
    pub fn send(&self, command: &str) {
        let _ = self.commands.send(command.to_owned());
    }
}

/// Connects to `target`: a serial port (`/dev/cu.usbserial-0001`, `COM3`) or a TCP
/// address with a colon (`192.168.4.1:2560`). Reconnects on its own.
pub fn start(target: String) -> (DccExHandle, mpsc::Receiver<DccExEvent>) {
    let (commands, command_receiver) = mpsc::unbounded_channel();
    let (events, event_receiver) = mpsc::channel(256);
    thread::spawn(move || run(target, command_receiver, events));
    (DccExHandle { commands }, event_receiver)
}

trait Link: Read + Write + Send {}
impl<T: Read + Write + Send> Link for T {}

fn connect(target: &str) -> io::Result<Box<dyn Link>> {
    if target.contains(':') && !target.starts_with('/') {
        let stream = TcpStream::connect(target)?;
        stream.set_read_timeout(Some(READ_TIMEOUT))?;
        stream.set_nodelay(true)?;
        Ok(Box::new(stream))
    } else {
        let port = serialport::new(target, BAUD_RATE)
            .timeout(READ_TIMEOUT)
            .open()?;
        Ok(Box::new(port))
    }
}

fn run(
    target: String,
    mut commands: mpsc::UnboundedReceiver<String>,
    events: mpsc::Sender<DccExEvent>,
) {
    let mut last_error = None;
    loop {
        match connect(&target) {
            Ok(mut link) => {
                last_error = None;
                if events
                    .blocking_send(DccExEvent::Connected(target.clone()))
                    .is_err()
                {
                    return;
                }
                // Commands queued while disconnected are stale.
                while commands.try_recv().is_ok() {}
                let reason = serve(link.as_mut(), &mut commands, &events);
                if events
                    .blocking_send(DccExEvent::Disconnected(reason))
                    .is_err()
                {
                    return;
                }
            }
            Err(error) => {
                // Report a failing connection once, not on every retry.
                let error = error.to_string();
                if last_error.as_ref() != Some(&error) {
                    if events
                        .blocking_send(DccExEvent::Disconnected(format!(
                            "cannot open {target}: {error}"
                        )))
                        .is_err()
                    {
                        return;
                    }
                    last_error = Some(error);
                }
            }
        }
        thread::sleep(RECONNECT_DELAY);
        if commands.is_closed() {
            return;
        }
    }
}

fn serve(
    link: &mut dyn Link,
    commands: &mut mpsc::UnboundedReceiver<String>,
    events: &mpsc::Sender<DccExEvent>,
) -> String {
    let mut decoder = FrameDecoder::default();
    let mut buffer = [0_u8; 1024];
    // Ask for the version, power state, track modes and the board add-on; opening the serial port usually resets
    // the board, which then reports the same after booting.
    // <U 0> asks for the Hive-Rail board add-on; a stock DCC-EX answers <X>.
    if let Err(error) = link.write_all(b"<s><=><U 0>") {
        return error.to_string();
    }
    loop {
        while let Ok(command) = commands.try_recv() {
            if let Err(error) = link.write_all(command.as_bytes()) {
                return error.to_string();
            }
        }
        match link.read(&mut buffer) {
            Ok(0) => return "connection closed".to_owned(),
            Ok(count) => {
                for frame in decoder.push(&String::from_utf8_lossy(&buffer[..count])) {
                    let message = DccExMessage::parse(&frame);
                    if events.blocking_send(DccExEvent::Received(message)).is_err() {
                        return "node stopped".to_owned();
                    }
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return error.to_string(),
        }
        if commands.is_closed() {
            return "node stopped".to_owned();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{clock_command, DccExMessage, DistrictTracks, FrameDecoder};

    #[test]
    fn district_power_follows_main_tracks() {
        let mut district = DistrictTracks::default();
        let mut feed = |frame: &str| district.update(&DccExMessage::parse(frame));

        // Without shield there are no tracks: only the overall state is known.
        assert_eq!(feed("p0"), Some(false));

        // Recorded on the 2021 board: modes at boot, then the green button (EX-RAIL
        // POWERON, MAIN only) and the red button (POWEROFF).
        assert_eq!(feed("= A MAIN"), None);
        assert_eq!(feed("= B PROG"), None);
        assert_eq!(feed("p1 A"), Some(true));
        assert_eq!(feed("p0 B"), Some(true));
        assert_eq!(feed("p1 MAIN"), None);
        assert_eq!(feed("p0 A"), Some(false));
        assert_eq!(feed("p0 B"), Some(false));
        assert_eq!(feed("p0"), None);

        // Only the programming track on: the district stays off.
        assert_eq!(feed("p1 B"), Some(false));
    }

    #[test]
    fn decodes_recorded_esp32_boot() {
        // Recorded from DCC-EX v5.6.6 on an ESP32 without a motor shield.
        let boot = "<* License GPLv3 fsf.org (c) dcc-ex.com *>\r\n\
            <* Scanning I2C bus for devices...\r\n   I2CManager: Using Wire library\r\n  \
            Device found at 0x3C, OLED Display or HMC583L Magnetometer?\r\n*>\r\n\
            <@ 0 0 \"DCC-EX v5.6.6\">\r\n\
            <iDCC-EX V-5.6.6 / ESP32 / No shield at all G-master-202609231425Z>\r\n\
            <p0>\r\n<@ 0 2 \"PWR Off\">\r\n";
        let mut decoder = FrameDecoder::default();
        // Split in the middle of the multi-line diagnostic, as a serial read might.
        let mut frames = decoder.push(&boot[..60]);
        frames.extend(decoder.push(&boot[60..]));
        let messages: Vec<DccExMessage> = frames
            .iter()
            .map(|frame| DccExMessage::parse(frame))
            .collect();

        assert_eq!(messages.len(), 6);
        assert!(matches!(&messages[1], DccExMessage::Diagnostic(text) if text.contains("0x3C")));
        assert_eq!(
            messages[3],
            DccExMessage::Version(
                "DCC-EX V-5.6.6 / ESP32 / No shield at all G-master-202609231425Z".to_owned()
            )
        );
        assert_eq!(
            messages[4],
            DccExMessage::Power {
                on: false,
                track: None
            }
        );
        assert!(matches!(messages[5], DccExMessage::Display(_)));
    }

    #[test]
    fn decodes_power_clock_and_overload() {
        assert_eq!(
            DccExMessage::parse("p1 MAIN"),
            DccExMessage::Power {
                on: true,
                track: Some("MAIN".to_owned())
            }
        );
        assert_eq!(DccExMessage::parse("p0 A").describe(), "power off track A");
        assert_eq!(
            DccExMessage::parse("jC 375 4"),
            DccExMessage::Clock {
                minutes: 375,
                rate: Some(4)
            }
        );
        assert_eq!(DccExMessage::parse("jC 375").describe(), "fast clock 06:15");
        assert_eq!(clock_command(375, 4), "<JC 375 4>");
        assert_eq!(
            DccExMessage::parse("u 1"),
            DccExMessage::Board { protocol: 1 }
        );
        let status = super::BoardStatus {
            rocnet_id: 101,
            nodes: 4,
            locos: 1,
            layout_locos: 5,
            allocator: true,
            register_in_sync: true,
            register_compared: true,
            emergency_stop: false,
            loco: Some((1234, 41, false, 0b0_0001)),
        };
        // At most 9 parameters, or DCC-EX rejects the command with <X>.
        assert_eq!(status.command(), "<U 1 101 4 1 5 11 1234 41 1>");
        assert_eq!(status.command().split_whitespace().count() - 1, 9);

        // Message formats from the DCC-EX motor driver source.
        let overload = DccExMessage::parse(
            "* TRACK A POWER OVERLOAD 3200mA (max 2000mA) detected after 100ms. Pause 40ms *",
        );
        assert!(matches!(
            overload,
            DccExMessage::Overload { track: 'A', .. }
        ));
        assert_eq!(
            DccExMessage::parse("* TRACK B FAULT PIN detected after 2ms. Pause 80ms) *"),
            DccExMessage::Overload {
                track: 'B',
                detail: "TRACK B FAULT PIN detected after 2ms. Pause 80ms)".to_owned()
            }
        );
        assert_eq!(
            DccExMessage::parse("* TRACK A NORMAL (after 20ms/45ms) 120mA *"),
            DccExMessage::TrackNormal { track: 'A' }
        );
        assert!(matches!(
            DccExMessage::parse("* TRACK A ALERT FAULT *"),
            DccExMessage::Diagnostic(_)
        ));
    }
}
