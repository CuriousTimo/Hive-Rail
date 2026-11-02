//! Simulated operators (`--sim-operators`, wiki "District Simulators and Train Tracking",
//! step 7): the people at a layout, for a bench that runs by itself. Each one drives a train
//! as a person does, through a LocoNet throttle on a node, and sets the turnouts on its way
//! through the nodes of the districts they are in, as station operators do. A train runs
//! back and forth between two blocks: it sets its way, leaves, stops in the far block,
//! waits, turns back and leaves again.
//!
//! An operator sees its own train, as a driver does: from the district simulators' truth
//! (`hive-sim/<session>/world/truth`), which Hive-Rail never reads. It does not watch
//! signals; holding a train before a border that is not clear is the nodes' job, and an
//! operator only notices that its train stands although it drives it.

use crate::district_sim::{Body, BodyWire};
use crate::loconet::{LocoNetMessage, StreamDecoder};
use crate::topology::{Heading, PieceId, Topology};
use std::collections::BTreeMap;
use std::error::Error;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// How long a train may stand while driven before the operator says it waits.
const WAITS_AFTER: Duration = Duration::from_secs(3);
/// How far before the end of its stop block a train halts.
const STOP_MARGIN_MM: f64 = 80.0;

/// One `--train` option: `<loco>@<module/block>,<module/block>[,speed=64][,dwell=10]`.
struct Plan {
    loco: u16,
    stops: [String; 2],
    speed: u8,
    dwell: Duration,
}

fn parse_plan(text: &str) -> Result<Plan, String> {
    let bad = || {
        format!(
            "--train {text}: expected <loco>@<module/block>,<module/block>[,speed=64][,dwell=10]"
        )
    };
    let (loco, rest) = text.split_once('@').ok_or_else(bad)?;
    let mut parts = rest.split(',');
    let (Some(a), Some(b)) = (parts.next(), parts.next()) else {
        return Err(bad());
    };
    let mut plan = Plan {
        loco: loco.parse().map_err(|_| bad())?,
        stops: [a.to_owned(), b.to_owned()],
        speed: 64,
        dwell: Duration::from_secs(10),
    };
    for part in parts {
        match part.split_once('=') {
            Some(("speed", v)) => plan.speed = v.parse().map_err(|_| bad())?,
            Some(("dwell", v)) => plan.dwell = Duration::from_secs(v.parse().map_err(|_| bad())?),
            _ => return Err(bad()),
        }
    }
    Ok(plan)
}

fn log(line: impl std::fmt::Display) {
    println!("{} {line}", chrono::Local::now().format("%H:%M:%S"));
}

/// A LocoNet throttle on a node's LocoNet over TCP port (raw LocoNet).
struct Throttle {
    stream: TcpStream,
    decoder: StreamDecoder,
}

impl Throttle {
    async fn connect(target: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        Ok(Self {
            stream: TcpStream::connect(target).await?,
            decoder: StreamDecoder::default(),
        })
    }

