//! The dispatcher screen (FREMO: Zugleiter): a read-only view of the whole layout for
//! the person in charge of the session. It shows every district, the fast clock and the
//! layout's loco register, and whether every node holds the same register.
//!
//! The dispatcher sends no heartbeat, so it is never the allocator or the clock
//! master and hands out nothing. It says hello on `hive/{session}/observer/hello`; every
//! node answers with its copy of the loco register, and the clock master with the clock.

use crate::{
    clock::FastClock,
    locos::{LocoRegister, RegisteredLoco},
    node::{zenoh_config, Heartbeat, HEARTBEAT_TIMEOUT, REGISTER_SYNC_GRACE},
    rocnet::{ClockAction, ClockUpdate, RocNetFrame},
    ui::{format_age, power_style, ClockChange, TerminalUi, UiCommand},
};
use chrono::Local;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame,
};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    error::Error,
    io::{self, IsTerminal},
    time::Duration,
};
use tokio::time::{self, Instant, MissedTickBehavior};
use zenoh::{handlers::FifoChannelHandler, pubsub::Subscriber, sample::Sample, Session};

/// Events kept; details (every block, sensor and turnout) come many to the minute.
const MAX_EVENTS: usize = 500;
/// How long `--show-locos` waits for the loco register after the first node answers.
const SHOW_LOCOS_SETTLE: Duration = Duration::from_millis(1500);
const SHOW_LOCOS_TIMEOUT: Duration = Duration::from_secs(6);
/// A node behind a firewall, such as Windows, only appears once it connects to a new peer.
const WANTED_NODE_TIMEOUT: Duration = Duration::from_secs(15);

struct Args {
    session: String,
    id: String,
    listen: Option<String>,
    connect: Option<String>,
    plain: bool,
}

fn parse_args(args: Vec<String>) -> Result<Args, Box<dyn Error + Send + Sync>> {
    let mut session = None;
    let mut id = None;
    let mut listen = None;
    let mut connect = None;
    let mut plain = false;
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("missing value after {flag}"))
        };
        match flag.as_str() {
            "--session" => session = Some(value()?),
            "--id" => id = Some(value()?),
            "--listen" => listen = Some(value()?),
            "--connect" => connect = Some(value()?),
            "--plain" => plain = true,
            _ => return Err(format!("unknown dispatcher option: {flag}").into()),
        }
    }
    Ok(Args {
        session: session.ok_or("missing --session")?,
        id: id.unwrap_or_else(|| "dispatcher".to_owned()),
        listen,
        connect,
        plain,
    })
}

/// One district as seen from its heartbeat.
struct District {
    last_seen: Instant,
    online: bool,
    rocnet_id: Option<u16>,
    power: String,
    locos: usize,
    register: Option<u64>,
    /// Since when this node's loco register checksum differs from the dispatcher's copy.
    differs_since: Option<Instant>,
}

impl District {
    fn register_sync(&self, own: u64) -> &'static str {
        match (self.register, self.differs_since) {
            (None, _) => "-",
            (Some(checksum), _) if checksum == own => "in sync",
            (_, Some(since)) if since.elapsed() >= REGISTER_SYNC_GRACE => "DIFFERS",
            _ => "updating",
        }
    }
}

struct State {
    session: String,
    districts: BTreeMap<String, District>,
    register: LocoRegister,
    clock: FastClock,
    /// Each event and whether it is a detail, shown only with `d`.
    events: VecDeque<(bool, String)>,
    echo: bool,
    /// Show the details too (the `d` key).
    details: bool,
}

impl State {
    fn event(&mut self, text: String) {
        self.add_event(false, text);
    }

    fn add_event(&mut self, detail: bool, text: String) {
        let line = format!("{} {text}", Local::now().format("%H:%M:%S"));
        if self.echo && (!detail || self.details) {
            println!("{line}");
        }
        if self.events.len() == MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back((detail, line));
    }

    /// A node's event, marked with its district and RocNet address: `MacBook (101)`. A
    /// node sends plain text; the layout simulator sends `{"detail": true, "text": ...}`
    /// for blocks, sensors and turnouts.
    fn node_event(&mut self, key: &str, payload: &str) {
        let (detail, text) = match serde_json::from_str::<serde_json::Value>(payload) {
            Ok(value) if value.is_object() => (
                value["detail"].as_bool().unwrap_or(false),
                value["text"].as_str().unwrap_or("").to_owned(),
            ),
            _ => (false, payload.to_owned()),
        };
        let text = text.as_str();
        // hive/<session>/node/<id>/event
        let node = key.split('/').nth(3).unwrap_or("?");
        let label = match self
            .districts
            .get(node)
            .and_then(|district| district.rocnet_id)
        {
            Some(rocnet_id) => format!("{node} ({rocnet_id})"),
            None => node.to_owned(),
        };
        self.add_event(detail, format!("{label}  {text}"));
    }

