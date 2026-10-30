//! Trains running on a layout's track graph, without hardware: the occupancy reports,
//! sensor passings, turnout settings and level crossings they cause come out as events
//! with the bus addresses of the devices, so they can enter Hive-Rail the way real
//! LocoNet reports do.
//!
//! A train has a head position, a length and a speed; its tail follows the path behind
//! it, so a block is occupied by the whole train. It runs to a destination block, setting
//! the turnouts on its way as a route would (facing turnouts towards the destination,
//! trailing ones to the leg it comes from). At the destination it stops, waits, reverses
//! and takes the next destination of its plan. It waits while the next piece of track is
//! occupied by another train.

use crate::layout::{Layout, ModuleFile};
use crate::topology::{Heading, Leg, PieceId, Topology};
use std::collections::{BTreeMap, BTreeSet};

/// How far before the end of its destination piece a train stops, in mm.
const STOP_MARGIN_MM: f64 = 40.0;

/// A place on the track: a piece, the heading along it, and how far along it in that
/// heading (0 at the end the train entered by).
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub piece: PieceId,
    pub heading: Heading,
    pub along_mm: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrainState {
    Running,
    /// Waiting for the track ahead to clear.
    Waiting,
    /// Standing at a destination until this time, then reversing.
    Dwelling {
        until_s: f64,
    },
}

#[derive(Debug, Clone)]
pub struct Train {
    pub name: String,
    pub address: u16,
    pub length_mm: f64,
    pub speed_mm_s: f64,
    pub head: Place,
    /// The pieces behind the head, oldest first; the head's own piece is not in it.
    trail: Vec<(PieceId, Heading)>,
    pub state: TrainState,
    /// Destination blocks (`module/block`), visited in turn.
    pub plan: Vec<String>,
    next_stop: usize,
    pub dwell_s: f64,
}

/// What happened in the simulation, with the time in seconds.
#[derive(Debug, Clone, PartialEq)]
pub enum SimEvent {
    /// A block became occupied or free. `address`: its occupancy detector, on LocoNet.
    Block {
        block: String,
        district: String,
        occupied: bool,
        address: Option<u16>,
    },
    /// The head of a train passed a sensor.
    Sensor {
        device: String,
        district: String,
        address: u16,
        train: String,
    },
    /// A turnout was set (closed: straight).
    Turnout {
        turnout: String,
        district: String,
        closed: bool,
        address: Option<u16>,
    },
    /// A level crossing closed (lights on, barriers down) or opened.
    Crossing {
        crossing: String,
        name: String,
        closed: bool,
    },
    /// A train's head moved into another district.
    Border {
        train: String,
        from: String,
        to: String,
    },
    /// A train stopped at its destination.
    Arrived { train: String, block: String },
    /// A train reversed and left for its next destination.
    Departed { train: String, to: String },
}

pub struct LayoutSim<'a> {
    layout: &'a Layout,
    topology: &'a Topology,
    pub trains: Vec<Train>,
    /// Turnout positions by `module/turnout`: closed is straight.
    pub turnouts: BTreeMap<String, bool>,
    occupied: BTreeSet<String>,
    /// The trains between each level crossing's closing and opening sensors: the
    /// crossing stays closed until the last of them has passed.
    crossing_trains: BTreeMap<String, BTreeSet<String>>,
    pub time_s: f64,
}

impl<'a> LayoutSim<'a> {
    pub fn new(layout: &'a Layout, topology: &'a Topology) -> Self {
        let turnouts = topology
            .turnouts()
            .map(|turnout| (format!("{}/{}", turnout.module, turnout.id), true))
            .collect();
        Self {
            layout,
            topology,
            trains: Vec::new(),
            turnouts,
            occupied: BTreeSet::new(),
            crossing_trains: BTreeMap::new(),
            time_s: 0.0,
        }
    }

