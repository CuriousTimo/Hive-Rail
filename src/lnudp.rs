//! LocoNet over UDP multicast, as the GCA101 LocoBuffer-UDP (MGV101) and Rocrail use it:
//! every datagram to the group 224.0.0.1, port 1235, carries raw LocoNet messages. The
//! GCA101 passes everything between the group and its LocoNet port, so joining the group
//! is joining its bus. Gives the node the same handle and events as the LbServer client.
//!
//! There is no connection to lose: the node is "connected" once it has joined the group,
//! and rejoins after a network error. It also renews its membership every half minute, so
//! it keeps receiving after the computer moves to another Wi-Fi access point.
//!
//! One GCA101 per network: every GCA101 uses the same group, so two would mix their LocoNet
//! buses into one. That is a limit of the network and is not supported.
//!
//! The node's own datagrams are not looped back to it. If the GCA101 sends back the bus
//! echo of a message the node sent, that echo is dropped, as on a serial LocoBuffer.

use crate::lbclient::LbClientHandle;
use crate::lbserver::LbEvent;
use crate::loconet::{LocoNetMessage, StreamDecoder};
use socket2::{Domain, Protocol, Socket, Type};
use std::{
    collections::VecDeque,
    io,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    thread,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

/// The group and port of the GCA101 and Rocrail's LocoNet UDP.
pub const DEFAULT_GROUP: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(224, 0, 0, 1), 1235);
const READ_TIMEOUT: Duration = Duration::from_millis(50);
const RECONNECT_DELAY: Duration = Duration::from_secs(2);
/// The GCA101 echoes within milliseconds if it echoes at all.
const ECHO_WINDOW: Duration = Duration::from_millis(300);
/// A Wi-Fi change can end the membership without an error; joining again restores it.
const REJOIN_INTERVAL: Duration = Duration::from_secs(30);

/// Joins `group` on `interface` (`0.0.0.0`: the system's choice) and keeps rejoining.
pub fn start(
    group: SocketAddrV4,
    interface: Ipv4Addr,
) -> (LbClientHandle, mpsc::Receiver<LbEvent>) {
    let (outgoing, outgoing_receiver) = mpsc::unbounded_channel();
    let (events, event_receiver) = mpsc::channel(256);
    thread::spawn(move || run(group, interface, outgoing_receiver, events));
    (LbClientHandle::new(outgoing), event_receiver)
}

/// Parses `--loconet-udp`: `group:port`, or a group alone for port 1235.
pub fn parse_group(value: &str) -> Option<SocketAddrV4> {
    let group = match value.parse::<SocketAddrV4>() {
        Ok(group) => group,
        Err(_) => SocketAddrV4::new(value.parse().ok()?, DEFAULT_GROUP.port()),
    };
    group.ip().is_multicast().then_some(group)
}

/// What `serve` needs from a socket, so the tests can use a fake one.
trait Datagrams {
    fn send(&mut self, data: &[u8]) -> io::Result<()>;
    fn receive(&mut self, buffer: &mut [u8]) -> io::Result<(usize, SocketAddr)>;
    fn rejoin(&mut self) -> io::Result<()>;
}

struct Group {
    socket: UdpSocket,
    group: SocketAddrV4,
    interface: Ipv4Addr,
}

impl Datagrams for Group {
    fn send(&mut self, data: &[u8]) -> io::Result<()> {
        self.socket.send_to(data, self.group).map(|_| ())
    }

    fn receive(&mut self, buffer: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.socket.recv_from(buffer)
    }

    fn rejoin(&mut self) -> io::Result<()> {
        // Leaving fails when the membership is already gone; joining is what counts.
        let _ = self
            .socket
            .leave_multicast_v4(self.group.ip(), &self.interface);
        self.socket
            .join_multicast_v4(self.group.ip(), &self.interface)
    }
}

fn join(group: SocketAddrV4, interface: Ipv4Addr) -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    // Rocrail or a monitor on the same computer may have the port already.
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, group.port()).into())?;
    socket.join_multicast_v4(group.ip(), &interface)?;
    if !interface.is_unspecified() {
        socket.set_multicast_if_v4(&interface)?;
    }
    socket.set_multicast_loop_v4(false)?;
    socket.set_read_timeout(Some(READ_TIMEOUT))?;
    Ok(socket.into())
}