    fn allocator(&self) -> Option<&str> {
        self.districts
            .iter()
            .find(|(_, district)| district.online)
            .map(|(id, _)| id.as_str())
    }

    /// Returns whether the node is new or came back online.
    fn heartbeat(&mut self, heartbeat: Heartbeat) -> bool {
        let own = self.register.checksum();
        let differs = heartbeat.register.is_some_and(|checksum| checksum != own);
        let previous = self.districts.get(&heartbeat.node).map(|district| {
            (
                district.online,
                district.power.clone(),
                district.differs_since,
            )
        });
        let differs_since = match (differs, previous.as_ref()) {
            (false, _) => None,
            (true, Some((_, _, Some(since)))) => Some(*since),
            (true, _) => Some(Instant::now()),
        };
        self.districts.insert(
            heartbeat.node.clone(),
            District {
                last_seen: Instant::now(),
                online: true,
                rocnet_id: heartbeat.rocnet_id,
                power: heartbeat.power.clone(),
                locos: heartbeat.locos.len(),
                register: heartbeat.register,
                differs_since,
            },
        );
        match previous {
            Some((true, power, _)) => {
                if power != heartbeat.power {
                    self.event(format!(
                        "{} district power {power} -> {}",
                        heartbeat.node, heartbeat.power
                    ));
                }
                false
            }
            _ => {
                self.event(format!(
                    "{} online (RocNet {}, power {})",
                    heartbeat.node,
                    heartbeat
                        .rocnet_id
                        .map_or_else(|| "?".to_owned(), |id| id.to_string()),
                    heartbeat.power
                ));
                true
            }
        }
    }

    fn refresh(&mut self) {
        let now = Instant::now();
        let stale: Vec<String> = self
            .districts
            .iter_mut()
            .filter(|(_, district)| {
                district.online && now.duration_since(district.last_seen) >= HEARTBEAT_TIMEOUT
            })
            .map(|(id, district)| {
                district.online = false;
                id.clone()
            })
            .collect();
        for id in stale {
            self.event(format!("{id} stale: no heartbeat"));
        }
    }

    /// Merges loco updates into the dispatcher's copy of the register. Only real changes
    /// become events: a loco in another district as an event, any other change as a
    /// detail. The copies every node sends of the same register are not repeated.
    fn loco_updates(&mut self, locos: Vec<RegisteredLoco>, origin: &str) {
        let merged = self.register.merge(locos);
        for (before, after) in merged.changes {
            let moved = before.as_ref().map(|b| &b.district) != Some(&after.district);
            match (&after.district, moved) {
                (Some(district), true) => self.event(format!(
                    "loco {} now in district {district}  (from {origin})",
                    after.address
                )),
                _ => self.add_event(true, format!("{}  from {origin}", after.describe())),
            }
        }
    }

    fn clock_frame(&mut self, frame: &RocNetFrame, origin: &str) {
        let was = self.clock.label(Instant::now());
        if apply_clock(&mut self.clock, frame) == Some(ClockAction::Set) {
            let now = self.clock.label(Instant::now());
            self.event(format!("fast clock set by {origin}: {was} -> {now}"));
        }
    }
}

/// Follows a clock set or sync from the mesh; returns which it was.
fn apply_clock(clock: &mut FastClock, frame: &RocNetFrame) -> Option<ClockAction> {
    let update = frame.clock_update()?;
    let rate = if update.divider == 0 {
        clock.rate()
    } else {
        update.divider
    };
    clock.set(
        u16::from(update.hour) * 60 + u16::from(update.minute),
        rate,
        update.divider != 0,
        Instant::now(),
    );
    Some(update.action)
}