    /// Puts a train with its head on `head`; its tail lies behind it on the same piece.
    pub fn add_train(
        &mut self,
        name: &str,
        address: u16,
        length_mm: f64,
        head: Place,
        plan: &[&str],
    ) {
        self.trains.push(Train {
            name: name.to_owned(),
            address,
            length_mm,
            speed_mm_s: 150.0,
            head,
            trail: Vec::new(),
            state: TrainState::Running,
            plan: plan.iter().map(|stop| (*stop).to_owned()).collect(),
            next_stop: 0,
            dwell_s: 10.0,
        });
    }

    fn module(&self, id: &str) -> Option<&ModuleFile> {
        self.layout.modules.get(id).map(|(module, _)| module)
    }

    fn piece_length(&self, piece: &PieceId) -> f64 {
        self.topology
            .piece(piece)
            .map_or(0.0, |info| f64::from(info.length_mm))
    }

    fn closed(&self, module: &str, turnout: &str) -> Option<bool> {
        self.turnouts.get(&format!("{module}/{turnout}")).copied()
    }

    /// The blocks occupied now (`module/block`).
    pub fn occupied(&self) -> &BTreeSet<String> {
        &self.occupied
    }

    /// The level crossings closed now (`module/crossing`).
    pub fn closed_crossings(&self) -> impl Iterator<Item = &String> {
        self.crossing_trains
            .iter()
            .filter(|(_, trains)| !trains.is_empty())
            .map(|(key, _)| key)
    }

    /// The stretches of track a train covers, head first, per piece, in mm from the
    /// piece's `from` point.
    pub fn covered(&self, train: &Train) -> Vec<(PieceId, f64, f64)> {
        self.covered_with_heading(train)
            .into_iter()
            .map(|(piece, _, a, b)| (piece, a, b))
            .collect()
    }

    /// As `covered`, with the heading the train runs along each piece.
    pub fn covered_with_heading(&self, train: &Train) -> Vec<(PieceId, Heading, f64, f64)> {
        let mut spans = Vec::new();
        let mut remaining = train.length_mm;
        let mut place = (
            train.head.piece.clone(),
            train.head.heading,
            train.head.along_mm,
        );
        let mut trail = train.trail.iter().rev();
        loop {
            let length = self.piece_length(&place.0);
            let start = (place.2 - remaining).max(0.0);
            let (a, b) = match place.1 {
                Heading::Forward => (start, place.2),
                Heading::Backward => (length - place.2, length - start),
            };
            spans.push((place.0.clone(), place.1, a, b));
            remaining -= place.2 - start;
            if remaining <= 0.0 {
                break;
            }
            let Some((piece, heading)) = trail.next() else {
                break;
            };
            let length = self.piece_length(piece);
            place = (piece.clone(), *heading, length);
        }
        spans
    }

    /// The blocks occupied now, by any train.
    fn occupancy(&self) -> BTreeSet<String> {
        let mut occupied = BTreeSet::new();
        for train in &self.trains {
            for (piece, a, b) in self.covered(train) {
                for (key, block) in &self.topology.blocks {
                    let [c0, c1] = block.cuts_mm;
                    if block.piece == piece && a < f64::from(c1) && b > f64::from(c0) {
                        occupied.insert(key.clone());
                    }
                }
            }
        }
        occupied
    }

    /// Whether another train covers any part of this piece.
    fn piece_taken(&self, piece: &PieceId, by: usize) -> bool {
        self.trains
            .iter()
            .enumerate()
            .any(|(i, train)| i != by && self.covered(train).iter().any(|(p, _, _)| p == piece))
    }

    /// The piece a train's destination block lies on.
    fn destination(&self, train: &Train) -> Option<PieceId> {
        let stop = train.plan.get(train.next_stop)?;
        self.topology
            .blocks
            .get(stop)
            .map(|block| block.piece.clone())
    }