    async fn send(&mut self, message: &LocoNetMessage) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.stream.write_all(message.as_bytes()).await?;
        Ok(())
    }

    /// Asks the node for a loco and puts its slot in use: the slot.
    async fn take(&mut self, loco: u16) -> Result<u8, Box<dyn Error + Send + Sync>> {
        self.send(&LocoNetMessage::loco_address(loco)).await?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut buffer = [0_u8; 512];
        while Instant::now() < deadline {
            let read =
                tokio::time::timeout(Duration::from_millis(200), self.stream.read(&mut buffer))
                    .await;
            let Ok(Ok(count)) = read else { continue };
            for message in self.decoder.push(&buffer[..count]).into_iter().flatten() {
                let bytes = message.as_bytes();
                if bytes.len() == 14
                    && bytes[0] == 0xE7
                    && (u16::from(bytes[4]) | u16::from(bytes[9]) << 7) == loco
                {
                    let slot = bytes[2];
                    self.send(&LocoNetMessage::null_move(slot)).await?;
                    return Ok(slot);
                }
            }
        }
        Err(format!("no slot for loco {loco}").into())
    }

    async fn drive(
        &mut self,
        slot: u8,
        speed: u8,
        forward: bool,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Direction and the lights, then the speed.
        self.send(&LocoNetMessage::loco_dirf(
            slot,
            if forward { 0x10 } else { 0x30 },
        ))
        .await?;
        self.send(&LocoNetMessage::loco_speed(slot, speed)).await
    }

    async fn switch(
        &mut self,
        address: u16,
        closed: bool,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.send(&LocoNetMessage::switch_request(address, closed, true))
            .await?;
        self.send(&LocoNetMessage::switch_request(address, closed, false))
            .await
    }

    /// Reads and drops what the node sends, so its buffer never fills.
    fn drain(&mut self) {
        let mut buffer = [0_u8; 512];
        while let Ok(count) = self.stream.try_read(&mut buffer) {
            if count == 0 {
                break;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    /// Setting the way to the next stop and leaving.
    Leaving,
    Running,
    /// Standing at a stop until then.
    Waiting(Instant),
}

struct Operator {
    plan: Plan,
    /// The district whose node's throttle drives the train (where it started).
    home: String,
    slot: u8,
    /// The stop it runs to: 0 or 1.
    next: usize,
    state: State,
    /// Which way the decoder runs it now.
    forward: bool,
    /// Where it was last seen moving, and since when it stands although driven.
    last_place: Option<(PieceId, i64)>,
    standing_since: Option<Instant>,
    said_waits: bool,
}

/// `--sim-operators <layout.json> --session <name> --throttle <district>=<host:port>…
/// --train <loco>@<block>,<block>[,speed=64][,dwell=10]…`.
pub async fn run(args: Vec<String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut args = args.into_iter();
    let path = args
        .next()
        .ok_or("expected a layout file after --sim-operators")?;
    let path = std::path::Path::new(&path);
    let (mut session, mut throttle_targets, mut plans) = (None, BTreeMap::new(), Vec::new());
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("expected a value after {flag}"))
        };
        match flag.as_str() {
            "--session" => session = Some(value()?),
            "--throttle" => {
                let value = value()?;
                let (district, target) = value
                    .split_once('=')
                    .ok_or("--throttle <district>=<host:port>")?;
                throttle_targets.insert(district.to_owned(), target.to_owned());
            }
            "--train" => plans.push(parse_plan(&value()?)?),
            _ => return Err(format!("unknown --sim-operators option: {flag}").into()),
        }
    }
    let session = session.ok_or("--sim-operators needs --session <name>")?;
    let layout = crate::layout::load(path, path.parent().unwrap_or(std::path::Path::new(".")))?;
    let topology = Topology::build(&layout);
    for plan in &plans {
        for stop in &plan.stops {
            if !topology.blocks.contains_key(stop) {
                return Err(format!("--train {}: no block {stop} in the layout", plan.loco).into());
            }
        }
    }
    let mut throttles = BTreeMap::new();
    for (district, target) in &throttle_targets {
        throttles.insert(district.clone(), Throttle::connect(target).await?);
        log(format!("throttle on {district}'s node at {target}"));
    }

    // The truth, as the district simulators publish it.
    let zenoh = zenoh::open(crate::node::zenoh_config(None, None, false)?).await?;
    let truth = zenoh
        .declare_subscriber(format!("hive-sim/{session}/world/truth/*"))
        .await?;
    let mut bodies: BTreeMap<u16, Body> = BTreeMap::new();
    let mut operators: Vec<Operator> = Vec::new();
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    loop {
        tokio::select! {
            sample = truth.recv_async() => {
                let Ok(sample) = sample else { continue };
                if let Ok(wire) = serde_json::from_slice::<BodyWire>(sample.payload().to_bytes().as_ref()) {
                    bodies.insert(wire.address, Body::from_wire(&wire));
                }
            }
            _ = tick.tick() => {
                for throttle in throttles.values_mut() {
                    throttle.drain();
                }
                // A train's operator starts once its train is seen: it takes the loco on the
                // node of the district it stands in.
                for plan in std::mem::take(&mut plans) {
                    let Some(body) = bodies.get(&plan.loco) else {
                        plans.push(plan);
                        continue;
                    };
                    let home = topology.piece(&body.head.piece).map(|p| p.district.clone()).unwrap_or_default();
                    let Some(throttle) = throttles.get_mut(&home) else {
                        return Err(format!("train {} stands in {home}, but there is no --throttle for it", plan.loco).into());
                    };
                    let slot = throttle.take(plan.loco).await?;
                    log(format!("operator of train {} takes it on {home}'s node, slot {slot}", plan.loco));
                    operators.push(Operator { plan, home, slot, next: 1, state: State::Leaving, forward: true, last_place: None, standing_since: None, said_waits: false });
                }
                for operator in &mut operators {
                    let Some(body) = bodies.get(&operator.plan.loco) else { continue };
                    step(operator, body, &topology, &mut throttles).await?;
                }
            }
            _ = tokio::signal::ctrl_c() => return Ok(()),
        }
    }
}

