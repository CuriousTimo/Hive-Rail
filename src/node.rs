use crate::{
    lbserver::{self, LbEvent, LbServerHandle},
    loconet::LocoNetMessage,
    power::{PowerRequest, SimulatedCommandStation},
    rocnet::RocNetFrame,
    ui::{Dashboard, PeerRow, TerminalUi, UiCommand},
};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    error::Error,
    io::{self, IsTerminal},
    time::Duration,
};
use tokio::{
    sync::mpsc,
    time::{self, Instant, MissedTickBehavior},
};
use zenoh::{
    config::Config,
    pubsub::Publisher,
    qos::{CongestionControl, Priority},
    sample::{Locality, Sample},
};

const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(250);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_millis(750);
const ZENOH_SCOUTING_ADDRESS: &str = "224.0.0.224:7446";
const MAX_CLI_LINES: usize = 200;
const DEFAULT_ROCNET_ID: u16 = 100;

#[derive(Debug)]
struct NodeArgs {
    session: String,
    id: String,
    listen: Option<String>,
    connect: Option<String>,
    rocnet_address: Option<u16>,
    identify: bool,
    send_rocnet_hex: Option<String>,
    loconet_tcp: Option<u16>,
    plain: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct Heartbeat {
    node: String,
    seq: u64,
    power: String,
    // Missing in heartbeats from nodes built before RocNet IDs were shared.
    #[serde(default)]
    rocnet_id: Option<u16>,
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
    rocnet_id: Option<u16>,
}

/// Where a power request came from, which decides where it still has to be sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestSource {
    Dashboard,
    LocoNet,
    Mesh,
}

struct CliLog {
    lines: VecDeque<String>,
    last_event: String,
    echo: bool,
}

impl CliLog {
    fn line(&mut self, line: String) {
        record_cli_line(
            &mut self.lines,
            format!("{} {line}", Local::now().format("%H:%M:%S")),
            self.echo,
        );
    }

    fn event(&mut self, event: String) {
        self.line(format!("EVENT  {event}"));
        self.last_event = event;
    }
}

/// Everything a power change has to be reported to.
struct PowerOutputs<'a> {
    node_id: &'a str,
    rocnet_id: u16,
    requests: &'a Publisher<'static>,
    status: &'a Publisher<'static>,
    loconet: Option<&'a LbServerHandle>,
}

struct PowerControl {
    station: SimulatedCommandStation,
    layout: String,
}