    /// Where the train goes after its head piece: through a turnout ahead the leg towards
    /// its destination (and the turnout set so), otherwise the only way on.
    fn choose_next(
        &mut self,
        index: usize,
        events: &mut Vec<(f64, SimEvent)>,
    ) -> Option<(PieceId, Heading)> {
        let train = &self.trains[index];
        let (piece, heading) = (train.head.piece.clone(), train.head.heading);
        let destination = self.destination(train);
        if let Some(ahead) = self.topology.turnout_ahead(&piece, heading) {
            let choice = match ahead.entry {
                // Facing: the leg from which the destination can be reached.
                Leg::Trunk => ahead
                    .options
                    .iter()
                    .find(|(_, next, heading)| {
                        destination.as_ref().is_some_and(|goal| {
                            self.topology
                                .reachable(next, *heading)
                                .iter()
                                .any(|(p, _)| p == goal)
                        })
                    })
                    .or(ahead.options.first())
                    .cloned(),
                // Trailing: out by the trunk, the turnout set to the leg we come from.
                entry => ahead
                    .options
                    .first()
                    .map(|(_, next, h)| (entry, next.clone(), *h)),
            }?;
            let closed = choice.0 == Leg::Straight
                || (choice.0 == Leg::Trunk && ahead.entry == Leg::Straight);
            let key = format!("{}/{}", ahead.module, ahead.id);
            if self.turnouts.get(&key) != Some(&closed) {
                self.turnouts.insert(key.clone(), closed);
                events.push((
                    self.time_s,
                    self.turnout_event(&ahead.module, &ahead.id, closed),
                ));
            }
            return Some((choice.1, choice.2));
        }
        self.topology
            .next(&piece, heading, |module, turnout| {
                self.closed(module, turnout)
            })
            .into_iter()
            .next()
    }

    fn turnout_event(&self, module: &str, turnout: &str, closed: bool) -> SimEvent {
        let module_file = self.module(module);
        let device = module_file
            .and_then(|m| m.turnouts.iter().find(|t| t.id == turnout))
            .and_then(|t| t.device.clone())
            .and_then(|d| module_file?.devices.iter().find(|x| x.id == d).cloned());
        SimEvent::Turnout {
            turnout: format!("{module}/{turnout}"),
            district: self.layout.district_of(module).unwrap_or("").to_owned(),
            closed,
            address: device.map(|d| d.address),
        }
    }

    /// Advances the simulation by `dt_s` seconds and returns what happened.
    pub fn step(&mut self, dt_s: f64) -> Vec<(f64, SimEvent)> {
        let mut events = Vec::new();
        self.time_s += dt_s;
        for index in 0..self.trains.len() {
            self.step_train(index, dt_s, &mut events);
        }
        // Occupancy changes.
        let occupied = self.occupancy();
        for block in occupied.symmetric_difference(&self.occupied) {
            let info = &self.topology.blocks[block];
            let address = self
                .module(&info.module)
                .and_then(|m| m.blocks.iter().find(|b| b.id == info.id))
                .and_then(|b| b.occupancy.clone())
                .and_then(|d| {
                    self.module(&info.module)?
                        .devices
                        .iter()
                        .find(|x| x.id == d)
                        .map(|x| x.address)
                });
            events.push((
                self.time_s,
                SimEvent::Block {
                    block: block.clone(),
                    district: info.district.clone(),
                    occupied: occupied.contains(block),
                    address,
                },
            ));
        }
        self.occupied = occupied;
        events
    }

