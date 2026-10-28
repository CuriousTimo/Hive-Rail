use crate::{
    rocnet::RocNetFrame,
    ui::{Dashboard, PeerRow, TerminalUi},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    error::Error,
    io::{self, IsTerminal},
    time::Duration,
};
use tokio::time::{self, Instant, MissedTickBehavior};
use zenoh::config::Config;

const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(250);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_millis(750);
const ZENOH_SCOUTING_ADDRESS: &str = "224.0.0.224:7446";
const MAX_CLI_LINES: usize = 200;

#[derive(Debug)]
struct NodeArgs {
    session: String,
    id: String,
    listen: Option<String>,
    connect: Option<String>,
    rocnet_address: Option<u16>,
    identify: bool,
    send_rocnet_hex: Option<String>,
    plain: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct Heartbeat {
    node: String,
    seq: u64,
    power: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct RocNetMeshEnvelope {
    origin_node: String,
    frame_ascii_hex: String,
}

#[derive(Debug)]
struct PeerState {
    last_seen: Instant,
    online: bool,
    sequence: u64,
    power: String,
}

pub async fn run(args: Vec<String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let options = parse_args(args)?;
    let interactive = !options.plain && io::stdin().is_terminal() && io::stdout().is_terminal();
    let mut tui = if interactive {
        Some(TerminalUi::new()?)
    } else {
        None
    };
    let mut config = Config::default();
    config.insert_json5("mode", "\"peer\"")?;
    config.insert_json5("scouting/multicast/enabled", "true")?;
    config.insert_json5(
        "scouting/multicast/address",
        &format!("\"{ZENOH_SCOUTING_ADDRESS}\""),
    )?;

    if let Some(endpoint) = &options.listen {
        config.insert_json5("listen/endpoints", &format!("[\"{endpoint}\"]"))?;
    }
    if let Some(endpoint) = &options.connect {
        config.insert_json5("connect/endpoints", &format!("[\"{endpoint}\"]"))?;
    }

    let session = zenoh::open(config).await?;
    let key = format!("hive/{}/node/{}/hb", options.session, options.id);
    let subscriber_key = format!("hive/{}/node/*/hb", options.session);
    let publisher = session.declare_publisher(key).await?;
    let subscriber = session.declare_subscriber(subscriber_key).await?;
    let rocnet_key = format!("hive/{}/rocnet/frame", options.session);
    let rocnet_publisher = session.declare_publisher(rocnet_key.clone()).await?;
    let rocnet_subscriber = session.declare_subscriber(rocnet_key).await?;
    let mut peers: HashMap<String, PeerState> = HashMap::new();
    let mut cli_output = VecDeque::with_capacity(MAX_CLI_LINES);
    let mut sequence = 0_u64;
    let mut last_event = "Waiting for Zenoh peers".to_owned();
    let mut pending_frame = if options.identify {
        let address = options
            .rocnet_address
            .ok_or("--identify requires --rocnet-address")?;
        Some(RocNetFrame::stationary_identify_request(0, address))
    } else if let Some(frame) = options.send_rocnet_hex.as_deref() {
        Some(RocNetFrame::from_ascii_hex(frame)?)
    } else {
        None
    };
    let mut heartbeat = time::interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut display_tick = time::interval(Duration::from_millis(100));
    display_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    record_cli_line(
        &mut cli_output,
        format!(
            "node {} joined session {}; heartbeat key: hive/{}/node/{}/hb",
            options.id, options.session, options.session, options.id
        ),
        tui.is_none(),
    );
    record_cli_line(
        &mut cli_output,
        format!("Zenoh multicast scouting: {ZENOH_SCOUTING_ADDRESS}"),
        tui.is_none(),
    );
    record_cli_line(
        &mut cli_output,
        "simulation only; no local RocNet transport or hardware is connected".to_owned(),
        tui.is_none(),
    );

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                if tui.is_none() {
                    println!("shutting down node {}", options.id);
                }
                break;
            }
            _ = heartbeat.tick() => {
                sequence += 1;
                let message = Heartbeat {
                    node: options.id.clone(),
                    seq: sequence,
                    power: "Normal".to_owned(),
                };
                publisher.put(serde_json::to_vec(&message)?).await?;
                for peer_id in refresh_peer_states(&mut peers) {
                    last_event = format!("Peer {peer_id} stale after 3 missed heartbeats; boundary state remains Stop");
                    record_cli_line(&mut cli_output, last_event.clone(), tui.is_none());
                }
                if let Some(frame) = pending_frame.take_if(|_| peers.values().any(|peer| peer.online)) {
                    let envelope = RocNetMeshEnvelope {
                        origin_node: options.id.clone(),
                        frame_ascii_hex: frame.to_ascii_hex(),
                    };
                    rocnet_publisher.put(serde_json::to_vec(&envelope)?).await?;
                    last_event = format!(
                        "published RocNet frame over Zenoh: {}",
                        envelope.frame_ascii_hex
                    );
                    record_cli_line(&mut cli_output, last_event.clone(), tui.is_none());
                }
            }
            sample = subscriber.recv_async() => {
                let sample = sample?;
                let payload = sample.payload().to_bytes();
                let message: Heartbeat = serde_json::from_slice(payload.as_ref())?;
                if message.node != options.id {
                    let was_online = peers
                        .get(&message.node)
                        .map(|peer| peer.online)
                        .unwrap_or(false);
                    peers.insert(message.node.clone(), PeerState {
                        last_seen: Instant::now(),
                        online: true,
                        sequence: message.seq,
                        power: message.power.clone(),
                    });
                    if !was_online {
                        last_event = format!("Peer {} online", message.node);
                        record_cli_line(
                            &mut cli_output,
                            format!("peer {} online (seq {}, power {})", message.node, message.seq, message.power),
                            tui.is_none(),
                        );
                    }
                }
            }
            sample = rocnet_subscriber.recv_async() => {
                let sample = sample?;
                let payload = sample.payload().to_bytes();
                let envelope: RocNetMeshEnvelope = serde_json::from_slice(payload.as_ref())?;
                if envelope.origin_node != options.id {
                    let frame = RocNetFrame::from_ascii_hex(&envelope.frame_ascii_hex)?;
                    last_event = format_rocnet_received_line(&envelope.origin_node, &frame);
                    record_cli_line(&mut cli_output, last_event.clone(), tui.is_none());
                }
            }
            _ = display_tick.tick(), if tui.is_some() => {
                if let Some(ui) = tui.as_mut() {
                    if ui.handle_input()? {
                        break;
                    }
                    let now = Instant::now();
                    let peers = peers.iter().map(|(id, peer)| PeerRow {
                        id: id.clone(),
                        online: peer.online,
                        last_seen: now.duration_since(peer.last_seen),
                        sequence: peer.sequence,
                        power: peer.power.clone(),
                    }).collect();
                    ui.draw(&Dashboard {
                        session: options.session.clone(),
                        node_id: options.id.clone(),
                        scouting: ZENOH_SCOUTING_ADDRESS.to_owned(),
                        heartbeat_interval: HEARTBEAT_INTERVAL,
                        last_event: last_event.clone(),
                        peers,
                        cli_output: cli_output.iter().cloned().collect(),
                    })?;
                }
            }
        }
    }

    Ok(())
}