fn run(
    group: SocketAddrV4,
    interface: Ipv4Addr,
    mut outgoing: mpsc::UnboundedReceiver<LocoNetMessage>,
    events: mpsc::Sender<LbEvent>,
) {
    let peer = SocketAddr::V4(group);
    let mut last_error = None;
    loop {
        match join(group, interface) {
            Ok(socket) => {
                last_error = None;
                if events.blocking_send(LbEvent::Connected(peer)).is_err() {
                    return;
                }
                // Messages queued while the network was down are stale.
                while outgoing.try_recv().is_ok() {}
                let mut link = Group {
                    socket,
                    group,
                    interface,
                };
                let reason = serve(&mut link, &mut outgoing, &events);
                let disconnected = LbEvent::Disconnected {
                    peer,
                    reason: Some(reason),
                };
                if events.blocking_send(disconnected).is_err() {
                    return;
                }
            }
            Err(error) => {
                // Report a failing join once, not on every retry.
                let error = format!("cannot join {group}: {error}");
                if last_error.as_ref() != Some(&error) {
                    let disconnected = LbEvent::Disconnected {
                        peer,
                        reason: Some(error.clone()),
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
    link: &mut dyn Datagrams,
    outgoing: &mut mpsc::UnboundedReceiver<LocoNetMessage>,
    events: &mpsc::Sender<LbEvent>,
) -> String {
    let mut buffer = [0_u8; 512];
    // Messages sent whose echo may still come, oldest first.
    let mut echoes: VecDeque<(LocoNetMessage, Instant)> = VecDeque::new();
    let mut joined = Instant::now();
    loop {
        if joined.elapsed() >= REJOIN_INTERVAL {
            // Without a network (Wi-Fi switching) the join fails; the next one retries.
            let _ = link.rejoin();
            joined = Instant::now();
        }
        while let Ok(message) = outgoing.try_recv() {
            if let Err(error) = link.send(message.as_bytes()) {
                return error.to_string();
            }
            echoes.push_back((message, Instant::now()));
        }
        match link.receive(&mut buffer) {
            Ok((count, peer)) => {
                echoes.retain(|(_, sent)| sent.elapsed() < ECHO_WINDOW);
                let datagram = &buffer[..count];
                // A LocoNet message starts with an opcode (bit 7 set); anything else is the
                // GCA101 talking about itself.
                if datagram.first().is_some_and(|byte| byte & 0x80 == 0) {
                    let rejected = LbEvent::Rejected {
                        peer,
                        input: hex(datagram),
                        reason: describe_announcement(datagram),
                    };
                    if events.blocking_send(rejected).is_err() {
                        return "node stopped".to_owned();
                    }
                    continue;
                }
                // Each datagram holds whole messages; a broken one does not spoil the next.
                let mut decoder = StreamDecoder::default();
                for decoded in decoder.push(datagram) {
                    let event = match decoded {
                        Ok(message) => {
                            if echoes.front().is_some_and(|(sent, _)| *sent == message) {
                                echoes.pop_front();
                                continue;
                            }
                            LbEvent::Received { peer, message }
                        }
                        // The whole datagram, to tell a bus collision from a split message.
                        Err(error) => LbEvent::Rejected {
                            peer,
                            input: hex(datagram),
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

/// The GCA101 announces itself when its LocoNet changes (seen when a device was plugged in):
/// `71 FF`, its address, its netmask, a zero, its name and two more bytes, such as
/// `71 FF C0 A8 00 C8 FF FF FF 00 00 4D 56 31 30 31 21 06` for 192.168.0.200, "MV101".
fn describe_announcement(datagram: &[u8]) -> String {
    match datagram {
        [0x71, 0xFF, a, b, c, d, _, _, _, _, 0, rest @ ..] if rest.len() >= 2 => {
            let name = String::from_utf8_lossy(&rest[..rest.len() - 2]);
            format!("not LocoNet: GCA101 announcement, {a}.{b}.{c}.{d} \"{name}\"")
        }
        _ => "not LocoNet: no opcode at the start".to_owned(),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{parse_group, serve, Datagrams};
    use crate::{lbserver::LbEvent, loconet::LocoNetMessage, power::PowerRequest};
    use std::{
        collections::VecDeque,
        io,
        net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    };
    use tokio::sync::mpsc;

    const GCA101: SocketAddr =
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 50), 1235));

    /// A group that hands out `input` one datagram per receive and records what is sent.
    struct FakeGroup {
        input: VecDeque<Vec<u8>>,
        sent: Vec<Vec<u8>>,
    }

    impl Datagrams for FakeGroup {
        fn send(&mut self, data: &[u8]) -> io::Result<()> {
            self.sent.push(data.to_vec());
            Ok(())
        }

        fn receive(&mut self, buffer: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
            // An empty queue ends serve.
            let datagram = self.input.pop_front().ok_or(io::ErrorKind::NotConnected)?;
            buffer[..datagram.len()].copy_from_slice(&datagram);
            Ok((datagram.len(), GCA101))
        }

        fn rejoin(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn received(events: &mut mpsc::Receiver<LbEvent>) -> Vec<String> {
        let mut messages = Vec::new();
        while let Ok(event) = events.try_recv() {
            match event {
                LbEvent::Received { message, peer } => {
                    assert_eq!(peer, GCA101);
                    messages.push(message.to_hex());
                }
                LbEvent::Rejected { .. } => messages.push("rejected".to_owned()),
                _ => {}
            }
        }
        messages
    }

    /// The node's GPON goes out as one datagram; the GCA101's echo of it is dropped, and
    /// an IB-Switch's switch request arrives.
    #[test]
    fn sends_a_datagram_and_drops_the_echo() {
        let (outgoing, mut outgoing_receiver) = mpsc::unbounded_channel();
        let (events, mut event_receiver) = mpsc::channel(16);
        outgoing
            .send(LocoNetMessage::power_request(PowerRequest::On))
            .unwrap();
        let mut group = FakeGroup {
            input: VecDeque::from([vec![0x83, 0x7C], vec![0xB0, 0x00, 0x30, 0x7F]]),
            sent: Vec::new(),
        };
        serve(&mut group, &mut outgoing_receiver, &events);
        assert_eq!(group.sent, [vec![0x83, 0x7C]]);
        assert_eq!(received(&mut event_receiver), ["B0 00 30 7F"]);
    }

    /// Two messages in one datagram both arrive; a broken datagram is rejected without
    /// spoiling the next one.
    #[test]
    fn decodes_each_datagram_on_its_own() {
        let (_outgoing, mut outgoing_receiver) = mpsc::unbounded_channel();
        let (events, mut event_receiver) = mpsc::channel(16);
        let mut group = FakeGroup {
            input: VecDeque::from([
                vec![0x83, 0x7C, 0x82, 0x7D],
                vec![0x83, 0x00],
                vec![0x82, 0x7D],
            ]),
            sent: Vec::new(),
        };
        serve(&mut group, &mut outgoing_receiver, &events);
        assert_eq!(
            received(&mut event_receiver),
            ["83 7C", "82 7D", "rejected", "82 7D"]
        );
    }

    /// The GCA101's announcement is reported once, not as seven broken messages.
    #[test]
    fn reports_the_gca101_announcement_once() {
        let (_outgoing, mut outgoing_receiver) = mpsc::unbounded_channel();
        let (events, mut event_receiver) = mpsc::channel(16);
        let announcement = vec![
            0x71, 0xFF, 0xC0, 0xA8, 0x00, 0xC8, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x4D, 0x56, 0x31,
            0x30, 0x31, 0x21, 0x06,
        ];
        let mut group = FakeGroup {
            input: VecDeque::from([announcement, vec![0x82, 0x7D]]),
            sent: Vec::new(),
        };
        serve(&mut group, &mut outgoing_receiver, &events);
        let mut reasons = Vec::new();
        while let Ok(event) = event_receiver.try_recv() {
            match event {
                LbEvent::Rejected { reason, .. } => reasons.push(reason),
                LbEvent::Received { message, .. } => reasons.push(message.to_hex()),
                _ => {}
            }
        }
        assert_eq!(
            reasons,
            [
                "not LocoNet: GCA101 announcement, 192.168.0.200 \"MV101\"",
                "82 7D"
            ]
        );
    }

    #[test]
    fn parses_a_group_with_or_without_a_port() {
        assert_eq!(parse_group("224.0.0.1"), Some(super::DEFAULT_GROUP));
        assert_eq!(
            parse_group("239.1.2.3:4000"),
            Some(SocketAddrV4::new(Ipv4Addr::new(239, 1, 2, 3), 4000))
        );
        assert_eq!(parse_group("192.168.1.50:1235"), None);
        assert_eq!(parse_group("nonsense"), None);
    }
}
