//! LocoNet over a serial port: a LocoNet interface that passes raw LocoNet bytes, such as
//! the RS232 port of an Intellibox I (65000, with SO 2 = 4) or a LocoBuffer. Reconnects on
//! its own and gives the node the same handle and events as the LbServer client, so the
//! port is the node's LocoNet bus.
//!
//! Some interfaces echo what the node sends (a LocoBuffer), others do not (the Intellibox);
//! an echo that comes back is dropped, so the node only sees what other bus devices sent.

use crate::lbclient::LbClientHandle;
use crate::lbserver::LbEvent;
use crate::loconet::{LocoNetMessage, StreamDecoder};
use serialport::FlowControl;
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr},
    thread,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

/// The Intellibox I's LocoNet interface: 19200 baud with RTS/CTS (JMRI's settings).
pub const DEFAULT_BAUD_RATE: u32 = 19_200;
const READ_TIMEOUT: Duration = Duration::from_millis(50);
const RECONNECT_DELAY: Duration = Duration::from_secs(2);
/// An interface that echoes does so within milliseconds; an interface that does not echo
/// must not have a later message of the same content dropped.
const ECHO_WINDOW: Duration = Duration::from_millis(300);

/// A serial port has no network address; the node names the bus by its port instead.
const NO_PEER: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);

/// Opens `port` (`/dev/cu.usbserial-0001`, `COM3`) and keeps reopening it.
pub fn start(
    port: String,
    baud_rate: u32,
    flow_control: FlowControl,
) -> (LbClientHandle, mpsc::Receiver<LbEvent>) {
    let (outgoing, outgoing_receiver) = mpsc::unbounded_channel();
    let (events, event_receiver) = mpsc::channel(256);
    thread::spawn(move || run(port, baud_rate, flow_control, outgoing_receiver, events));
    (LbClientHandle::new(outgoing), event_receiver)
}

/// Parses `--loconet-serial-flow`: `hardware` (RTS/CTS) or `none`.
pub fn parse_flow_control(value: &str) -> Option<FlowControl> {
    match value {
        "hardware" => Some(FlowControl::Hardware),
        "none" => Some(FlowControl::None),
        _ => None,
    }
}

trait Link: Read + Write + Send {}
impl<T: Read + Write + Send> Link for T {}

fn run(
    port: String,
    baud_rate: u32,
    flow_control: FlowControl,
    mut outgoing: mpsc::UnboundedReceiver<LocoNetMessage>,
    events: mpsc::Sender<LbEvent>,
) {
    let mut last_error = None;
    loop {
        let opened = serialport::new(&port, baud_rate)
            .flow_control(flow_control)
            .timeout(READ_TIMEOUT)
            .open();
        match opened {
            Ok(mut link) => {
                last_error = None;
                if events.blocking_send(LbEvent::Connected(NO_PEER)).is_err() {
                    return;
                }
                // Messages queued while disconnected are stale.
                while outgoing.try_recv().is_ok() {}
                let reason = serve(&mut link, &mut outgoing, &events);
                let disconnected = LbEvent::Disconnected {
                    peer: NO_PEER,
                    reason: Some(reason),
                };
                if events.blocking_send(disconnected).is_err() {
                    return;
                }
            }
            Err(error) => {
                // Report a failing port once, not on every retry.
                let error = format!("cannot open {port}: {error}");
                if last_error.as_ref() != Some(&error) {
                    let reason = Some(error.clone());
                    let disconnected = LbEvent::Disconnected {
                        peer: NO_PEER,
                        reason,
                    };
                    if events.blocking_send(disconnected).is_err() {
                        return;
                    }
                    last_error = Some(error);
                }
            }
        }
        thread::sleep(RECONNECT_DELAY);
        if outgoing.is_closed() {
            return;
        }
    }
}