fn record_cli_line(lines: &mut VecDeque<String>, line: String, echo_to_stdout: bool) {
    if lines.len() == MAX_CLI_LINES {
        lines.pop_front();
    }
    if echo_to_stdout {
        println!("{line}");
    }
    lines.push_back(line);
}

fn format_rocnet_received_line(origin_node: &str, frame: &RocNetFrame) -> String {
    format!(
        "published RocNet frame over Zenoh from {origin_node}: net={}, recipient={}, sender={}, group={}, code={}, data={:02X?}",
        frame.network_id(),
        frame.recipient_id(),
        frame.sender_id(),
        frame.group(),
        frame.code(),
        frame.data()
    )
}

fn refresh_peer_states(peers: &mut HashMap<String, PeerState>) -> Vec<String> {
    let now = Instant::now();
    let mut stale_peers = Vec::new();
    for (peer_id, peer) in peers.iter_mut() {
        if now.duration_since(peer.last_seen) >= HEARTBEAT_TIMEOUT && peer.online {
            peer.online = false;
            stale_peers.push(peer_id.clone());
        }
    }
    stale_peers
}

fn parse_args(args: Vec<String>) -> Result<NodeArgs, Box<dyn Error + Send + Sync>> {
    let mut session = None;
    let mut id = None;
    let mut listen = None;
    let mut connect = None;
    let mut rocnet_address = None;
    let mut identify = false;
    let mut send_rocnet_hex = None;
    let mut plain = false;
    let mut args = args.into_iter();

    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--identify" => identify = true,
            "--plain" => plain = true,
            "--session" => session = Some(next_value(&mut args, &flag)?),
            "--id" => id = Some(next_value(&mut args, &flag)?),
            "--listen" => listen = Some(next_value(&mut args, &flag)?),
            "--connect" => connect = Some(next_value(&mut args, &flag)?),
            "--send-rocnet-hex" => send_rocnet_hex = Some(next_value(&mut args, &flag)?),
            "--rocnet-address" => {
                let value = next_value(&mut args, &flag)?;
                rocnet_address = Some(value.parse::<u16>().map_err(|_| {
                    format!("invalid RocNet address '{value}'; expected an integer from 0 to 65535")
                })?);
            }
            _ => return Err(format!("unknown node option: {flag}").into()),
        }
    }

    if listen.is_some() && connect.is_some() {
        return Err("choose either --listen or --connect for this prototype".into());
    }
    if identify && send_rocnet_hex.is_some() {
        return Err("choose either --identify or --send-rocnet-hex".into());
    }
    if identify && rocnet_address.is_none() {
        return Err("--identify requires --rocnet-address".into());
    }

    Ok(NodeArgs {
        session: session.ok_or("missing --session")?,
        id: id.ok_or("missing --id")?,
        listen,
        connect,
        rocnet_address,
        identify,
        send_rocnet_hex,
        plain,
    })
}