/// One look at the train and what the operator does about it.
async fn step(
    operator: &mut Operator,
    body: &Body,
    topology: &Topology,
    throttles: &mut BTreeMap<String, Throttle>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let loco = operator.plan.loco;
    let target = operator.plan.stops[operator.next].clone();
    let Some(block) = topology.blocks.get(&target) else {
        return Ok(());
    };
    match operator.state {
        State::Waiting(until) if Instant::now() < until => {}
        State::Waiting(_) | State::Leaving => {
            // The way to the next stop: from the leading end running one way or the other.
            let way = [true, false].into_iter().find_map(|forward| {
                let (piece, heading, _) = body.leading_end(topology, forward)?;
                Some((forward, topology.route(&piece, heading, &block.piece)?))
            });
            let Some((forward, turnouts)) = way else {
                log(format!("operator of train {loco}: no way to {target}"));
                operator.state = State::Waiting(Instant::now() + operator.plan.dwell);
                return Ok(());
            };
            for (turnout, closed) in &turnouts {
                let Some(info) = topology.turnouts().find(|t| &t.key() == turnout) else {
                    continue;
                };
                let (Some(drive), Some(throttle)) =
                    (topology.drive_of(info), throttles.get_mut(&info.district))
                else {
                    continue;
                };
                if drive.bus == "loconet" {
                    throttle.switch(drive.address, *closed).await?;
                }
            }
            operator.forward = forward;
            operator.state = State::Running;
            operator.standing_since = None;
            operator.said_waits = false;
            log(format!(
                "train {loco} leaves for {target} ({} turnouts set)",
                turnouts.len()
            ));
            let home = operator.home.clone();
            if let Some(throttle) = throttles.get_mut(&home) {
                throttle
                    .drive(operator.slot, operator.plan.speed, forward)
                    .await?;
            }
        }
        State::Running => {
            // At the stop: the leading end in the stop block, near its far end.
            let Some((piece, heading, along)) = body.leading_end(topology, operator.forward) else {
                return Ok(());
            };
            let [c0, c1] = block.cuts_mm.map(f64::from);
            let length = topology
                .piece(&piece)
                .map_or(0.0, |p| f64::from(p.length_mm));
            let at = match heading {
                Heading::Forward => along,
                Heading::Backward => length - along,
            };
            let far = match heading {
                Heading::Forward => at >= c1 - STOP_MARGIN_MM,
                Heading::Backward => at <= c0 + STOP_MARGIN_MM,
            };
            if piece == block.piece && at >= c0 && at <= c1 && far {
                let home = operator.home.clone();
                if let Some(throttle) = throttles.get_mut(&home) {
                    throttle.drive(operator.slot, 0, operator.forward).await?;
                }
                log(format!("train {loco} arrived at {target}"));
                operator.next = 1 - operator.next;
                operator.state = State::Waiting(Instant::now() + operator.plan.dwell);
                return Ok(());
            }
            // Standing although driven: held before a border, or blocked.
            let place = (piece.clone(), at as i64);
            if operator.last_place.as_ref() == Some(&place) {
                let since = *operator.standing_since.get_or_insert_with(Instant::now);
                if since.elapsed() > WAITS_AFTER && !operator.said_waits {
                    operator.said_waits = true;
                    log(format!(
                        "train {loco} waits at {piece}, driven but standing"
                    ));
                }
            } else {
                if operator.said_waits {
                    log(format!("train {loco} runs on"));
                }
                operator.standing_since = None;
                operator.said_waits = false;
            }
            operator.last_place = Some(place);
        }
    }
    Ok(())
}
