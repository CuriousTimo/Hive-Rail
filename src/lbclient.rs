//! LocoNet over TCP client: connects to an LbServer that sits on a real LocoNet bus, such
//! as the node board's LocoNet buffer, and reconnects on its own.
//!
//! LbServer text format: the client sends `SEND <hex>`; the server puts it on the bus and
//! answers `RECEIVE <hex>` (the bus echo) and `SENT OK` or `SENT ERROR`. Every other bus
//! message arrives as `RECEIVE <hex>`. The echo of this client's own messages is dropped
//! here, so the node only sees what other bus devices sent.

use crate::lbserver::LbEvent;
use crate::loconet::LocoNetMessage;
use std::{
    collections::VecDeque,
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{lookup_host, TcpStream},
    sync::mpsc,
    time::{self, Instant},
};

const RECONNECT_DELAY: Duration = Duration::from_secs(2);
/// An echo that has not come back by then belongs to a message the bus lost.
const ECHO_WINDOW: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct LbClientHandle {
    outgoing: mpsc::UnboundedSender<LocoNetMessage>,
}

impl LbClientHandle {
    /// Puts a message on the bus; dropped while the client is not connected.
    pub fn send(&self, message: &LocoNetMessage) {
        let _ = self.outgoing.send(message.clone());
    }
}

/// Connects to `target` (`host:port`) and keeps reconnecting.
pub fn start(target: String) -> (LbClientHandle, mpsc::Receiver<LbEvent>) {
    let (outgoing, outgoing_receiver) = mpsc::unbounded_channel();
    let (events, event_receiver) = mpsc::channel(256);
    tokio::spawn(run(target, outgoing_receiver, events));
    (LbClientHandle { outgoing }, event_receiver)
}

async fn run(
    target: String,
    mut outgoing: mpsc::UnboundedReceiver<LocoNetMessage>,
    events: mpsc::Sender<LbEvent>,
) {
    let unresolved = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0));
    let mut last_error = None;
    loop {
        let peer = match lookup_host(&target).await.map(|mut addrs| addrs.next()) {
            Ok(Some(peer)) => peer,
            _ => unresolved,
        };
        match TcpStream::connect(peer).await {
            Ok(stream) => {
                last_error = None;
                let _ = stream.set_nodelay(true);
                if events.send(LbEvent::Connected(peer)).await.is_err() {
                    return;
                }
                // Messages queued while disconnected are stale.
                while outgoing.try_recv().is_ok() {}
                let reason = serve(stream, peer, &mut outgoing, &events).await;
                if events
                    .send(LbEvent::Disconnected {
                        peer,
                        reason: Some(reason),
                    })
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Err(error) => {
                // Report a failing connection once, not on every retry.
                let error = format!("cannot connect to {target}: {error}");
                if last_error.as_ref() != Some(&error) {
                    let reason = Some(error.clone());
                    if events
                        .send(LbEvent::Disconnected { peer, reason })
                        .await
                        .is_err()
                    {
                        return;
                    }
                    last_error = Some(error);
                }
            }
        }
        time::sleep(RECONNECT_DELAY).await;
        if outgoing.is_closed() {
            return;
        }
    }
}

async fn serve(
    stream: TcpStream,
    peer: SocketAddr,
    outgoing: &mut mpsc::UnboundedReceiver<LocoNetMessage>,
    events: &mpsc::Sender<LbEvent>,
) -> String {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    // Messages sent and not yet echoed, oldest first.
    let mut echoes: VecDeque<(LocoNetMessage, Instant)> = VecDeque::new();
    loop {
        tokio::select! {
            message = outgoing.recv() => {
                let Some(message) = message else { return "node stopped".to_owned() };
                let line = format!("SEND {}\r\n", message.to_hex());
                if let Err(error) = writer.write_all(line.as_bytes()).await {
                    return error.to_string();
                }
                echoes.push_back((message, Instant::now()));
            }
            line = lines.next_line() => {
                let line = match line {
                    Ok(Some(line)) => line,
                    Ok(None) => return "connection closed".to_owned(),
                    Err(error) => return error.to_string(),
                };
                echoes.retain(|(_, sent)| sent.elapsed() < ECHO_WINDOW);
                let line = line.trim();
                if let Some(hex) = line.strip_prefix("RECEIVE ") {
                    match LocoNetMessage::from_hex(hex) {
                        Ok(message) => {
                            if echoes.front().is_some_and(|(sent, _)| *sent == message) {
                                echoes.pop_front();
                                continue;
                            }
                            if events.send(LbEvent::Received { peer, message }).await.is_err() {
                                return "node stopped".to_owned();
                            }
                        }
                        Err(error) => {
                            let rejected = LbEvent::Rejected { peer, input: line.to_owned(), reason: error.to_string() };
                            if events.send(rejected).await.is_err() {
                                return "node stopped".to_owned();
                            }
                        }
                    }
                } else if line.starts_with("SENT ERROR") {
                    // The bus could not take the oldest message; its echo will not come.
                    let input = echoes.pop_front().map_or_else(String::new, |(message, _)| message.to_hex());
                    let rejected = LbEvent::Rejected { peer, input, reason: line.to_owned() };
                    if events.send(rejected).await.is_err() {
                        return "node stopped".to_owned();
                    }
                }
                // SENT OK and VERSION need nothing.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::start;
    use crate::{lbserver::LbEvent, loconet::LocoNetMessage, power::PowerRequest};
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::TcpListener,
    };

    /// A fake LbServer, as on the node board: echoes each SEND as RECEIVE and SENT OK.
    #[tokio::test]
    async fn drops_its_own_echo_and_passes_other_bus_traffic() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (client, mut events) = start(address.to_string());

        let (stream, _) = listener.accept().await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        assert!(matches!(events.recv().await, Some(LbEvent::Connected(_))));
        writer
            .write_all(b"VERSION Hive-Rail board\r\n")
            .await
            .unwrap();

        client.send(&LocoNetMessage::power_request(PowerRequest::On));
        assert_eq!(lines.next_line().await.unwrap().unwrap(), "SEND 83 7C");
        // The echo of our own GPON, then a FRED's GPOFF from the bus.
        writer
            .write_all(b"RECEIVE 83 7C\r\nSENT OK\r\nRECEIVE 82 7D\r\n")
            .await
            .unwrap();
        match events.recv().await {
            Some(LbEvent::Received { message, .. }) => assert_eq!(message.to_hex(), "82 7D"),
            _ => panic!("expected only the FRED's message"),
        }

        // A message the bus could not take is reported.
        client.send(&LocoNetMessage::power_request(PowerRequest::Off));
        assert_eq!(lines.next_line().await.unwrap().unwrap(), "SEND 82 7D");
        writer.write_all(b"SENT ERROR\r\n").await.unwrap();
        match events.recv().await {
            Some(LbEvent::Rejected { input, .. }) => assert_eq!(input, "82 7D"),
            _ => panic!("expected the failed send"),
        }
    }
}