fn next_value(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    args.next()
        .ok_or_else(|| format!("missing value after {flag}").into())
}

#[cfg(test)]
mod tests {
    use super::{
        format_rocnet_received_line, parse_args, record_cli_line, refresh_peer_states, PeerState,
        HEARTBEAT_TIMEOUT, MAX_CLI_LINES, ZENOH_SCOUTING_ADDRESS,
    };
    use crate::rocnet::RocNetFrame;
    use std::{
        collections::{HashMap, VecDeque},
        time::Duration,
    };
    use tokio::time::Instant;

    const ROCNET_UDP_MULTICAST: &str = "224.0.0.1:4321";

    #[test]
    fn default_peer_mode_does_not_require_manual_endpoints() {
        let args = parse_args(vec![
            "--session".into(),
            "bench".into(),
            "--id".into(),
            "node-a".into(),
        ])
        .unwrap();

        assert!(args.listen.is_none());
        assert!(args.connect.is_none());
        assert!(!args.plain);
    }

    #[test]
    fn plain_mode_can_be_selected_for_non_interactive_runs() {
        let args = parse_args(vec![
            "--session".into(),
            "bench".into(),
            "--id".into(),
            "node-a".into(),
            "--plain".into(),
        ])
        .unwrap();
        assert!(args.plain);
    }

    #[test]
    fn zenoh_scouting_is_separate_from_rocnet_multicast() {
        assert_ne!(ZENOH_SCOUTING_ADDRESS, ROCNET_UDP_MULTICAST);
    }

    #[test]
    fn rocnet_identify_requires_a_local_protocol_address() {
        let missing_address = parse_args(vec![
            "--session".into(),
            "bench".into(),
            "--id".into(),
            "node-a".into(),
            "--identify".into(),
        ]);
        assert!(missing_address.is_err());

        let with_address = parse_args(vec![
            "--session".into(),
            "bench".into(),
            "--id".into(),
            "node-a".into(),
            "--rocnet-address".into(),
            "42".into(),
            "--identify".into(),
        ])
        .unwrap();
        assert!(with_address.identify);
        assert_eq!(with_address.rocnet_address, Some(42));
    }

    #[test]
    fn stale_peers_are_reported_once_and_remain_stale() {
        let mut peers = HashMap::from([(
            "node-b".to_owned(),
            PeerState {
                last_seen: Instant::now() - HEARTBEAT_TIMEOUT - Duration::from_millis(1),
                online: true,
                sequence: 10,
                power: "Normal".to_owned(),
            },
        )]);

        assert_eq!(refresh_peer_states(&mut peers), vec!["node-b"]);
        assert!(refresh_peer_states(&mut peers).is_empty());
        assert!(!peers["node-b"].online);
    }

    #[test]
    fn raw_cli_output_buffer_is_bounded_and_keeps_recent_lines() {
        let mut lines = VecDeque::new();
        for index in 0..=MAX_CLI_LINES {
            record_cli_line(&mut lines, format!("line {index}"), false);
        }

        assert_eq!(lines.len(), MAX_CLI_LINES);
        assert_eq!(lines.front().map(String::as_str), Some("line 1"));
        assert_eq!(lines.back().map(String::as_str), Some("line 200"));
    }

    #[test]
    fn received_rocnet_line_keeps_the_cli_wording() {
        let frame = RocNetFrame::from_ascii_hex("@0001230001020203304140").unwrap();
        let line = format_rocnet_received_line("windows", &frame);

        assert!(line
            .starts_with("published RocNet frame over Zenoh from windows: net=0, recipient=291"));
        assert!(line.contains("group=2, code=2, data=[30, 41, 40]"));
    }
}