struct Mesh {
    _session: Session,
    heartbeats: Subscriber<FifoChannelHandler<Sample>>,
    locos: Subscriber<FifoChannelHandler<Sample>>,
    clock: Subscriber<FifoChannelHandler<Sample>>,
    /// The nodes' events (`hive/<session>/node/<id>/event`).
    events: Subscriber<FifoChannelHandler<Sample>>,
    hello: zenoh::pubsub::Publisher<'static>,
    updates: zenoh::pubsub::Publisher<'static>,
    /// Clock sets from the dispatcher's keys, as a node's dashboard sends them.
    clock_set: zenoh::pubsub::Publisher<'static>,
    id: String,
}

impl Mesh {
    async fn open(args: &Args) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let session = zenoh::open(zenoh_config(
            args.listen.as_deref(),
            args.connect.as_deref(),
        )?)
        .await?;
        let s = &args.session;
        Ok(Self {
            heartbeats: session
                .declare_subscriber(format!("hive/{s}/node/*/hb"))
                .await?,
            locos: session
                .declare_subscriber(format!("hive/{s}/locos/update"))
                .await?,
            clock: session
                .declare_subscriber(format!("hive/{s}/rn/ck"))
                .await?,
            events: session
                .declare_subscriber(format!("hive/{s}/node/*/event"))
                .await?,
            hello: session
                .declare_publisher(format!("hive/{s}/observer/hello"))
                .await?,
            updates: session
                .declare_publisher(format!("hive/{s}/locos/update"))
                .await?,
            clock_set: session
                .declare_publisher(format!("hive/{s}/rn/ck"))
                .allowed_destination(zenoh::sample::Locality::Remote)
                .await?,
            _session: session,
            id: args.id.clone(),
        })
    }

    /// Sets the layout's fast clock: every node takes it, and the clock master keeps it.
    async fn set_clock(&self, clock: &FastClock) -> Result<(), Box<dyn Error + Send + Sync>> {
        let minute = clock.minute_of_day(Instant::now());
        // The dispatcher has no RocNet address of its own; 0 is the sender.
        let frame = RocNetFrame::clock(
            0,
            ClockUpdate {
                action: ClockAction::Set,
                hour: (minute / 60) as u8,
                minute: (minute % 60) as u8,
                divider: if clock.running() { clock.rate() } else { 0 },
            },
        );
        self.clock_set
            .put(frame.as_bytes().to_vec())
            .attachment(self.id.clone())
            .await?;
        Ok(())
    }

    /// Asks the allocator for the loco register and the clock.
    async fn say_hello(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.hello
            .put(Vec::new())
            .attachment(self.id.clone())
            .await?;
        Ok(())
    }
}

fn origin(sample: &Sample) -> String {
    sample
        .attachment()
        .and_then(|attachment| attachment.try_to_string().ok())
        .map_or_else(|| "unknown".to_owned(), |origin| origin.into_owned())
}

/// `--mode dispatcher`: the monitor screen, or a log with `--plain`.
pub async fn run(args: Vec<String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let args = parse_args(args)?;
    let interactive = !args.plain && io::stdin().is_terminal() && io::stdout().is_terminal();
    let mesh = Mesh::open(&args).await?;
    let mut tui = if interactive {
        Some(TerminalUi::new()?)
    } else {
        None
    };
    let mut state = State {
        session: args.session.clone(),
        districts: BTreeMap::new(),
        register: LocoRegister::new(&args.id),
        clock: FastClock::new(Instant::now()),
        events: VecDeque::new(),
        echo: tui.is_none(),
        details: false,
    };
    state.event(format!(
        "dispatcher {} watching session {}",
        args.id, args.session
    ));
    mesh.say_hello().await?;

    let mut tick = time::interval(Duration::from_millis(200));
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            sample = mesh.heartbeats.recv_async() => {
                let sample = sample?;
                if let Ok(heartbeat) = serde_json::from_slice::<Heartbeat>(sample.payload().to_bytes().as_ref()) {
                    // A node that just came online may be the new allocator.
                    if state.heartbeat(heartbeat) {
                        mesh.say_hello().await?;
                    }
                }
            }
            sample = mesh.locos.recv_async() => {
                let sample = sample?;
                let origin = origin(&sample);
                match serde_json::from_slice::<Vec<RegisteredLoco>>(sample.payload().to_bytes().as_ref()) {
                    Ok(locos) => state.loco_updates(locos, &origin),
                    Err(error) => state.event(format!("ignored invalid loco update from {origin}: {error}")),
                }
            }
            sample = mesh.events.recv_async() => {
                let sample = sample?;
                let text = String::from_utf8_lossy(sample.payload().to_bytes().as_ref()).into_owned();
                state.node_event(sample.key_expr().as_str(), &text);
            }
            sample = mesh.clock.recv_async() => {
                let sample = sample?;
                let origin = origin(&sample);
                if let Ok(frame) = RocNetFrame::from_bytes(sample.payload().to_bytes().as_ref()) {
                    state.clock_frame(&frame, &origin);
                }
            }
            _ = tick.tick() => {
                state.refresh();
                if let Some(ui) = tui.as_mut() {
                    match ui.handle_input()? {
                        UiCommand::Quit => break,
                        UiCommand::ToggleDetails => state.details = !state.details,
                        UiCommand::Clock(change) => {
                            let now = Instant::now();
                            let was = state.clock.label(now);
                            match change {
                                ClockChange::ToggleRunning => state.clock.toggle_running(now),
                                ClockChange::Rate(delta) => state.clock.change_rate(delta, now),
                                ClockChange::Hours(delta) => state.clock.shift_hours(delta, now),
                            }
                            let label = state.clock.label(now);
                            state.event(format!("fast clock set here: {was} -> {label}"));
                            mesh.set_clock(&state.clock).await?;
                        }
                        _ => {}
                    }
                    ui.draw_with(|frame| render(frame, &state))?;
                }
            }
        }
    }
    Ok(())
}

