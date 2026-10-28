//! LocoNet over TCP server, so Rocrail and JMRI can connect.
//!
//! Each client's format is detected from the first byte it sends:
//! - **text** (LbServer): the client sends `SEND <hex>`; a valid message is echoed as
//!   `RECEIVE <hex>` followed by `SENT OK`, and other clients get `RECEIVE <hex>`.
//! - **binary**: the client sends raw LocoNet bytes; every valid message is echoed back
//!   unchanged, as a LocoNet interface does, and other clients get it in their format.

use crate::loconet::{LocoNetMessage, StreamDecoder};
use std::{fmt, io, net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{tcp::OwnedWriteHalf, TcpListener, TcpStream},
    sync::{broadcast, mpsc},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientFormat {
    Text,
    Binary,
}

impl fmt::Display for ClientFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Text => "text (LbServer)",
            Self::Binary => "binary",
        })
    }
}

pub enum LbEvent {
    Connected(SocketAddr),
    FormatDetected {
        peer: SocketAddr,
        format: ClientFormat,
    },
    Disconnected {
        peer: SocketAddr,
        reason: Option<String>,
    },
    Received {
        peer: SocketAddr,
        message: LocoNetMessage,
    },
    Rejected {
        peer: SocketAddr,
        input: String,
        reason: String,
    },
}

#[derive(Clone, Debug)]
struct Outgoing {
    skip_client: Option<u64>,
    message: LocoNetMessage,
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
            message: message.clone(),
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
    let reason = client_loop(client, stream, peer, &outgoing, &events)
        .await
        .err()
        .map(|error| error.to_string());
    let _ = events.send(LbEvent::Disconnected { peer, reason }).await;
}

struct Client {
    id: u64,
    peer: SocketAddr,
    format: Option<ClientFormat>,
    writer: OwnedWriteHalf,
    text: String,
    decoder: StreamDecoder,
}

impl Client {
    /// Writes a bus message in this client's format. Text is used until the format is known.
    async fn write_message(&mut self, message: &LocoNetMessage) -> io::Result<()> {
        match self.format {
            Some(ClientFormat::Binary) => self.writer.write_all(message.as_bytes()).await,
            Some(ClientFormat::Text) | None => {
                self.writer
                    .write_all(format!("RECEIVE {}\r\n", message.to_hex()).as_bytes())
                    .await
            }
        }
    }