pub async fn run(args: Vec<String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let options = parse_args(args)?;
    let rocnet_id = options.rocnet_address.unwrap_or(DEFAULT_ROCNET_ID);
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
    // Own power frames are applied directly, so they are only routed to other nodes.
    let power_request_publisher = session
        .declare_publisher(format!("hive/{}/rn/cs/all", options.session))
        .priority(Priority::RealTime)
        .congestion_control(CongestionControl::Block)
        .allowed_destination(Locality::Remote)
        .await?;
    let power_status_publisher = session
        .declare_publisher(format!(
            "hive/{}/rn/cs/node/{}",
            options.session, options.id
        ))
        .priority(Priority::RealTime)
        .congestion_control(CongestionControl::Block)
        .allowed_destination(Locality::Remote)
        .await?;
    let command_station_subscriber = session
        .declare_subscriber(format!("hive/{}/rn/cs/**", options.session))
        .await?;

    let (loconet_server, mut loconet_events) = match options.loconet_tcp {
        Some(port) => {
            let (server, events) = lbserver::start(port).await?;
            (Some(server), Some(events))
        }
        None => (None, None),
    };
    let mut loconet_clients = 0_usize;
    let outputs = PowerOutputs {
        node_id: &options.id,
        rocnet_id,
        requests: &power_request_publisher,
        status: &power_status_publisher,
        loconet: loconet_server.as_ref(),
    };
    let mut power = PowerControl {
        station: SimulatedCommandStation::new(),
        layout: "no request yet".to_owned(),
    };

    let mut peers: HashMap<String, PeerState> = HashMap::new();
    let mut log = CliLog {
        lines: VecDeque::with_capacity(MAX_CLI_LINES),
        last_event: "Waiting for Zenoh peers".to_owned(),
        echo: tui.is_none(),
    };
    let mut sequence = 0_u64;
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

    log.line(format!(
        "node {} (RocNet {rocnet_id}) joined session {}; heartbeat key: hive/{}/node/{}/hb",
        options.id, options.session, options.session, options.id
    ));
    log.line(format!(
        "Zenoh multicast scouting: {ZENOH_SCOUTING_ADDRESS}"
    ));
    match options.loconet_tcp {
        Some(port) => log.line(format!(
            "LocoNet over TCP (LbServer) listening on port {port}"
        )),
        None => log.line("LocoNet over TCP disabled; start with --loconet-tcp <port>".to_owned()),
    }
    log.line("simulated command station; no railway hardware is connected".to_owned());
    publish_power_status(&outputs, &power, &mut log).await?;

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
                    power: power.station.state().label().to_owned(),
                    rocnet_id: Some(rocnet_id),
                };
                publisher.put(serde_json::to_vec(&message)?).await?;
                for peer_id in refresh_peer_states(&mut peers) {
                    log.event(format!("Peer {peer_id} stale after 3 missed heartbeats; boundary state remains Stop"));
                }
                if let Some(frame) = pending_frame.take_if(|_| peers.values().any(|peer| peer.online)) {
                    let envelope = RocNetMeshEnvelope {
                        origin_node: options.id.clone(),
                        frame_ascii_hex: frame.to_ascii_hex(),
                    };
                    rocnet_publisher.put(serde_json::to_vec(&envelope)?).await?;
                    log.event(format!(
                        "published RocNet frame over Zenoh: {}",
                        envelope.frame_ascii_hex
                    ));
                }
            }
            sample = subscriber.recv_async() => {
                let sample = sample?;
                let payload = sample.payload().to_bytes();
                let message: Heartbeat = match serde_json::from_slice(payload.as_ref()) {
                    Ok(message) => message,
                    Err(error) => {
                        log.line(format!("[mesh] <- ignored invalid heartbeat on {}: {error}", sample.key_expr()));
                        continue;
                    }
                };
                if message.node != options.id {
                    let previous = peers
                        .get(&message.node)
                        .map(|peer| (peer.online, peer.power.clone()));
                    peers.insert(message.node.clone(), PeerState {
                        last_seen: Instant::now(),
                        online: true,
                        sequence: message.seq,
                        power: message.power.clone(),
                        rocnet_id: message.rocnet_id,
                    });
                    match previous {
                        Some((true, previous_power)) if previous_power != message.power => {
                            log.event(format!("Peer {} district power {} -> {}", message.node, previous_power, message.power));
                        }
                        Some((true, _)) => {}
                        _ => {
                            log.event(format!(
                                "Peer {} online (RocNet {}, seq {}, power {})",
                                message.node,
                                message.rocnet_id.map_or_else(|| "unknown".to_owned(), |id| id.to_string()),
                                message.seq,
                                message.power
                            ));
                            if message.rocnet_id == Some(rocnet_id) {
                                log.event(format!(
                                    "WARNING peer {} uses the same RocNet ID {rocnet_id} as this node; give each node its own --rocnet-address",
                                    message.node
                                ));
                            }
                        }
                    }
                }
            }
            sample = rocnet_subscriber.recv_async() => {
                let sample = sample?;
                let payload = sample.payload().to_bytes();
                let envelope: RocNetMeshEnvelope = match serde_json::from_slice(payload.as_ref()) {
                    Ok(envelope) => envelope,
                    Err(error) => {
                        log.line(format!("[mesh] <- ignored invalid RocNet envelope: {error}"));
                        continue;
                    }
                };
                if envelope.origin_node != options.id {
                    match RocNetFrame::from_ascii_hex(&envelope.frame_ascii_hex) {
                        Ok(frame) => log.event(format_rocnet_received_line(&envelope.origin_node, &frame)),
                        Err(error) => log.line(format!("[mesh] <- ignored invalid RocNet frame from {}: {error}", envelope.origin_node)),
                    }
                }
            }
            sample = command_station_subscriber.recv_async() => {
                let sample = sample?;
                let origin = sample_origin(&sample);
                let payload = sample.payload().to_bytes();
                match RocNetFrame::from_bytes(payload.as_ref()) {
                    Ok(frame) => {
                        log.line(format!("[mesh] <- {}  {}  from {origin}", frame.to_hex(), frame.describe()));
                        if sample.key_expr().as_str().ends_with("/rn/cs/all") {
                            if let Some(request) = frame.power_request_kind() {
                                handle_power_request(request, RequestSource::Mesh, &origin, &mut power, &outputs, &mut log).await?;
                            }
                        }
                    }
                    Err(error) => log.line(format!("[mesh] <- ignored invalid RocNet frame from {origin}: {error}")),
                }
            }
            event = next_loconet_event(&mut loconet_events) => match event {
                Some(LbEvent::Connected(peer)) => {
                    loconet_clients += 1;
                    log.event(format!("LocoNet client {peer} connected"));
                }
                Some(LbEvent::Disconnected(peer)) => {
                    loconet_clients = loconet_clients.saturating_sub(1);
                    log.event(format!("LocoNet client {peer} disconnected"));
                }
                Some(LbEvent::Received { peer, message }) => {
                    log.line(format!("[ln  ] <- {}  {}  from {peer}", message.to_hex(), message.describe()));
                    if let Some(request) = message.power_request_kind() {
                        let origin = format!("{} LocoNet", options.id);
                        handle_power_request(request, RequestSource::LocoNet, &origin, &mut power, &outputs, &mut log).await?;
                    }
                }
                Some(LbEvent::Rejected { peer, line, reason }) => {
                    log.line(format!("[ln  ] <- rejected from {peer}: {line} ({reason})"));
                }
                None => loconet_events = None,
            },
            _ = display_tick.tick(), if tui.is_some() => {
                let command = match tui.as_mut() {
                    Some(ui) => ui.handle_input()?,
                    None => UiCommand::None,
                };
                match command {
                    UiCommand::None => {}
                    UiCommand::Quit => break,
                    UiCommand::Power(request) => {
                        handle_power_request(request, RequestSource::Dashboard, &options.id, &mut power, &outputs, &mut log).await?;
                    }
                    UiCommand::SimulateShort => {
                        if power.station.simulate_short() {
                            log.event("SHORT in this district: power lost without request (simulated)".to_owned());
                            publish_power_status(&outputs, &power, &mut log).await?;
                            if let Some(server) = outputs.loconet {
                                let message = LocoNetMessage::power_request(PowerRequest::Off);
                                server.send(&message);
                                log.line(format!("[ln  ] -> {}  {}", message.to_hex(), message.describe()));
                            }
                        } else {
                            log.event("Short not simulated: district has no power".to_owned());
                        }
                    }
                }
                if let Some(ui) = tui.as_mut() {
                    let now = Instant::now();
                    let peers = peers.iter().map(|(id, peer)| PeerRow {
                        id: id.clone(),
                        online: peer.online,
                        last_seen: now.duration_since(peer.last_seen),
                        sequence: peer.sequence,
                        power: peer.power.clone(),
                        rocnet_id: peer.rocnet_id,
                    }).collect();
                    ui.draw(&Dashboard {
                        session: options.session.clone(),
                        node_id: options.id.clone(),
                        rocnet_id,
                        local_power: power.station.state(),
                        layout_power: power.layout.clone(),
                        loconet: match options.loconet_tcp {
                            Some(port) => format!("TCP :{port}, {loconet_clients} client(s)"),
                            None => "off".to_owned(),
                        },
                        scouting: ZENOH_SCOUTING_ADDRESS.to_owned(),
                        heartbeat_interval: HEARTBEAT_INTERVAL,
                        last_event: log.last_event.clone(),
                        peers,
                        cli_output: log.lines.iter().cloned().collect(),
                    })?;
                }
            }
        }
    }

    Ok(())
}