/// `--show-locos`: prints the layout's loco register once and exits.
pub async fn show_locos(args: Vec<String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut args = parse_args(args)?;
    args.plain = true;
    let mesh = Mesh::open(&args).await?;
    let state = gather(&mesh, &args, None).await?;
    if state.districts.is_empty() {
        println!(
            "No node answered in session {} within {} s.",
            args.session,
            SHOW_LOCOS_TIMEOUT.as_secs()
        );
        return Ok(());
    }
    print_register(&args, &state);
    Ok(())
}

/// Joins the mesh as an observer for a few seconds and collects the districts and the
/// loco register.
/// Collects the nodes and the loco register; with `want`, keeps listening until that node
/// is seen too, since a new peer finds the nodes on other machines later than local ones.
async fn gather(
    mesh: &Mesh,
    args: &Args,
    want: Option<&str>,
) -> Result<State, Box<dyn Error + Send + Sync>> {
    let mut state = State {
        session: args.session.clone(),
        districts: BTreeMap::new(),
        register: LocoRegister::new(&args.id),
        clock: FastClock::new(Instant::now()),
        events: VecDeque::new(),
        echo: false,
        details: false,
    };
    let deadline = Instant::now() + want.map_or(SHOW_LOCOS_TIMEOUT, |_| WANTED_NODE_TIMEOUT);
    let mut settle_until: Option<Instant> = None;
    // Hello once the first node is seen, so the allocator is reachable.
    loop {
        let wait_until = settle_until.unwrap_or(deadline).min(deadline);
        tokio::select! {
            _ = time::sleep_until(wait_until) => {
                let missing = want.is_some_and(|node| !state.districts.contains_key(node));
                if !missing || Instant::now() >= deadline {
                    break;
                }
                settle_until = Some(deadline);
            }
            sample = mesh.heartbeats.recv_async() => {
                let sample = sample?;
                if let Ok(heartbeat) = serde_json::from_slice::<Heartbeat>(sample.payload().to_bytes().as_ref()) {
                    if state.heartbeat(heartbeat) && settle_until.is_none() {
                        // Give scouting a moment to find every node before asking.
                        time::sleep(Duration::from_millis(500)).await;
                        mesh.say_hello().await?;
                        settle_until = Some(Instant::now() + SHOW_LOCOS_SETTLE);
                    }
                }
            }
            sample = mesh.locos.recv_async() => {
                let sample = sample?;
                if let Ok(locos) = serde_json::from_slice::<Vec<RegisteredLoco>>(sample.payload().to_bytes().as_ref()) {
                    state.register.merge(locos);
                }
            }
            sample = mesh.clock.recv_async() => {
                let sample = sample?;
                if let Ok(frame) = RocNetFrame::from_bytes(sample.payload().to_bytes().as_ref()) {
                    apply_clock(&mut state.clock, &frame);
                }
            }
        }
    }
    Ok(state)
}