    fn step_train(&mut self, index: usize, dt_s: f64, events: &mut Vec<(f64, SimEvent)>) {
        match self.trains[index].state {
            TrainState::Dwelling { until_s } => {
                if self.time_s >= until_s {
                    self.reverse(index, events);
                }
                return;
            }
            TrainState::Waiting | TrainState::Running => {}
        }
        let start = self.trains[index].head.clone();
        let mut travel = self.trains[index].speed_mm_s * dt_s;
        loop {
            let train = &self.trains[index];
            let length = self.piece_length(&train.head.piece);
            // Stop at the destination, near the end of its piece.
            if self.destination(train).as_ref() == Some(&train.head.piece) {
                let stop_at = (length - STOP_MARGIN_MM).max(0.0);
                if train.head.along_mm + travel >= stop_at {
                    let train = &mut self.trains[index];
                    train.head.along_mm = stop_at.max(train.head.along_mm);
                    train.state = TrainState::Dwelling {
                        until_s: self.time_s + train.dwell_s,
                    };
                    let block = train.plan[train.next_stop].clone();
                    events.push((
                        self.time_s,
                        SimEvent::Arrived {
                            train: train.name.clone(),
                            block,
                        },
                    ));
                    break;
                }
            }
            if train.head.along_mm + travel <= length {
                self.trains[index].head.along_mm += travel;
                self.trains[index].state = TrainState::Running;
                break;
            }
            // The head reaches the end of its piece: on to the next one, if free.
            let rest = train.head.along_mm + travel - length;
            let Some((next, heading)) = self.choose_next(index, events) else {
                let train = &mut self.trains[index];
                train.head.along_mm = length;
                train.state = TrainState::Waiting;
                break;
            };
            if self.piece_taken(&next, index) {
                let train = &mut self.trains[index];
                train.head.along_mm = length;
                train.state = TrainState::Waiting;
                break;
            }
            let from = self
                .topology
                .piece(&self.trains[index].head.piece)
                .map(|p| p.district.clone());
            let to = self.topology.piece(&next).map(|p| p.district.clone());
            let train = &mut self.trains[index];
            train
                .trail
                .push((train.head.piece.clone(), train.head.heading));
            train.head = Place {
                piece: next,
                heading,
                along_mm: 0.0,
            };
            travel = rest;
            if let (Some(from), Some(to)) = (from, to) {
                if from != to {
                    events.push((
                        self.time_s,
                        SimEvent::Border {
                            train: train.name.clone(),
                            from,
                            to,
                        },
                    ));
                }
            }
        }
        self.trim_trail(index);
        self.pass_sensors(index, &start, events);
    }

    /// Keeps only the trail pieces the train still covers.
    fn trim_trail(&mut self, index: usize) {
        let covered = self.covered(&self.trains[index]).len();
        let train = &mut self.trains[index];
        let keep = covered.saturating_sub(1);
        if train.trail.len() > keep {
            let drop = train.trail.len() - keep;
            train.trail.drain(..drop);
        }
    }