fn serve(
    link: &mut dyn Link,
    outgoing: &mut mpsc::UnboundedReceiver<LocoNetMessage>,
    events: &mpsc::Sender<LbEvent>,
) -> String {
    let mut decoder = StreamDecoder::default();
    let mut buffer = [0_u8; 256];
    // Messages sent whose echo may still come, oldest first.
    let mut echoes: VecDeque<(LocoNetMessage, Instant)> = VecDeque::new();
    loop {
        while let Ok(message) = outgoing.try_recv() {
            match link
                .write_all(message.as_bytes())
                .and_then(|()| link.flush())
            {
                Ok(()) => echoes.push_back((message, Instant::now())),
                // With RTS/CTS the interface holds CTS while it is busy; the message is lost.
                Err(error) if error.kind() == io::ErrorKind::TimedOut => {
                    let rejected = LbEvent::Rejected {
                        peer: NO_PEER,
                        input: message.to_hex(),
                        reason: "interface busy (CTS)".to_owned(),
                    };
                    if events.blocking_send(rejected).is_err() {
                        return "node stopped".to_owned();
                    }
                }
                Err(error) => return error.to_string(),
            }
        }
        match link.read(&mut buffer) {
            Ok(0) => return "port closed".to_owned(),
            Ok(count) => {
                echoes.retain(|(_, sent)| sent.elapsed() < ECHO_WINDOW);
                for decoded in decoder.push(&buffer[..count]) {
                    let event = match decoded {
                        Ok(message) => {
                            if echoes.front().is_some_and(|(sent, _)| *sent == message) {
                                echoes.pop_front();
                                continue;
                            }
                            LbEvent::Received {
                                peer: NO_PEER,
                                message,
                            }
                        }
                        Err(error) => LbEvent::Rejected {
                            peer: NO_PEER,
                            input: String::new(),
                            reason: error.to_string(),
                        },
                    };
                    if events.blocking_send(event).is_err() {
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
        if outgoing.is_closed() {
            return "node stopped".to_owned();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::serve;
    use crate::{lbserver::LbEvent, loconet::LocoNetMessage, power::PowerRequest};
    use std::io::{self, Read, Write};
    use tokio::sync::mpsc;

    /// A serial interface that hands out `input` once and records what is written.
    struct FakePort {
        input: Vec<u8>,
        written: Vec<u8>,
    }

    impl Read for FakePort {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.input.is_empty() {
                return Ok(0); // ends serve
            }
            let count = self.input.len().min(buffer.len());
            buffer[..count].copy_from_slice(&self.input[..count]);
            self.input.drain(..count);
            Ok(count)
        }
    }

    impl Write for FakePort {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            self.written.extend_from_slice(data);
            Ok(data.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn received(events: &mut mpsc::Receiver<LbEvent>) -> Vec<String> {
        let mut messages = Vec::new();
        while let Ok(event) = events.try_recv() {
            if let LbEvent::Received { message, .. } = event {
                messages.push(message.to_hex());
            }
        }
        messages
    }

    /// An echoing interface (LocoBuffer): our own GPON comes back and is dropped; a
    /// throttle's message split across two reads still arrives.
    #[test]
    fn drops_an_echo_and_passes_other_bus_traffic() {
        let (outgoing, mut outgoing_receiver) = mpsc::unbounded_channel();
        let (events, mut event_receiver) = mpsc::channel(16);
        outgoing
            .send(LocoNetMessage::power_request(PowerRequest::On))
            .unwrap();
        // GPON echo, then LOCO_SPD slot 5 speed 0x20 and GPOFF.
        let mut port = FakePort {
            input: vec![0x83, 0x7C, 0xA0, 0x05, 0x20, 0x7A, 0x82, 0x7D],
            written: Vec::new(),
        };
        serve(&mut port, &mut outgoing_receiver, &events);
        assert_eq!(port.written, [0x83, 0x7C]);
        assert_eq!(received(&mut event_receiver), ["A0 05 20 7A", "82 7D"]);
    }

    /// An interface without an echo (the Intellibox I): nothing is dropped.
    #[test]
    fn passes_everything_without_an_echo() {
        let (outgoing, mut outgoing_receiver) = mpsc::unbounded_channel();
        let (events, mut event_receiver) = mpsc::channel(16);
        outgoing
            .send(LocoNetMessage::power_request(PowerRequest::On))
            .unwrap();
        let mut port = FakePort {
            input: vec![0x82, 0x7D],
            written: Vec::new(),
        };
        serve(&mut port, &mut outgoing_receiver, &events);
        assert_eq!(received(&mut event_receiver), ["82 7D"]);
    }
}