fn print_register(args: &Args, state: &State) {
    println!(
        "Session {}: {} node(s), allocator {}",
        args.session,
        state.districts.len(),
        state.allocator().unwrap_or("-")
    );
    let entries = registered(&state.register);
    if entries.is_empty() {
        println!("No locos registered.");
    } else {
        println!(
            "{:<7} {:>8}  {:<9} {:<15} {:<8} {:<6} {:<12} {:>4}",
            "Loco", "Throttle", "Status", "Speed", "Dir", "F0-F4", "Driven at", "Ver"
        );
        for entry in entries {
            let loco = entry.loco();
            println!(
                "{:<7} {:>8}  {:<9} {:<15} {:<8} {:<6} {:<12} {:>4}",
                entry.address,
                entry
                    .throttle
                    .map_or_else(|| "-".to_owned(), |throttle| throttle.to_string()),
                entry.status_label(),
                loco.speed_label(),
                loco.direction(),
                functions(&loco),
                entry.node,
                entry.version
            );
        }
    }
    let own = state.register.checksum();
    for (id, district) in &state.districts {
        if district.register.is_some_and(|checksum| checksum != own) {
            println!("Note: {id}'s loco register differs from the copy shown here.");
        }
    }
}

/// `--transfer-loco <address> --to <node>`: a test tool that puts a loco into a district,
/// as a handoff will. The loco gets that node as its district in the register; the node
/// then takes it on its command station's slot master. `--speed`, `--forward`/`--reverse`
/// and `--lights`/`--no-lights` set its state; what is not given stays as it is, so running
/// it again changes only the speed.
/// Puts locos into the layout's loco register from a program that is not a node, such as
/// the layout simulator: which district a loco is in, its speed and direction. The nodes
/// then take it on, or release it from, their slot masters as for a real handoff.
pub struct LocoPublisher {
    mesh: Mesh,
    id: String,
    session: String,
    locos: HashMap<u16, RegisteredLoco>,
    /// The layout's fast clock, as the nodes keep it.
    pub clock: FastClock,
}

impl LocoPublisher {
    pub async fn open(session: &str, id: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let options = parse_args(vec![
            "--session".to_owned(),
            session.to_owned(),
            "--id".to_owned(),
            id.to_owned(),
            "--plain".to_owned(),
        ])?;
        let mesh = Mesh::open(&options).await?;
        // The register as the nodes have it, so the versions published here are newer.
        let state = gather(&mesh, &options, None).await?;
        let locos = state
            .register
            .snapshot()
            .into_iter()
            .map(|loco| (loco.address, loco))
            .collect();
        Ok(Self {
            mesh,
            id: id.to_owned(),
            session: session.to_owned(),
            locos,
            clock: state.clock,
        })
    }

    /// Waits for the next clock set or sync on the mesh and follows it.
    pub async fn follow_clock(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let sample = self.mesh.clock.recv_async().await?;
        if let Ok(frame) = RocNetFrame::from_bytes(sample.payload().to_bytes().as_ref()) {
            apply_clock(&mut self.clock, &frame);
        }
        Ok(())
    }

    /// An event in a district, as that district's node reports it, for the dispatcher's
    /// list; `detail` events show only with its `d` key.
    pub async fn event(
        &self,
        district: &str,
        detail: bool,
        text: &str,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let payload = serde_json::json!({ "detail": detail, "text": text }).to_string();
        self.mesh
            ._session
            .put(
                format!("hive/{}/node/{district}/event", self.session),
                payload,
            )
            .await?;
        Ok(())
    }

    /// Publishes a loco: its district, a speed step (0 to 126), its direction and lights.
    pub async fn put(
        &mut self,
        address: u16,
        district: &str,
        step: u8,
        forward: bool,
        lights: bool,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut loco = self
            .locos
            .get(&address)
            .cloned()
            .unwrap_or_else(|| RegisteredLoco::new(address, &self.id));
        loco.district = Some(district.to_owned());
        loco.status |= 0x30; // in use
                             // LocoNet speed: 0 stop, 1 emergency stop, 2 to 127 for speed 1 to 126.
        loco.speed = if step == 0 { 0 } else { step.min(126) + 1 };
        loco.dirf = if forward { 0 } else { 0x20 } | if lights { 0x10 } else { 0 };
        loco.version += 1;
        loco.node = self.id.clone();
        self.locos.insert(address, loco.clone());
        self.mesh
            .updates
            .put(serde_json::to_vec(&vec![loco])?)
            .attachment(self.id.clone())
            .await?;
        Ok(())
    }
}

