use crate::{
    clock::FastClock,
    dccex::{self, BoardStatus, DccExEvent, DccExHandle, DccExMessage},
    lbclient::{self, LbClientHandle},
    lbserver::{self, LbEvent, LbServerHandle},
    loconet::{LocoNetMessage, HIVE_RAIL_CLOCK_ID},
    locos::{Change, LocoInfo, LocoNetEdge, LocoRegister, RegisteredLoco, SlotOwner},
    power::{PowerRequest, PowerState, SimulatedCommandStation},
    rocnet::{ClockAction, ClockUpdate, RocNetFrame},
    ui::{command_for_key, ClockChange, Dashboard, PeerRow, TerminalUi, UiCommand},
};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    error::Error,
    io::{self, BufRead, IsTerminal},
    sync::mpsc as std_mpsc,
    thread,
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
pub(crate) const HEARTBEAT_TIMEOUT: Duration = Duration::from_millis(750);
pub(crate) const ZENOH_SCOUTING_ADDRESS: &str = "224.0.0.224:7446";
const MAX_CLI_LINES: usize = 200;
const DEFAULT_ROCNET_ID: u16 = 100;
const DEFAULT_LOCONET_TCP_PORT: u16 = 1234;
const THROTTLE_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

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
    loconet_connect: Option<String>,
    /// Who owns the slots on the LbServer clients' side (default: their own slot master).
    loconet_tcp_slots: SlotOwner,
    /// Who owns the slots on the node board's bus (default: the node, for FREDs).
    loconet_connect_slots: SlotOwner,
    dccex: Option<String>,
    plain: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Heartbeat {
    pub(crate) node: String,
    pub(crate) seq: u64,
    pub(crate) power: String,
    // Missing in heartbeats from nodes built before RocNet IDs were shared.
    #[serde(default)]
    pub(crate) rocnet_id: Option<u16>,
    // Missing in heartbeats from nodes built before locos were shared.
    #[serde(default)]
    pub(crate) locos: Vec<LocoInfo>,
    /// Checksum of the node's copy of the loco register (missing from older nodes).
    #[serde(default)]
    pub(crate) register: Option<u64>,
}

/// A node asking the allocator for a throttle number, on `hive/{session}/locos/request`.
#[derive(Debug, Serialize, Deserialize)]
struct ThrottleRequest {
    address: u16,
    node: String,
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
    locos: Vec<LocoInfo>,
    register: Option<u64>,
}

/// Where a power request came from, which decides where it still has to be echoed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestSource {
    Dashboard,
    LocoNet,
    /// The global emergency stop from another node.
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

/// Where this node's LocoNet messages go: the LbServer for Rocrail and JMRI, and the
/// LocoNet bus of a node board (its LocoNet buffer, reached as an LbServer client).
struct LocoNetPorts {
    server: Option<LbServerHandle>,
    bus: Option<LbClientHandle>,
}

impl LocoNetPorts {
    fn send(&self, message: &LocoNetMessage) {
        self.to_clients(message);
        self.to_bus(message);
    }

    fn to_clients(&self, message: &LocoNetMessage) {
        if let Some(server) = &self.server {
            server.send(message);
        }
    }

    fn to_bus(&self, message: &LocoNetMessage) {
        if let Some(bus) = &self.bus {
            bus.send(message);
        }
    }

    fn to_side(&self, side: LocoNetSide, message: &LocoNetMessage) {
        match side {
            LocoNetSide::Clients => self.to_clients(message),
            LocoNetSide::Bus => self.to_bus(message),
        }
    }
}

/// Which side a LocoNet message came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocoNetSide {
    /// Rocrail, JMRI and other clients of this node's LbServer.
    Clients,
    /// The LocoNet bus of a node board.
    Bus,
}

impl LocoNetSide {
    fn other(self) -> Self {
        match self {
            Self::Clients => Self::Bus,
            Self::Bus => Self::Clients,
        }
    }
}

/// Everything a power change has to be reported to.
struct PowerOutputs<'a> {
    node_id: &'a str,
    rocnet_id: u16,
    status: &'a Publisher<'static>,
    loconet: Option<&'a LocoNetPorts>,
    dccex: Option<&'a DccExHandle>,
}