/// Applies a layout power request here and forwards it to the mesh and LocoNet clients
/// that have not seen it yet.
async fn handle_power_request(
    request: PowerRequest,
    source: RequestSource,
    origin: &str,
    power: &mut PowerControl,
    outputs: &PowerOutputs<'_>,
    log: &mut CliLog,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if source != RequestSource::Mesh {
        let frame = RocNetFrame::power_request(outputs.rocnet_id, request);
        outputs
            .requests
            .put(frame.as_bytes().to_vec())
            .attachment(origin.to_owned())
            .await?;
        log.line(format!(
            "[mesh] -> {}  {}",
            frame.to_hex(),
            frame.describe()
        ));
    }
    if source != RequestSource::LocoNet {
        if let Some(server) = outputs.loconet {
            let message = LocoNetMessage::power_request(request);
            server.send(&message);
            log.line(format!(
                "[ln  ] -> {}  {}",
                message.to_hex(),
                message.describe()
            ));
        }
    }

    let label = match request {
        PowerRequest::On => "ON",
        PowerRequest::Off => "OFF",
        PowerRequest::Halt => "STOP",
    };
    power.layout = format!(
        "{label}  ({request} from {origin}, {})",
        Local::now().format("%H:%M:%S")
    );
    log.event(format!("Layout {request} requested by {origin}"));
    if power.station.apply(request) {
        log.event(format!("District power {}", power.station.state().label()));
        publish_power_status(outputs, power, log).await?;
    }
    Ok(())
}