pub async fn transfer_loco(args: Vec<String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut args = args.into_iter();
    let address: u16 = args
        .next()
        .and_then(|value| value.parse().ok())
        // DCC long addresses end at 10239; most throttles stop at 9999.
        .filter(|address| (1..=10239).contains(address))
        .ok_or("expected a loco address from 1 to 10239 after --transfer-loco")?;
    let mut to = None;
    let mut speed: Option<u8> = None;
    let mut reverse: Option<bool> = None;
    let mut lights: Option<bool> = None;
    let mut rest = vec!["--id".to_owned(), "transfer".to_owned()];
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("missing value after {flag}"))
        };
        match flag.as_str() {
            "--to" => to = Some(value()?),
            "--speed" => {
                let value = value()?;
                let step: u8 = value
                    .parse()
                    .ok()
                    .filter(|step| *step <= 126)
                    .ok_or_else(|| format!("invalid speed '{value}'; expected 0 to 126"))?;
                // LocoNet speed: 0 stop, 1 emergency stop, 2 to 127 for speed 1 to 126.
                speed = Some(if step == 0 { 0 } else { step + 1 });
            }
            "--forward" => reverse = Some(false),
            "--reverse" => reverse = Some(true),
            "--lights" => lights = Some(true),
            "--no-lights" => lights = Some(false),
            _ => {
                rest.push(flag.clone());
                if let Some(value) = args.next() {
                    rest.push(value);
                }
            }
        }
    }
    let to = to.ok_or("missing --to <node>")?;
    let mut options = parse_args(rest)?;
    options.plain = true;
    let mesh = Mesh::open(&options).await?;
    let state = gather(&mesh, &options, Some(&to)).await?;
    if !state.districts.contains_key(&to) {
        return Err(format!("node {to} is not online in session {}", options.session).into());
    }

    let mut loco = state
        .register
        .get(address)
        .cloned()
        .unwrap_or_else(|| RegisteredLoco::new(address, "virtual"));
    loco.district = Some(to.clone());
    loco.status |= 0x30; // in use
    if let Some(speed) = speed {
        loco.speed = speed;
    }
    // DIRF: bit 5 reverse, bit 4 F0 (the lights).
    if let Some(reverse) = reverse {
        loco.dirf = (loco.dirf & !0x20) | if reverse { 0x20 } else { 0 };
    }
    if let Some(lights) = lights {
        loco.dirf = (loco.dirf & !0x10) | if lights { 0x10 } else { 0 };
    }
    loco.version += 1;
    mesh.updates
        .put(serde_json::to_vec(&vec![loco.clone()])?)
        .attachment(options.id.clone())
        .await?;
    // Give Zenoh a moment to deliver before the session closes.
    time::sleep(Duration::from_millis(500)).await;
    println!("{}", loco.describe());
    Ok(())
}

/// Every registered loco except the released ones, by address.
fn registered(register: &LocoRegister) -> Vec<RegisteredLoco> {
    register
        .snapshot()
        .into_iter()
        .filter(|loco| loco.status_label() != "released")
        .collect()
}

fn functions(loco: &crate::locos::LocoInfo) -> String {
    (0..=4)
        .map(|number| {
            if loco.function_on(number) {
                char::from(b'0' + number)
            } else {
                '-'
            }
        })
        .collect()
}