struct PowerControl {
    station: SimulatedCommandStation,
    last_request: String,
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
    let session = zenoh::open(zenoh_config(
        options.listen.as_deref(),
        options.connect.as_deref(),
    )?)
    .await?;
    let key = format!("hive/{}/node/{}/hb", options.session, options.id);
    let subscriber_key = format!("hive/{}/node/*/hb", options.session);
    let publisher = session.declare_publisher(key).await?;
    let subscriber = session.declare_subscriber(subscriber_key).await?;
    let rocnet_key = format!("hive/{}/rocnet/frame", options.session);
    let rocnet_publisher = session.declare_publisher(rocnet_key.clone()).await?;
    let rocnet_subscriber = session.declare_subscriber(rocnet_key).await?;
    // The global emergency stop is the only power request that crosses the mesh.
    // Own requests are applied directly, so they are only routed to other nodes.
    let global_stop_publisher = session
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
    let clock_publisher = session
        .declare_publisher(format!("hive/{}/rn/ck", options.session))
        .allowed_destination(Locality::Remote)
        .await?;
    let clock_subscriber = session
        .declare_subscriber(format!("hive/{}/rn/ck", options.session))
        .await?;
    // The layout's loco register and throttle numbers (Hive-Rail namespace, see locos.rs).
    let loco_update_publisher = session
        .declare_publisher(format!("hive/{}/locos/update", options.session))
        .allowed_destination(Locality::Remote)
        .await?;
    let loco_update_subscriber = session
        .declare_subscriber(format!("hive/{}/locos/update", options.session))
        .await?;
    let throttle_request_publisher = session
        .declare_publisher(format!("hive/{}/locos/request", options.session))
        .allowed_destination(Locality::Remote)
        .await?;
    let throttle_request_subscriber = session
        .declare_subscriber(format!("hive/{}/locos/request", options.session))
        .await?;
    // A dispatcher screen sends no heartbeat, so it never counts as allocator or clock
    // master; it says hello instead and the allocator answers with the table and clock.
    let observer_subscriber = session
        .declare_subscriber(format!("hive/{}/observer/hello", options.session))
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
    let (loconet_bus, mut loconet_bus_events) = match options.loconet_connect.clone() {
        Some(target) => {
            let (client, events) = lbclient::start(target);
            (Some(client), Some(events))
        }
        None => (None, None),
    };
    let mut loconet_bus_state = match &options.loconet_connect {
        Some(target) => format!("bus {target}, connecting"),
        None => String::new(),
    };
    let loconet_ports = LocoNetPorts {
        server: loconet_server.clone(),
        bus: loconet_bus,
    };
    let mut register = LocoRegister::new(&options.id);
    // The slots of each LocoNet connection and who owns them.
    let mut client_edge = LocoNetEdge::new(options.loconet_tcp_slots);
    let mut bus_edge = LocoNetEdge::new(options.loconet_connect_slots);
    // Loco addresses this node asked the allocator a throttle number for, and since when.
    let mut throttle_requests: HashMap<u16, Instant> = HashMap::new();
    // Peers whose loco register checksum differs from ours, and since when.
    let mut register_mismatch: HashMap<String, Instant> = HashMap::new();
    let (dccex_driver, mut dccex_events) = match options.dccex.clone() {
        Some(target) => {
            let (driver, events) = dccex::start(target);
            (Some(driver), Some(events))
        }
        None => (None, None),
    };
    let mut dccex_tracks = dccex::DistrictTracks::default();
    // Set once DCC-EX answers <U 0>: it runs the Hive-Rail board add-on.
    let mut board_protocol: Option<u16> = None;
    let mut command_station = match &options.dccex {
        Some(target) => format!("DCC-EX {target}, connecting"),
        None => "simulated".to_owned(),
    };
    let outputs = PowerOutputs {
        node_id: &options.id,
        rocnet_id,
        status: &power_status_publisher,
        loconet: (loconet_ports.server.is_some() || loconet_ports.bus.is_some())
            .then_some(&loconet_ports),
        dccex: dccex_driver.as_ref(),
    };
    let mut power = PowerControl {
        station: SimulatedCommandStation::new(),
        last_request: "no request yet".to_owned(),
    };
    if dccex_driver.is_some() {
        power.station.report(PowerState::Unknown);
    }