    /// Sensors the head passed between `start` and now, and the level crossings they
    /// close or open.
    fn pass_sensors(&mut self, index: usize, start: &Place, events: &mut Vec<(f64, SimEvent)>) {
        let train = &self.trains[index];
        // The pieces the head moved over: (piece, from-based start, from-based end).
        let mut moved = Vec::new();
        let span = |piece: &PieceId, heading: Heading, a: f64, b: f64| {
            let length = self.piece_length(piece);
            match heading {
                Heading::Forward => (a, b),
                Heading::Backward => (length - b, length - a),
            }
        };
        if start.piece == train.head.piece {
            moved.push((
                start.piece.clone(),
                span(
                    &start.piece,
                    start.heading,
                    start.along_mm,
                    train.head.along_mm,
                ),
            ));
        } else {
            let length = self.piece_length(&start.piece);
            moved.push((
                start.piece.clone(),
                span(&start.piece, start.heading, start.along_mm, length),
            ));
            let entered = train
                .trail
                .iter()
                .skip_while(|(p, _)| p != &start.piece)
                .skip(1);
            for (piece, heading) in entered {
                let length = self.piece_length(piece);
                moved.push((piece.clone(), span(piece, *heading, 0.0, length)));
            }
            moved.push((
                train.head.piece.clone(),
                span(
                    &train.head.piece,
                    train.head.heading,
                    0.0,
                    train.head.along_mm,
                ),
            ));
        }
        let name = train.name.clone();
        for (piece, (a, b)) in moved {
            let Some(module) = self.module(&piece.module) else {
                continue;
            };
            let passed: Vec<_> = module
                .devices
                .iter()
                .filter(|d| d.kind == "sensor" && d.track.as_deref() == Some(piece.piece.as_str()))
                .filter(|d| {
                    d.at_mm
                        .is_some_and(|at| a < f64::from(at) && f64::from(at) <= b)
                })
                .cloned()
                .collect();
            let crossings = module.crossings.clone();
            let district = self
                .layout
                .district_of(&piece.module)
                .unwrap_or("")
                .to_owned();
            for device in passed {
                events.push((
                    self.time_s,
                    SimEvent::Sensor {
                        device: format!("{}/{}", piece.module, device.id),
                        district: district.clone(),
                        address: device.address,
                        train: name.clone(),
                    },
                ));
                for crossing in &crossings {
                    let key = format!("{}/{}", piece.module, crossing.id);
                    let close = crossing.close.contains(&device.id);
                    let open = crossing.open.contains(&device.id);
                    let trains = self.crossing_trains.entry(key.clone()).or_default();
                    let was_closed = !trains.is_empty();
                    if close {
                        trains.insert(name.clone());
                    } else if open {
                        trains.remove(&name);
                    }
                    let is_closed = !trains.is_empty();
                    if is_closed && !was_closed {
                        events.push((
                            self.time_s,
                            SimEvent::Crossing {
                                crossing: key,
                                name: crossing.name.clone(),
                                closed: true,
                            },
                        ));
                    } else if was_closed && !is_closed {
                        events.push((
                            self.time_s,
                            SimEvent::Crossing {
                                crossing: key,
                                name: crossing.name.clone(),
                                closed: false,
                            },
                        ));
                    }
                }
            }
        }
    }

    /// At a destination: the tail becomes the head, the train runs the other way, and the
    /// next stop of its plan is its destination.
    fn reverse(&mut self, index: usize, events: &mut Vec<(f64, SimEvent)>) {
        let covered = self.covered(&self.trains[index]);
        // The covered spans, head first; the last one holds the tail.
        let Some((tail_piece, a, b)) = covered.last().cloned() else {
            return;
        };
        let tail_heading = if covered.len() == 1 {
            self.trains[index].head.heading
        } else {
            self.trains[index].trail[self.trains[index].trail.len() + 1 - covered.len()].1
        };
        let length = self.piece_length(&tail_piece);
        // Running the other way, the new head is at the tail's end of its span.
        let new_heading = tail_heading.reversed();
        let along = match new_heading {
            Heading::Forward => b,
            Heading::Backward => length - a,
        };
        // The new trail: the covered pieces from the old head back to the tail's piece,
        // reversed in heading, oldest (the old head) first.
        let train = &self.trains[index];
        let mut pieces: Vec<(PieceId, Heading)> =
            std::iter::once((train.head.piece.clone(), train.head.heading))
                .chain(train.trail.iter().rev().cloned())
                .take(covered.len())
                .collect();
        pieces.pop(); // the tail's piece is the new head
        let trail = pieces.into_iter().map(|(p, h)| (p, h.reversed())).collect();
        let train = &mut self.trains[index];
        train.head = Place {
            piece: tail_piece,
            heading: new_heading,
            along_mm: along,
        };
        train.trail = trail;
        train.next_stop = (train.next_stop + 1) % train.plan.len().max(1);
        train.state = TrainState::Running;
        let to = train.plan.get(train.next_stop).cloned().unwrap_or_default();
        events.push((
            self.time_s,
            SimEvent::Departed {
                train: train.name.clone(),
                to,
            },
        ));
    }
}