    async fn handle_input(
        &mut self,
        data: &[u8],
        outgoing: &broadcast::Sender<Outgoing>,
        events: &mpsc::Sender<LbEvent>,
    ) -> io::Result<()> {
        let format = match self.format {
            Some(format) => format,
            None => {
                let Some(&first) = data.iter().find(|byte| !byte.is_ascii_whitespace()) else {
                    return Ok(());
                };
                let format = if first & 0x80 != 0 {
                    ClientFormat::Binary
                } else {
                    ClientFormat::Text
                };
                self.format = Some(format);
                let _ = events
                    .send(LbEvent::FormatDetected {
                        peer: self.peer,
                        format,
                    })
                    .await;
                if format == ClientFormat::Text {
                    self.writer
                        .write_all(
                            format!("VERSION Hive-Rail {}\r\n", env!("CARGO_PKG_VERSION"))
                                .as_bytes(),
                        )
                        .await?;
                }
                format
            }
        };

        match format {
            ClientFormat::Binary => {
                for result in self.decoder.push(data) {
                    match result {
                        Ok(message) => {
                            self.writer.write_all(message.as_bytes()).await?;
                            self.accept(message, outgoing, events).await;
                        }
                        Err(error) => self.reject(String::new(), error.to_string(), events).await,
                    }
                }
            }
            ClientFormat::Text => {
                self.text.push_str(&String::from_utf8_lossy(data));
                while let Some(end) = self.text.find(['\r', '\n']) {
                    let line: String = self.text.drain(..=end).collect();
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    let Some(hex) = line.strip_prefix("SEND ") else {
                        self.reject(line.to_owned(), "unknown command".to_owned(), events)
                            .await;
                        continue;
                    };
                    match LocoNetMessage::from_hex(hex) {
                        Ok(message) => {
                            self.writer
                                .write_all(
                                    format!("RECEIVE {}\r\nSENT OK\r\n", message.to_hex())
                                        .as_bytes(),
                                )
                                .await?;
                            self.accept(message, outgoing, events).await;
                        }
                        Err(error) => {
                            self.writer
                                .write_all(format!("SENT ERROR {error}\r\n").as_bytes())
                                .await?;
                            self.reject(line.to_owned(), error.to_string(), events)
                                .await;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn accept(
        &self,
        message: LocoNetMessage,
        outgoing: &broadcast::Sender<Outgoing>,
        events: &mpsc::Sender<LbEvent>,
    ) {
        let _ = outgoing.send(Outgoing {
            skip_client: Some(self.id),
            message: message.clone(),
        });
        let _ = events
            .send(LbEvent::Received {
                peer: self.peer,
                message,
            })
            .await;
    }

    async fn reject(&self, input: String, reason: String, events: &mpsc::Sender<LbEvent>) {
        let _ = events
            .send(LbEvent::Rejected {
                peer: self.peer,
                input,
                reason,
            })
            .await;
    }
}

async fn client_loop(
    id: u64,
    stream: TcpStream,
    peer: SocketAddr,
    outgoing: &broadcast::Sender<Outgoing>,
    events: &mpsc::Sender<LbEvent>,
) -> io::Result<()> {
    let mut subscription = outgoing.subscribe();
    let (mut reader, writer) = stream.into_split();
    let mut client = Client {
        id,
        peer,
        format: None,
        writer,
        text: String::new(),
        decoder: StreamDecoder::default(),
    };
    let mut buffer = [0_u8; 1024];

    loop {
        tokio::select! {
            read = reader.read(&mut buffer) => {
                let count = read?;
                if count == 0 {
                    return Ok(());
                }
                client.handle_input(&buffer[..count], outgoing, events).await?;
            }
            queued = subscription.recv() => match queued {
                Ok(queued) if queued.skip_client != Some(client.id) => {
                    client.write_message(&queued.message).await?;
                }
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return Ok(()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{start, ClientFormat, LbEvent, LbServerHandle};
    use crate::loconet::LocoNetMessage;
    use crate::power::PowerRequest;
    use tokio::{
        io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
        net::TcpStream,
        sync::mpsc,
    };

    async fn start_on_free_port() -> (u16, LbServerHandle, mpsc::Receiver<LbEvent>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let (server, events) = start(port).await.unwrap();
        (port, server, events)
    }

    #[tokio::test]
    async fn text_clients_get_echo_sent_ok_and_node_messages() {
        let (port, server, mut events) = start_on_free_port().await;
        let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        assert!(matches!(events.recv().await, Some(LbEvent::Connected(_))));

        writer.write_all(b"SEND 83 7C\r\n").await.unwrap();
        assert!(lines
            .next_line()
            .await
            .unwrap()
            .unwrap()
            .starts_with("VERSION Hive-Rail"));
        assert_eq!(lines.next_line().await.unwrap().unwrap(), "RECEIVE 83 7C");
        assert_eq!(lines.next_line().await.unwrap().unwrap(), "SENT OK");
        assert!(matches!(
            events.recv().await,
            Some(LbEvent::FormatDetected {
                format: ClientFormat::Text,
                ..
            })
        ));
        match events.recv().await {
            Some(LbEvent::Received { message, .. }) => {
                assert_eq!(message.power_request_kind(), Some(PowerRequest::On));
            }
            _ => panic!("expected a received message"),
        }

        writer.write_all(b"SEND 83 7D\nHELLO\n").await.unwrap();
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
        match events.recv().await {
            Some(LbEvent::Rejected { input, .. }) => assert_eq!(input, "HELLO"),
            _ => panic!("expected the unknown command to be reported"),
        }

        server.send(&LocoNetMessage::power_request(PowerRequest::Off));
        assert_eq!(lines.next_line().await.unwrap().unwrap(), "RECEIVE 82 7D");
    }

    #[tokio::test]
    async fn binary_clients_get_raw_echo_and_node_messages() {
        let (port, server, mut events) = start_on_free_port().await;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        assert!(matches!(events.recv().await, Some(LbEvent::Connected(_))));

        stream.write_all(&[0x83, 0x7C]).await.unwrap();
        let mut echo = [0_u8; 2];
        stream.read_exact(&mut echo).await.unwrap();
        assert_eq!(echo, [0x83, 0x7C]);
        assert!(matches!(
            events.recv().await,
            Some(LbEvent::FormatDetected {
                format: ClientFormat::Binary,
                ..
            })
        ));
        assert!(matches!(
            events.recv().await,
            Some(LbEvent::Received { .. })
        ));

        server.send(&LocoNetMessage::power_request(PowerRequest::Halt));
        let mut forwarded = [0_u8; 2];
        stream.read_exact(&mut forwarded).await.unwrap();
        assert_eq!(forwarded, [0x85, 0x7A]);
    }
}