async fn publish_power_status(
    outputs: &PowerOutputs<'_>,
    power: &PowerControl,
    log: &mut CliLog,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let frame =
        RocNetFrame::power_status_event(outputs.rocnet_id, power.station.state().rocnet_status());
    outputs
        .status
        .put(frame.as_bytes().to_vec())
        .attachment(outputs.node_id.to_owned())
        .await?;
    log.line(format!(
        "[mesh] -> {}  {}",
        frame.to_hex(),
        frame.describe()
    ));
    Ok(())
}

fn sample_origin(sample: &Sample) -> String {
    sample
        .attachment()
        .and_then(|attachment| attachment.try_to_string().ok())
        .map_or_else(|| "unknown".to_owned(), |origin| origin.into_owned())
}

async fn next_loconet_event(events: &mut Option<mpsc::Receiver<LbEvent>>) -> Option<LbEvent> {
    match events {
        Some(events) => events.recv().await,
        None => std::future::pending().await,
    }
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
    let mut loconet_tcp = None;
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
            "--loconet-tcp" => {
                let value = next_value(&mut args, &flag)?;
                loconet_tcp = Some(value.parse::<u16>().map_err(|_| {
                    format!(
                        "invalid LocoNet TCP port '{value}'; expected an integer from 1 to 65535"
                    )
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
        loconet_tcp,
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
        format_rocnet_received_line, parse_args, record_cli_line, refresh_peer_states, Heartbeat,
        PeerState, HEARTBEAT_TIMEOUT, MAX_CLI_LINES, ZENOH_SCOUTING_ADDRESS,
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
        assert!(args.loconet_tcp.is_none());
        assert!(!args.plain);
    }

    #[test]
    fn loconet_tcp_port_is_parsed_and_validated() {
        let base = ["--session", "bench", "--id", "node-a", "--loconet-tcp"];
        let mut valid: Vec<String> = base.iter().map(|arg| (*arg).to_owned()).collect();
        valid.push("1234".into());
        assert_eq!(parse_args(valid).unwrap().loconet_tcp, Some(1234));

        let mut invalid: Vec<String> = base.iter().map(|arg| (*arg).to_owned()).collect();
        invalid.push("port".into());
        assert!(parse_args(invalid).is_err());
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
    fn heartbeat_carries_rocnet_id_and_accepts_older_nodes() {
        let current: Heartbeat =
            serde_json::from_str(r#"{"node":"windows","seq":7,"power":"ON","rocnet_id":102}"#)
                .unwrap();
        assert_eq!(current.rocnet_id, Some(102));

        let older: Heartbeat =
            serde_json::from_str(r#"{"node":"windows","seq":7,"power":"ON"}"#).unwrap();
        assert_eq!(older.rocnet_id, None);
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
                rocnet_id: Some(102),
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