impl std::fmt::Display for SimEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let address = |address: &Option<u16>| {
            address
                .map(|a| format!(" (LocoNet {a})"))
                .unwrap_or_default()
        };
        match self {
            SimEvent::Block {
                block,
                district,
                occupied,
                address: a,
            } => write!(
                f,
                "{district}: block {block} {}{}",
                if *occupied { "occupied" } else { "free" },
                address(a)
            ),
            SimEvent::Sensor {
                device,
                district,
                address,
                train,
            } => {
                write!(
                    f,
                    "{district}: sensor {device} (LocoNet {address}) passed by {train}"
                )
            }
            SimEvent::Turnout {
                turnout,
                district,
                closed,
                address: a,
            } => write!(
                f,
                "{district}: turnout {turnout} set {}{}",
                if *closed { "straight" } else { "diverging" },
                address(a)
            ),
            SimEvent::Crossing { name, closed, .. } => write!(
                f,
                "level crossing {name} {}",
                if *closed {
                    "closes: lights on, barriers down"
                } else {
                    "opens: barriers up, lights off"
                }
            ),
            SimEvent::Border { train, from, to } => {
                write!(f, "{train} crosses the border {from} -> {to}")
            }
            SimEvent::Arrived { train, block } => write!(f, "{train} arrives at {block}"),
            SimEvent::Departed { train, to } => write!(f, "{train} reverses and leaves for {to}"),
        }
    }
}