    let mut peers: HashMap<String, PeerState> = HashMap::new();
    let mut log = CliLog {
        lines: VecDeque::with_capacity(MAX_CLI_LINES),
        last_event: "Waiting for Zenoh peers".to_owned(),
        echo: tui.is_none(),
    };
    let mut sequence = 0_u64;
    let mut clock = FastClock::new(Instant::now());
    // Last clock state written to LocoNet: minute of day, running, rate.
    let mut clock_written: Option<(u16, bool, u8)> = None;
    let mut clock_writers_warned: HashSet<std::net::SocketAddr> = HashSet::new();
    let mut clock_tick = time::interval(Duration::from_millis(100));
    clock_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
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
    // Without the dashboard, the same keys are read as lines from piped stdin, so scripts
    // can switch district power or simulate a short. A terminal is not read: a `--plain`
    // node started in the background would be stopped by the shell.
    let mut stdin_keys = if tui.is_none() && !io::stdin().is_terminal() {
        Some(spawn_stdin_reader())
    } else {
        None
    };
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
    log.line(format!(
        "LocoNet slots: clients {} (external = their own slot master, node = this node), node board bus {}",
        options.loconet_tcp_slots.label(),
        options.loconet_connect_slots.label()
    ));
    match &options.dccex {
        Some(target) => log.line(format!("DCC-EX command station on {target}")),
        None => log.line("simulated command station; no railway hardware is connected".to_owned()),
    }
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
                    locos: register.locos(),
                    register: Some(register.checksum()),
                };
                publisher.put(serde_json::to_vec(&message)?).await?;
                track_register_mismatch(&peers, register.checksum(), &mut register_mismatch);
                // Once a second: the node board's screen, also its link heartbeat.
                if let (Some(driver), Some(_)) = (outputs.dccex, board_protocol) {
                    if sequence.is_multiple_of(4) {
                        let loco = register.last_driven();
                        let status = BoardStatus {
                            rocnet_id,
                            nodes: 1 + peers.values().filter(|peer| peer.online).count(),
                            locos: register.locos().len(),
                            layout_locos: register.taken(),
                            allocator: lowest_online(&peers, &options.id),
                            register_in_sync: !register_sync_label(&peers, &register_mismatch).starts_with("LOCOS DIFFER"),
                            register_compared: peers.values().any(|peer| peer.online && peer.register.is_some()),
                            emergency_stop: power.station.state() == PowerState::EmergencyStop,
                            loco: loco.and_then(|loco| {
                                loco.address.map(|address| (address, loco.speed, loco.forward, loco.functions))
                            }),
                        };
                        driver.send(&status.command());
                    }
                }
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
                        locos: message.locos,
                        register: message.register,
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
                            if clock.known() {
                                publish_clock(&clock_publisher, rocnet_id, ClockAction::Sync, &clock, &mut log).await?;
                            }
                            let snapshot = register.snapshot();
                            if lowest_online(&peers, &options.id) && !snapshot.is_empty() {
                                log.line(format!("[mesh] -> loco register, {} locos, for {}", snapshot.len(), message.node));
                                publish_locos(&loco_update_publisher, &options.id, snapshot).await?;
                            }
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
                            match frame.power_request_kind() {
                                Some(PowerRequest::Halt) => {
                                    handle_power_request(PowerRequest::Halt, RequestSource::Mesh, &origin, &mut power, &outputs, &mut log).await?;
                                }
                                Some(request) => log.line(format!("[mesh] <- ignored layout-wide {request} from {origin}; power is per district")),
                                None => {}
                            }
                        }
                    }
                    Err(error) => log.line(format!("[mesh] <- ignored invalid RocNet frame from {origin}: {error}")),
                }
            }
            event = next_dccex_event(&mut dccex_events) => match event {
                Some(DccExEvent::Connected(target)) => {
                    // A reconnect may find a different board or configuration.
                    dccex_tracks = dccex::DistrictTracks::default();
                    board_protocol = None;
                    command_station = format!("DCC-EX {target}");
                    log.event(format!("DCC-EX connected on {target}"));
                    // Give DCC-EX the current clock on the next tick.
                    clock_written = None;
                }
                Some(DccExEvent::Disconnected(reason)) => {
                    command_station = format!("DCC-EX {}, disconnected", options.dccex.as_deref().unwrap_or_default());
                    log.event(format!("DCC-EX disconnected: {reason}"));
                    if power.station.report(PowerState::Unknown) {
                        log.event("District power UNKNOWN: no contact with the command station".to_owned());
                        publish_power_status(&outputs, &power, &mut log).await?;
                    }
                }
                Some(DccExEvent::Received(message)) => {
                    // Text for the command station's own display is not worth logging.
                    if !matches!(message, DccExMessage::Display(_)) {
                        log.line(format!("[DCC-EX] <- {}", message.describe()));
                    }
                    if matches!(message, DccExMessage::TrackMode { .. }) {
                        dccex_tracks.update(&message);
                    }
                    let changed = match &message {
                        DccExMessage::Power { .. } => {
                            let Some(on) = dccex_tracks.update(&message) else { continue };
                            let changed = power.station.report_power(on);
                            // Requested changes are applied before DCC-EX confirms them, so
                            // this one came from a button, a WiThrottle or another client.
                            if changed {
                                let request = if on { PowerRequest::On } else { PowerRequest::Off };
                                power.last_request = format!(
                                    "{}  ({request} on DCC-EX, {})",
                                    power.station.state().label(),
                                    Local::now().format("%H:%M:%S")
                                );
                                if let Some(server) = outputs.loconet {
                                    let message = LocoNetMessage::power_request(request);
                                    server.send(&message);
                                    log.line(format!("[ln  ] -> {}  {}", message.to_hex(), message.describe()));
                                }
                            }
                            changed
                        }
                        DccExMessage::Overload { detail, .. } => {
                            let changed = power.station.report(PowerState::Short);
                            if changed {
                                log.event(format!("SHORT in this district: {detail}"));
                                send_loconet_power_off(&outputs, &mut log);
                            }
                            changed
                        }
                        DccExMessage::TrackNormal { track } => {
                            let changed = power.station.state() == PowerState::Short
                                && power.station.report(PowerState::On);
                            if changed {
                                log.event(format!("Short cleared: DCC-EX restored power on track {track}"));
                            }
                            changed
                        }
                        DccExMessage::Board { protocol } => {
                            if board_protocol.replace(*protocol).is_none() {
                                log.event(format!("DCC-EX runs the Hive-Rail board add-on (protocol {protocol}); sending status to its screen"));
                            }
                            false
                        }
                        DccExMessage::Version(version) => {
                            command_station = format!("{version} on {}", options.dccex.as_deref().unwrap_or_default());
                            false
                        }
                        _ => false,
                    };
                    if changed {
                        log.event(format!("District power {} (reported by DCC-EX)", power.station.state().label()));
                        publish_power_status(&outputs, &power, &mut log).await?;
                    }
                }
                None => dccex_events = None,
            },
            (side, event) = next_loconet_event(&mut loconet_events, &mut loconet_bus_events) => match event {
                Some(LbEvent::Connected(peer)) if side == LocoNetSide::Bus => {
                    loconet_bus_state = format!("bus {peer} connected");
                    // Give the bus the current clock on the next tick.
                    clock_written = None;
                    log.event(format!("LocoNet bus connected (node board at {peer})"));
                }
                Some(LbEvent::Disconnected { reason, .. }) if side == LocoNetSide::Bus => {
                    let reason = reason.unwrap_or_default();
                    loconet_bus_state = format!("bus {}, disconnected", options.loconet_connect.as_deref().unwrap_or_default());
                    log.event(format!("LocoNet bus disconnected: {reason}"));
                }
                Some(LbEvent::Connected(peer)) => {
                    loconet_clients += 1;
                    // Give the new client the current clock on the next tick.
                    clock_written = None;
                    log.event(format!("LocoNet client {peer} connected"));
                }
                Some(LbEvent::FormatDetected { peer, format }) => {
                    // Messages are skipped until the format is known; send the clock now.
                    clock_written = None;
                    log.event(format!("LocoNet client {peer} uses the {format} format"));
                }
                Some(LbEvent::Disconnected { peer, reason }) => {
                    loconet_clients = loconet_clients.saturating_sub(1);
                    clock_writers_warned.remove(&peer);
                    match reason {
                        Some(reason) => log.event(format!("LocoNet client {peer} disconnected: {reason}")),
                        None => log.event(format!("LocoNet client {peer} disconnected")),
                    }
                }
                Some(LbEvent::Received { peer, message }) => {
                    // The bus and the clients see each other's traffic, as on one LocoNet,
                    // except slot messages: slot numbers belong to one connection and are
                    // translated through the loco register below.
                    let other_side = side.other();
                    let from = match side {
                        LocoNetSide::Clients => peer.to_string(),
                        LocoNetSide::Bus => format!("bus {peer}"),
                    };
                    if !message.is_slot_message() {
                        loconet_ports.to_side(other_side, &message);
                    }
                    log.line(format!("[ln  ] <- {}  {}  from {from}", message.to_hex(), message.describe()));
                    let (edge, other_edge) = match side {
                        LocoNetSide::Clients => (&mut client_edge, &bus_edge),
                        LocoNetSide::Bus => (&mut bus_edge, &client_edge),
                    };
                    let outcome = edge.receive(
                        &mut register,
                        &message,
                        loconet_track_status(power.station.state()),
                        lowest_online(&peers, &options.id),
                    );
                    send_to_side(&loconet_ports, side, outcome.replies, &mut log);
                    tell_side(&loconet_ports, other_side, other_edge, &outcome.changes, &mut log);
                    for loco in &outcome.publish {
                        log.line(format!("[mesh] -> {}", loco.describe()));
                    }
                    publish_locos(&loco_update_publisher, &options.id, outcome.publish).await?;
                    if let Some(address) = outcome.request {
                        throttle_requests.insert(address, Instant::now());
                        log.line(format!("[mesh] -> throttle number request for loco {address}"));
                        let request = ThrottleRequest { address, node: options.id.clone() };
                        throttle_request_publisher.put(serde_json::to_vec(&request)?).attachment(options.id.clone()).await?;
                    }
                    if let Some(request) = message.power_request_kind() {
                        let origin = format!("{} LocoNet", options.id);
                        handle_power_request(request, RequestSource::LocoNet, &origin, &mut power, &outputs, &mut log).await?;
                    }
                    // Where the node is slot master it answers the clock slot too; an external
                    // slot master answers it itself (with the time Hive-Rail writes to it).
                    let owner = match side {
                        LocoNetSide::Clients => client_edge.owner(),
                        LocoNetSide::Bus => bus_edge.owner(),
                    };
                    if message.is_fast_clock_request() && clock.known() && owner == SlotOwner::Node {
                        let minute = clock.minute_of_day(Instant::now());
                        let reply = LocoNetMessage::fast_clock_read(
                            (minute / 60) as u8,
                            (minute % 60) as u8,
                            if clock.running() { clock.rate() } else { 0 },
                            loconet_track_status(power.station.state()),
                        );
                        send_to_side(&loconet_ports, side, vec![reply], &mut log);
                    }
                    if let Some(slot) = message.fast_clock() {
                        if slot.valid && slot.setter != HIVE_RAIL_CLOCK_ID && message.opcode() == 0xEF && clock_writers_warned.insert(peer) {
                            log.event(format!(
                                "Ignored fast clock write from {peer} ({:02}:{:02}): Hive-Rail commands the clock; turn off the client's clock sync",
                                slot.hour, slot.minute
                            ));
                        }
                    }
                }
                Some(LbEvent::Rejected { peer, input, reason }) => {
                    log.line(format!("[ln  ] <- rejected from {peer}: {input} ({reason})"));
                }
                None => match side {
                    LocoNetSide::Clients => loconet_events = None,
                    LocoNetSide::Bus => loconet_bus_events = None,
                },
            },
            sample = loco_update_subscriber.recv_async() => {
                let sample = sample?;
                let origin = sample_origin(&sample);
                match serde_json::from_slice::<Vec<RegisteredLoco>>(sample.payload().to_bytes().as_ref()) {
                    Ok(locos) => {
                        for loco in &locos {
                            log.line(format!("[mesh] <- {}  from {origin}", loco.describe()));
                        }
                        let merged = register.merge(locos);
                        // A loco driven elsewhere that a connection here also knows.
                        tell_side(&loconet_ports, LocoNetSide::Clients, &client_edge, &merged.changes, &mut log);
                        tell_side(&loconet_ports, LocoNetSide::Bus, &bus_edge, &merged.changes, &mut log);
                        // Throttles that waited for a throttle number get their slot data.
                        let track_status = loconet_track_status(power.station.state());
                        for address in merged.answered {
                            for (side, edge) in [(LocoNetSide::Clients, &client_edge), (LocoNetSide::Bus, &bus_edge)] {
                                let answer = edge.answer(&register, address, track_status);
                                send_to_side(&loconet_ports, side, answer.into_iter().collect(), &mut log);
                            }
                        }
                    }
                    Err(error) => log.line(format!("[mesh] <- ignored invalid loco update from {origin}: {error}")),
                }
            }
            sample = observer_subscriber.recv_async() => {
                let sample = sample?;
                let observer = sample_origin(&sample);
                // Every node answers with its copy of the register, so a dispatcher gets
                // it even when the allocator cannot answer; the versions sort out repeats.
                log.line(format!("[mesh] <- hello from observer {observer}; sending loco register"));
                publish_locos(&loco_update_publisher, &options.id, register.snapshot()).await?;
                if lowest_online(&peers, &options.id) && clock.known() {
                    publish_clock(&clock_publisher, rocnet_id, ClockAction::Sync, &clock, &mut log).await?;
                }
            }
            sample = throttle_request_subscriber.recv_async() => {
                let sample = sample?;
                match serde_json::from_slice::<ThrottleRequest>(sample.payload().to_bytes().as_ref()) {
                    Ok(request) => {
                        log.line(format!("[mesh] <- throttle number request for loco {} from {}", request.address, request.node));
                        if lowest_online(&peers, &options.id) {
                            match register.allocate_for(request.address, &request.node) {
                                Some(loco) => {
                                    log.event(format!("Throttle {} for loco {} on {}", loco.throttle.unwrap_or(0), request.address, request.node));
                                    log.line(format!("[mesh] -> {}", loco.describe()));
                                    publish_locos(&loco_update_publisher, &options.id, vec![loco]).await?;
                                }
                                None => log.event(format!("No free throttle number for loco {} on {}", request.address, request.node)),
                            }
                        }
                    }
                    Err(error) => log.line(format!("[mesh] <- ignored invalid throttle number request: {error}")),
                }
            }
            sample = clock_subscriber.recv_async() => {
                let sample = sample?;
                let origin = sample_origin(&sample);
                match RocNetFrame::from_bytes(sample.payload().to_bytes().as_ref()) {
                    Ok(frame) => match frame.clock_update() {
                        Some(update) => {
                            log.line(format!("[mesh] <- {}  {}  from {origin}", frame.to_hex(), frame.describe()));
                            let was = clock.label(Instant::now());
                            let rate = if update.divider == 0 { clock.rate() } else { update.divider };
                            clock.set(u16::from(update.hour) * 60 + u16::from(update.minute), rate, update.divider != 0, Instant::now());
                            if update.action == ClockAction::Set {
                                log.event(format!("Fast clock set by {origin}: {was} -> {}", clock.label(Instant::now())));
                            }
                        }
                        None => log.line(format!("[mesh] <- ignored clock frame from {origin}: {}", frame.describe())),
                    },
                    Err(error) => log.line(format!("[mesh] <- ignored invalid clock frame from {origin}: {error}")),
                }
            }
            _ = clock_tick.tick() => {
                let now = Instant::now();
                // Throttle number requests nobody answered: the throttle asks again by itself.
                throttle_requests.retain(|&address, since| {
                    if !register.is_pending(address) {
                        return false;
                    }
                    if now.duration_since(*since) < THROTTLE_REQUEST_TIMEOUT {
                        return true;
                    }
                    register.cancel(address);
                    log.event(format!("No allocator answered for loco {address}; the throttle can ask again"));
                    false
                });
                if clock.known() {
                    let state = (clock.minute_of_day(now), clock.running(), clock.rate());
                    if clock_written != Some(state) {
                        let minute_ticked = clock_written.is_some_and(|(minute, ..)| minute != state.0);
                        clock_written = Some(state);
                        // The clock master keeps every node in phase on each fast minute.
                        let master = lowest_online(&peers, &options.id);
                        if minute_ticked && master {
                            publish_clock(&clock_publisher, rocnet_id, ClockAction::Sync, &clock, &mut log).await?;
                        }
                        if let Some(server) = outputs.loconet {
                            let message = LocoNetMessage::fast_clock_write(
                                (state.0 / 60) as u8,
                                (state.0 % 60) as u8,
                                if state.1 { state.2 } else { 0 },
                                loconet_track_status(power.station.state()),
                            );
                            server.send(&message);
                            log.line(format!("[ln  ] -> {}  {}", message.to_hex(), message.describe()));
                        }
                        if let Some(driver) = outputs.dccex {
                            let command = dccex::clock_command(state.0, if state.1 { state.2 } else { 0 });
                            driver.send(&command);
                            log.line(format!("[DCC-EX] -> {command}"));
                        }
                    }
                }
            }
            _ = display_tick.tick(), if tui.is_some() || stdin_keys.is_some() => {
                let command = match tui.as_mut() {
                    Some(ui) => ui.handle_input()?,
                    None => next_stdin_command(&mut stdin_keys, &mut log),
                };
                match command {
                    UiCommand::None => {}
                    UiCommand::Quit => break,
                    UiCommand::Power(request) => {
                        handle_power_request(request, RequestSource::Dashboard, &options.id, &mut power, &outputs, &mut log).await?;
                    }
                    UiCommand::GlobalStop => {
                        let frame = RocNetFrame::power_request(rocnet_id, PowerRequest::Halt);
                        global_stop_publisher.put(frame.as_bytes().to_vec()).attachment(options.id.clone()).await?;
                        log.line(format!("[mesh] -> {}  {}", frame.to_hex(), frame.describe()));
                        log.event("GLOBAL EMERGENCY STOP sent to every district".to_owned());
                        handle_power_request(PowerRequest::Halt, RequestSource::Dashboard, &options.id, &mut power, &outputs, &mut log).await?;
                    }
                    UiCommand::Clock(change) => {
                        let now = Instant::now();
                        let was = clock.label(now);
                        match change {
                            ClockChange::ToggleRunning => clock.toggle_running(now),
                            ClockChange::Rate(delta) => clock.change_rate(delta, now),
                            ClockChange::Hours(delta) => clock.shift_hours(delta, now),
                        }
                        log.event(format!("Fast clock set here: {was} -> {}", clock.label(now)));
                        publish_clock(&clock_publisher, rocnet_id, ClockAction::Set, &clock, &mut log).await?;
                    }
                    UiCommand::RemovePeer(peer_id) => match peers.get(&peer_id) {
                        Some(peer) if peer.online => {
                            log.event(format!("Peer {peer_id} is online; only stale peers can be removed"));
                        }
                        Some(_) => {
                            peers.remove(&peer_id);
                            log.event(format!("Removed stale peer {peer_id}; it returns if its heartbeat resumes"));
                        }
                        None => {}
                    },
                    UiCommand::SimulateShort => {
                        if power.station.simulate_short() {
                            log.event("SHORT in this district: power lost without request (simulated)".to_owned());
                            publish_power_status(&outputs, &power, &mut log).await?;
                            send_loconet_power_off(&outputs, &mut log);
                        } else {
                            log.event("Short not simulated: district has no power".to_owned());
                        }
                    }
                }
                if let Some(ui) = tui.as_mut() {
                    let now = Instant::now();
                    let allocator = lowest_online_id(&peers, &options.id).to_owned();
                    let slot_sync = register_sync_label(&peers, &register_mismatch);
                    let mut peers: Vec<PeerRow> = peers.iter().map(|(id, peer)| PeerRow {
                        id: id.clone(),
                        online: peer.online,
                        last_seen: now.duration_since(peer.last_seen),
                        sequence: peer.sequence,
                        power: peer.power.clone(),
                        rocnet_id: peer.rocnet_id,
                        locos: peer.locos.clone(),
                        allocator: *id == allocator,
                    }).collect();
                    // A fixed order keeps the cursor on the same peer between frames.
                    peers.sort_by(|a, b| a.id.cmp(&b.id));
                    ui.draw(&Dashboard {
                        session: options.session.clone(),
                        node_id: options.id.clone(),
                        rocnet_id,
                        local_power: power.station.state(),
                        local_sequence: sequence,
                        local_locos: register.locos(),
                        last_request: power.last_request.clone(),
                        clock: clock.label(Instant::now()),
                        local_allocator: allocator == options.id,
                        slot_sync,
                        command_station: command_station.clone(),
                        loconet: match (options.loconet_tcp, loconet_bus_state.is_empty()) {
                            (Some(port), true) => format!("TCP :{port}, {loconet_clients} client(s)"),
                            (Some(port), false) => format!("TCP :{port}, {loconet_clients} client(s); {loconet_bus_state}"),
                            (None, false) => loconet_bus_state.clone(),
                            (None, true) => "off".to_owned(),
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

/// Applies a power request to this node's district and echoes it to the local LocoNet
/// clients that have not seen it yet. Other nodes only see the resulting status.
async fn handle_power_request(
    request: PowerRequest,
    source: RequestSource,
    origin: &str,
    power: &mut PowerControl,
    outputs: &PowerOutputs<'_>,
    log: &mut CliLog,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if let Some(driver) = outputs.dccex {
        let command = dccex::power_command(request);
        driver.send(command);
        log.line(format!("[DCC-EX] -> {command}  {request}"));
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
    power.last_request = format!(
        "{label}  ({request} from {origin}, {})",
        Local::now().format("%H:%M:%S")
    );
    log.event(format!("District {request} requested by {origin}"));
    if power.station.apply(request) {
        log.event(format!("District power {}", power.station.state().label()));
        publish_power_status(outputs, power, log).await?;
    }
    Ok(())
}

async fn publish_clock(
    publisher: &Publisher<'static>,
    rocnet_id: u16,
    action: ClockAction,
    clock: &FastClock,
    log: &mut CliLog,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let minute = clock.minute_of_day(Instant::now());
    let frame = RocNetFrame::clock(
        rocnet_id,
        ClockUpdate {
            action,
            hour: (minute / 60) as u8,
            minute: (minute % 60) as u8,
            divider: if clock.running() { clock.rate() } else { 0 },
        },
    );
    publisher
        .put(frame.as_bytes().to_vec())
        .attachment(rocnet_id.to_string())
        .await?;
    log.line(format!(
        "[mesh] -> {}  {}",
        frame.to_hex(),
        frame.describe()
    ));
    Ok(())
}

/// LocoNet `TRK` byte: bit 0 power on, bit 1 set when not idle, bit 2 LocoNet 1.1.
fn loconet_track_status(state: PowerState) -> u8 {
    match state {
        PowerState::On => 0x07,
        PowerState::EmergencyStop => 0x05,
        PowerState::Off | PowerState::Short | PowerState::Unknown => 0x04,
    }
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

fn spawn_stdin_reader() -> std_mpsc::Receiver<String> {
    let (sender, receiver) = std_mpsc::channel();
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    receiver
}

/// One key line from stdin, as `s` for a short or `0` for power off; `q` quits.
/// The reader is dropped when stdin closes.
fn next_stdin_command(
    keys: &mut Option<std_mpsc::Receiver<String>>,
    log: &mut CliLog,
) -> UiCommand {
    let Some(receiver) = keys else {
        return UiCommand::None;
    };
    match receiver.try_recv() {
        Ok(line) => {
            let line = line.trim();
            let mut chars = line.chars();
            match (chars.next(), chars.next()) {
                (Some('q'), None) => UiCommand::Quit,
                (Some(key), None) => command_for_key(key).unwrap_or_else(|| {
                    log.line(format!("ignored stdin key {line:?}"));
                    UiCommand::None
                }),
                (None, _) => UiCommand::None,
                _ => {
                    log.line(format!(
                        "ignored stdin line {line:?}; send one key per line"
                    ));
                    UiCommand::None
                }
            }
        }
        Err(std_mpsc::TryRecvError::Empty) => UiCommand::None,
        Err(std_mpsc::TryRecvError::Disconnected) => {
            *keys = None;
            UiCommand::None
        }
    }
}

/// Peer mode with multicast scouting on the LAN; endpoints are optional overrides.
pub(crate) fn zenoh_config(
    listen: Option<&str>,
    connect: Option<&str>,
) -> Result<Config, Box<dyn Error + Send + Sync>> {
    let mut config = Config::default();
    config.insert_json5("mode", "\"peer\"")?;
    config.insert_json5("scouting/multicast/enabled", "true")?;
    config.insert_json5(
        "scouting/multicast/address",
        &format!("\"{ZENOH_SCOUTING_ADDRESS}\""),
    )?;
    if let Some(endpoint) = listen {
        config.insert_json5("listen/endpoints", &format!("[\"{endpoint}\"]"))?;
    }
    if let Some(endpoint) = connect {
        config.insert_json5("connect/endpoints", &format!("[\"{endpoint}\"]"))?;
    }
    Ok(config)
}

/// The online node with the lowest id is the clock master and the slot allocator.
fn lowest_online(peers: &HashMap<String, PeerState>, node_id: &str) -> bool {
    lowest_online_id(peers, node_id) == node_id
}

fn lowest_online_id<'a>(peers: &'a HashMap<String, PeerState>, node_id: &'a str) -> &'a str {
    peers
        .iter()
        .filter(|(_, peer)| peer.online)
        .map(|(id, _)| id.as_str())
        .fold(node_id, |lowest, id| lowest.min(id))
}

/// Copies of the loco register may differ for a moment while a change travels; only a
/// difference that lasts this long is reported.
pub(crate) const REGISTER_SYNC_GRACE: Duration = Duration::from_secs(2);

fn track_register_mismatch(
    peers: &HashMap<String, PeerState>,
    own: u64,
    mismatch: &mut HashMap<String, Instant>,
) {
    for (id, peer) in peers {
        match peer.register {
            Some(checksum) if peer.online && checksum != own => {
                mismatch.entry(id.clone()).or_insert_with(Instant::now);
            }
            _ => {
                mismatch.remove(id);
            }
        }
    }
    mismatch.retain(|id, _| peers.contains_key(id));
}

fn register_sync_label(
    peers: &HashMap<String, PeerState>,
    mismatch: &HashMap<String, Instant>,
) -> String {
    let mut differs: Vec<&str> = mismatch
        .iter()
        .filter(|(_, since)| since.elapsed() >= REGISTER_SYNC_GRACE)
        .map(|(id, _)| id.as_str())
        .collect();
    differs.sort_unstable();
    let compared = peers
        .values()
        .filter(|peer| peer.online && peer.register.is_some())
        .count();
    match (differs.is_empty(), compared) {
        (true, 0) => "locos: no peer to compare".to_owned(),
        (true, _) => format!("locos in sync with {compared} peer(s)"),
        (false, _) => format!("LOCOS DIFFER on {}", differs.join(", ")),
    }
}

async fn publish_locos(
    publisher: &Publisher<'static>,
    node_id: &str,
    locos: Vec<RegisteredLoco>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if !locos.is_empty() {
        publisher
            .put(serde_json::to_vec(&locos)?)
            .attachment(node_id.to_owned())
            .await?;
    }
    Ok(())
}

fn send_to_side(
    ports: &LocoNetPorts,
    side: LocoNetSide,
    messages: Vec<LocoNetMessage>,
    log: &mut CliLog,
) {
    for message in messages {
        ports.to_side(side, &message);
        let to = match side {
            LocoNetSide::Clients => "",
            LocoNetSide::Bus => "  to bus",
        };
        log.line(format!(
            "[ln  ] -> {}  {}{to}",
            message.to_hex(),
            message.describe()
        ));
    }
}

/// Tells one LocoNet connection about locos that changed elsewhere, in its own slot
/// numbers; nothing for locos it does not know.
fn tell_side(
    ports: &LocoNetPorts,
    side: LocoNetSide,
    edge: &LocoNetEdge,
    changes: &[Change],
    log: &mut CliLog,
) {
    for (before, after) in changes {
        send_to_side(ports, side, edge.notify(before.as_ref(), after), log);
    }
}

/// Tells local LocoNet clients the district lost power, as a booster trip would.
fn send_loconet_power_off(outputs: &PowerOutputs<'_>, log: &mut CliLog) {
    if let Some(server) = outputs.loconet {
        let message = LocoNetMessage::power_request(PowerRequest::Off);
        server.send(&message);
        log.line(format!(
            "[ln  ] -> {}  {}",
            message.to_hex(),
            message.describe()
        ));
    }
}

async fn next_dccex_event(events: &mut Option<mpsc::Receiver<DccExEvent>>) -> Option<DccExEvent> {
    match events {
        Some(events) => events.recv().await,
        None => std::future::pending().await,
    }
}

async fn next_loconet_event(
    clients: &mut Option<mpsc::Receiver<LbEvent>>,
    bus: &mut Option<mpsc::Receiver<LbEvent>>,
) -> (LocoNetSide, Option<LbEvent>) {
    tokio::select! {
        event = recv_or_wait(clients) => (LocoNetSide::Clients, event),
        event = recv_or_wait(bus) => (LocoNetSide::Bus, event),
    }
}

async fn recv_or_wait(events: &mut Option<mpsc::Receiver<LbEvent>>) -> Option<LbEvent> {
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
    let mut loconet_connect = None;
    let mut loconet_tcp_slots = SlotOwner::External;
    let mut loconet_connect_slots = SlotOwner::Node;
    let mut dccex = None;
    let mut plain = false;
    let mut args = args.into_iter().peekable();

    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--identify" => identify = true,
            "--plain" => plain = true,
            "--session" => session = Some(next_value(&mut args, &flag)?),
            "--id" => id = Some(next_value(&mut args, &flag)?),
            "--listen" => listen = Some(next_value(&mut args, &flag)?),
            "--connect" => connect = Some(next_value(&mut args, &flag)?),
            "--send-rocnet-hex" => send_rocnet_hex = Some(next_value(&mut args, &flag)?),
            "--dccex" => dccex = Some(next_value(&mut args, &flag)?),
            "--loconet-tcp-slots" | "--loconet-connect-slots" => {
                let value = next_value(&mut args, &flag)?;
                let owner = SlotOwner::parse(&value).ok_or_else(|| {
                    format!("invalid {flag} '{value}'; expected 'external' or 'node'")
                })?;
                if flag == "--loconet-tcp-slots" {
                    loconet_tcp_slots = owner;
                } else {
                    loconet_connect_slots = owner;
                }
            }
            "--loconet-connect" => {
                // A node board's LocoNet buffer listens on 1234 like any LbServer.
                let target = next_value(&mut args, &flag)?;
                loconet_connect = Some(if target.contains(':') {
                    target
                } else {
                    format!("{target}:{DEFAULT_LOCONET_TCP_PORT}")
                });
            }
            "--rocnet-address" => {
                let value = next_value(&mut args, &flag)?;
                rocnet_address = Some(value.parse::<u16>().map_err(|_| {
                    format!("invalid RocNet address '{value}'; expected an integer from 0 to 65535")
                })?);
            }
            "--loconet-tcp" => {
                // The port is optional; LocoNet over TCP uses 1234 by convention.
                let Some(value) = args.next_if(|value| !value.starts_with("--")) else {
                    loconet_tcp = Some(DEFAULT_LOCONET_TCP_PORT);
                    continue;
                };
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
        loconet_connect,
        loconet_tcp_slots,
        loconet_connect_slots,
        dccex,
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

        let without_port: Vec<String> = base.iter().map(|arg| (*arg).to_owned()).collect();
        assert_eq!(parse_args(without_port).unwrap().loconet_tcp, Some(1234));

        let mut followed_by_flag: Vec<String> = base.iter().map(|arg| (*arg).to_owned()).collect();
        followed_by_flag.push("--plain".into());
        let parsed = parse_args(followed_by_flag).unwrap();
        assert_eq!(parsed.loconet_tcp, Some(1234));
        assert!(parsed.plain);
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
        assert!(older.locos.is_empty());

        let with_locos: Heartbeat = serde_json::from_str(
            r#"{"node":"mac","seq":3,"power":"ON","rocnet_id":101,"locos":[{"slot":1,"address":3,"speed":41,"forward":false,"functions":3}]}"#,
        )
        .unwrap();
        assert_eq!(with_locos.locos[0].label(), "loco 3");
        assert_eq!(with_locos.locos[0].direction(), "reverse");
        assert_eq!(with_locos.locos[0].speed_label(), "speed 40");
        assert!(with_locos.locos[0].function_on(0) && with_locos.locos[0].function_on(1));
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
                locos: Vec::new(),
                register: None,
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
