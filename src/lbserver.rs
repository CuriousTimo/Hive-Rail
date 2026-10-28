//! LocoNet over TCP server in the LbServer text format, so Rocrail and JMRI can connect.
//!
//! Clients send `SEND <hex>`; every valid message is echoed to the sender as
//! `RECEIVE <hex>` followed by `SENT OK`, and sent to all other clients as `RECEIVE <hex>`.

use crate::loconet::LocoNetMessage;
use std::{io, net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{broadcast, mpsc},
};

pub enum LbEvent {
    Connected(SocketAddr),
    Disconnected(SocketAddr),
    Received {
        peer: SocketAddr,
        message: LocoNetMessage,
    },
    Rejected {
        peer: SocketAddr,
        line: String,
        reason: String,
    },
}

#[derive(Clone, Debug)]
struct Outgoing {
    skip_client: Option<u64>,
    line: String,
}

#[derive(Clone)]
pub struct LbServerHandle {
    outgoing: broadcast::Sender<Outgoing>,
}

impl LbServerHandle {
    /// Sends a message to every connected client as if it came from the LocoNet bus.
    pub fn send(&self, message: &LocoNetMessage) {
        // No connected clients is not an error.
        let _ = self.outgoing.send(Outgoing {
            skip_client: None,
            line: format!("RECEIVE {}", message.to_hex()),
        });
    }
}

pub async fn start(port: u16) -> io::Result<(LbServerHandle, mpsc::Receiver<LbEvent>)> {
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    let (outgoing, _) = broadcast::channel(256);
    let (events, event_receiver) = mpsc::channel(256);
    tokio::spawn(accept_clients(listener, outgoing.clone(), events));
    Ok((LbServerHandle { outgoing }, event_receiver))
}

async fn accept_clients(
    listener: TcpListener,
    outgoing: broadcast::Sender<Outgoing>,
    events: mpsc::Sender<LbEvent>,
) {
    let mut next_client = 0_u64;
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                next_client += 1;
                tokio::spawn(serve_client(
                    next_client,
                    stream,
                    peer,
                    outgoing.clone(),
                    events.clone(),
                ));
            }
            // Accept errors are usually transient (for example too many open files).
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

async fn serve_client(
    client: u64,
    stream: TcpStream,
    peer: SocketAddr,
    outgoing: broadcast::Sender<Outgoing>,
    events: mpsc::Sender<LbEvent>,
) {
    let _ = events.send(LbEvent::Connected(peer)).await;
    let _ = client_loop(client, stream, peer, &outgoing, &events).await;
    let _ = events.send(LbEvent::Disconnected(peer)).await;
}

async fn client_loop(
    client: u64,
    stream: TcpStream,
    peer: SocketAddr,
    outgoing: &broadcast::Sender<Outgoing>,
    events: &mpsc::Sender<LbEvent>,
) -> io::Result<()> {
    let mut subscription = outgoing.subscribe();
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    writer
        .write_all(format!("VERSION Hive-Rail {}\r\n", env!("CARGO_PKG_VERSION")).as_bytes())
        .await?;

    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else {
                    return Ok(());
                };
                let Some(hex) = line.trim().strip_prefix("SEND ") else {
                    continue;
                };
                match LocoNetMessage::from_hex(hex) {
                    Ok(message) => {
                        let receive = format!("RECEIVE {}", message.to_hex());
                        writer.write_all(format!("{receive}\r\nSENT OK\r\n").as_bytes()).await?;
                        let _ = outgoing.send(Outgoing { skip_client: Some(client), line: receive });
                        let _ = events.send(LbEvent::Received { peer, message }).await;
                    }
                    Err(error) => {
                        writer.write_all(format!("SENT ERROR {error}\r\n").as_bytes()).await?;
                        let _ = events
                            .send(LbEvent::Rejected { peer, line: line.trim().to_owned(), reason: error.to_string() })
                            .await;
                    }
                }
            }
            queued = subscription.recv() => match queued {
                Ok(queued) if queued.skip_client != Some(client) => {
                    writer.write_all(format!("{}\r\n", queued.line).as_bytes()).await?;
                }
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return Ok(()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{start, LbEvent};
    use crate::loconet::LocoNetMessage;
    use crate::power::PowerRequest;
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::TcpStream,
    };

    #[tokio::test]
    async fn echoes_sends_and_forwards_node_messages() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let (server, mut events) = start(port).await.unwrap();

        let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        assert!(lines
            .next_line()
            .await
            .unwrap()
            .unwrap()
            .starts_with("VERSION Hive-Rail"));
        assert!(matches!(events.recv().await, Some(LbEvent::Connected(_))));

        writer.write_all(b"SEND 83 7C\n").await.unwrap();
        assert_eq!(lines.next_line().await.unwrap().unwrap(), "RECEIVE 83 7C");
        assert_eq!(lines.next_line().await.unwrap().unwrap(), "SENT OK");
        match events.recv().await {
            Some(LbEvent::Received { message, .. }) => {
                assert_eq!(message.power_request_kind(), Some(PowerRequest::On));
            }
            _ => panic!("expected a received message"),
        }

        writer.write_all(b"SEND 83 7D\n").await.unwrap();
        assert!(lines
            .next_line()
            .await
            .unwrap()
            .unwrap()
            .starts_with("SENT ERROR"));
        assert!(matches!(
            events.recv().await,
            Some(LbEvent::Rejected { .. })
        ));

        server.send(&LocoNetMessage::power_request(PowerRequest::Off));
        assert_eq!(lines.next_line().await.unwrap().unwrap(), "RECEIVE 82 7D");
    }
}