/// The example run: two trains on the double-track example, shuttling between the termini.
pub fn example_trains(sim: &mut LayoutSim<'_>) {
    sim.add_train(
        "IC 2001",
        3,
        600.0,
        Place {
            piece: PieceId::new("timo-adorf", "1"),
            heading: Heading::Forward,
            along_mm: 900.0,
        },
        &["anna-bdorf/bdorf_2", "timo-adorf/adorf_1"],
    );
    sim.add_train(
        "Ng 4711",
        4711,
        500.0,
        Place {
            piece: PieceId::new("anna-bdorf", "s2"),
            heading: Heading::Forward,
            along_mm: 900.0,
        },
        &["timo-adorf/adorf_3", "anna-bdorf/bdorf_4"],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn layout() -> Layout {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/layout");
        crate::layout::load(&dir.join("bench-double-track.json"), &dir).unwrap()
    }

    fn run(seconds: f64) -> Vec<(f64, SimEvent)> {
        let layout = layout();
        let topology = Topology::build(&layout);
        let mut sim = LayoutSim::new(&layout, &topology);
        example_trains(&mut sim);
        let mut events = Vec::new();
        let steps = (seconds / 0.1) as usize;
        for _ in 0..steps {
            events.extend(sim.step(0.1));
        }
        events
    }

    /// The IC runs from Adorf to Bdorf: across the border into Windows, closing and
    /// opening both level crossings on the way, into Bdorf platform 2 over the crossover.
    #[test]
    fn a_train_runs_to_the_other_terminus() {
        let events = run(120.0);
        let ic = |event: &SimEvent| matches!(event, SimEvent::Border { train, .. } | SimEvent::Arrived { train, .. } if train == "IC 2001");
        let ic_events: Vec<_> = events.iter().map(|(_, e)| e).filter(|e| ic(e)).collect();
        assert!(ic_events.contains(&&SimEvent::Border {
            train: "IC 2001".into(),
            from: "MacBook".into(),
            to: "Windows".into()
        }));
        assert!(ic_events.contains(&&SimEvent::Arrived {
            train: "IC 2001".into(),
            block: "anna-bdorf/bdorf_2".into()
        }));
        for name in ["Feldweg", "Dorfstraße"] {
            assert!(events.iter().any(
                |(_, e)| matches!(e, SimEvent::Crossing { name: n, closed: true, .. } if n == name)
            ));
            assert!(events.iter().any(
                |(_, e)| matches!(e, SimEvent::Crossing { name: n, closed: false, .. } if n == name)
            ));
        }
        // At Bdorf the crossover took it over to track 2.
        assert!(events.iter().any(|(_, e)| matches!(e, SimEvent::Turnout { turnout, closed: false, .. } if turnout == "anna-bdorf/X3")));
    }

    /// Both trains complete round trips: every destination is reached more than once, and
    /// both cross the border both ways.
    #[test]
    fn both_trains_shuttle_and_cross_the_border_both_ways() {
        let events = run(900.0);
        for (train, stops) in [
            ("IC 2001", ["anna-bdorf/bdorf_2", "timo-adorf/adorf_1"]),
            ("Ng 4711", ["timo-adorf/adorf_3", "anna-bdorf/bdorf_4"]),
        ] {
            for stop in stops {
                let count = events
                    .iter()
                    .filter(|(_, e)| matches!(e, SimEvent::Arrived { train: t, block } if t == train && block == stop))
                    .count();
                assert!(count >= 1, "{train} never reached {stop}");
            }
            for (from, to) in [("MacBook", "Windows"), ("Windows", "MacBook")] {
                assert!(
                    events.iter().any(|(_, e)| matches!(e, SimEvent::Border { train: t, from: f, to: d } if t == train && f == from && d == to)),
                    "{train} never crossed {from} -> {to}"
                );
            }
        }
    }

    /// A level crossing stays closed while any train is between its closing and opening
    /// sensors: with one train on each track, it opens only after the second has passed.
    #[test]
    fn a_crossing_stays_closed_until_the_last_train_has_passed() {
        let layout = layout();
        let topology = Topology::build(&layout);
        let mut sim = LayoutSim::new(&layout, &topology);
        // Two trains towards the Feldweg crossing at once, one on each track.
        sim.add_train(
            "East",
            1,
            300.0,
            Place {
                piece: PieceId::new("timo-adorf", "1b"),
                heading: Heading::Forward,
                along_mm: 50.0,
            },
            &["anna-bdorf/bdorf_2"],
        );
        sim.add_train(
            "West",
            2,
            300.0,
            Place {
                piece: PieceId::new("timo-2e96-border", "2"),
                heading: Heading::Backward,
                along_mm: 450.0,
            },
            &["timo-adorf/adorf_1"],
        );
        let mut changes = Vec::new();
        let (mut east_passed, mut west_passed) = (None, None);
        for _ in 0..200 {
            for (time, event) in sim.step(0.1) {
                match event {
                    SimEvent::Crossing { name, closed, .. } if name == "Feldweg" => {
                        changes.push((time, closed))
                    }
                    SimEvent::Sensor { device, .. } if device == "timo-2e96-west/lc_open_1" => {
                        east_passed = Some(time)
                    }
                    SimEvent::Sensor { device, .. } if device == "timo-2e96-west/lc_open_2" => {
                        west_passed = Some(time)
                    }
                    _ => {}
                }
            }
        }
        // Closed once and opened once, and only when the second train has passed: the first
        // one passing its opening sensor while the second still comes leaves it closed.
        let states: Vec<bool> = changes.iter().map(|(_, closed)| *closed).collect();
        assert_eq!(states, [true, false], "{changes:?}");
        let (east, west) = (east_passed.unwrap(), west_passed.unwrap());
        assert!(
            east < west,
            "East must pass first for this test: {east} {west}"
        );
        assert_eq!(
            changes[1].0, west,
            "opened at {}, the last train passed at {west}",
            changes[1].0
        );
    }

    /// Occupancy reports carry the LocoNet address of their detector.
    #[test]
    fn occupancy_comes_with_its_address() {
        let events = run(30.0);
        assert!(events.iter().any(|(_, e)| matches!(
            e,
            SimEvent::Block { block, occupied: true, address: Some(201), district, .. }
                if block == "timo-2e96-west/line_1" && district == "MacBook"
        )));
    }
}