fn render(frame: &mut Frame<'_>, state: &State) {
    let own = state.register.checksum();
    let entries = registered(&state.register);
    let differing: Vec<&str> = state
        .districts
        .iter()
        .filter(|(_, district)| district.online && district.register_sync(own) == "DIFFERS")
        .map(|(id, _)| id.as_str())
        .collect();
    let online = state.districts.values().filter(|d| d.online).count();

    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(state.districts.len().clamp(1, 12) as u16 + 3),
            Constraint::Min(6),
            // The events of every district: a dozen lines.
            Constraint::Length(14),
        ])
        .split(frame.area());

    let sync = if differing.is_empty() {
        Span::styled(
            format!("loco register in sync on {online} node(s)"),
            Style::default().fg(Color::Green),
        )
    } else {
        Span::styled(
            format!("LOCO REGISTER DIFFERS on {}", differing.join(", ")),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    };
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                " HIVE-RAIL DISPATCHER ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("  Session: {}", state.session)),
            Span::styled(
                format!("   FAST CLOCK {}", state.clock.label(Instant::now())),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("   wall {}", Local::now().format("%H:%M:%S")),
                Style::default().fg(Color::Gray),
            ),
        ]),
        Line::from(vec![
            Span::raw(format!(
                "Allocator and clock master: {} ★   ",
                state.allocator().unwrap_or("none online")
            )),
            sync,
            Span::raw(format!("   {} loco(s) registered", entries.len())),
        ]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Zugleiter / dispatcher"),
    );
    frame.render_widget(header, areas[0]);

    let allocator = state.allocator();
    let districts = Table::new(
        state.districts.iter().map(|(id, district)| {
            let (heartbeat, color) = if district.online {
                ("ONLINE", Color::Green)
            } else {
                ("STALE / STOP", Color::Red)
            };
            let sync = district.register_sync(own);
            let sync_style = match sync {
                "in sync" => Style::default().fg(Color::Green),
                "DIFFERS" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                _ => Style::default().fg(Color::Gray),
            };
            Row::new(vec![
                Cell::from(format!(
                    "{id}{}",
                    if Some(id.as_str()) == allocator {
                        " ★"
                    } else {
                        ""
                    }
                )),
                Cell::from(
                    district
                        .rocnet_id
                        .map_or_else(|| "-".to_owned(), |id| id.to_string()),
                ),
                Cell::from(Span::styled(
                    heartbeat,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )),
                Cell::from(format_age(district.last_seen.elapsed())),
                Cell::from(Span::styled(
                    district.power.clone(),
                    power_style(&district.power),
                )),
                Cell::from(district.locos.to_string()),
                Cell::from(Span::styled(sync, sync_style)),
            ])
        }),
        [
            Constraint::Percentage(22),
            Constraint::Percentage(10),
            Constraint::Percentage(16),
            Constraint::Percentage(14),
            Constraint::Percentage(12),
            Constraint::Percentage(10),
            Constraint::Percentage(16),
        ],
    )
    .header(
        Row::new([
            "District",
            "RocNet",
            "Heartbeat",
            "Last seen",
            "Power",
            "Locos",
            "Register",
        ])
        .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(Block::default().borders(Borders::ALL).title("Districts"));
    frame.render_widget(districts, areas[1]);

    let slots = Table::new(
        entries.iter().map(|entry| {
            let loco = entry.loco();
            let speed_color = match loco.speed {
                0 => Color::Gray,
                1 => Color::Red,
                _ => Color::Green,
            };
            Row::new(vec![
                Cell::from(entry.address.to_string()),
                Cell::from(
                    entry
                        .throttle
                        .map_or_else(|| "-".to_owned(), |throttle| throttle.to_string()),
                ),
                Cell::from(entry.status_label()),
                Cell::from(Span::styled(
                    loco.speed_label(),
                    Style::default().fg(speed_color),
                )),
                Cell::from(format!(
                    "{} {}",
                    if loco.forward { "▶" } else { "◀" },
                    loco.direction()
                )),
                Cell::from(functions(&loco)),
                Cell::from(entry.district.clone().unwrap_or_else(|| "-".to_owned())),
                Cell::from(entry.node.clone()),
                Cell::from(entry.version.to_string()),
            ])
        }),
        [
            Constraint::Length(7),
            Constraint::Length(9),
            Constraint::Length(10),
            Constraint::Length(16),
            Constraint::Length(10),
            Constraint::Length(7),
            Constraint::Min(12),
            Constraint::Min(12),
            Constraint::Length(6),
        ],
    )
    .header(
        Row::new([
            "Loco",
            "Throttle",
            "Status",
            "Speed",
            "Direction",
            "F0-F4",
            "District",
            "Driven at",
            "Ver",
        ])
        .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Loco register (whole layout)"),
    );
    frame.render_widget(slots, areas[2]);

    let visible = usize::from(areas[3].height.saturating_sub(2));
    let shown: Vec<&String> = state
        .events
        .iter()
        .filter(|(detail, _)| state.details || !detail)
        .map(|(_, line)| line)
        .collect();
    let events: Vec<Line> = shown
        .iter()
        .skip(shown.len().saturating_sub(visible))
        .map(|line| Line::from(line.as_str()))
        .collect();
    let title = if state.details {
        "Events of every district, all details (d: fewer, c: clock run/pause, + -: rate, [ ]: hour, q: quit)"
    } else {
        "Events of every district (d: all details, c: clock run/pause, + -: rate, [ ]: hour, q: quit)"
    };
    frame.render_widget(
        Paragraph::new(events).block(Block::default().borders(Borders::ALL).title(title)),
        areas[3],
    );
}
