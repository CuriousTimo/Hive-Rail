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

/// How long a train stands at a stop, in fast clock minutes: its usual stop time, varied
/// from half to one and a half times, differently at each stop but the same in every run.
fn stop_time(train: &Train) -> f64 {
    let mut hash: u32 = 2_166_136_261;
    for byte in train.name.bytes().chain(train.stops_made.to_le_bytes()) {
        hash = (hash ^ u32::from(byte)).wrapping_mul(16_777_619);
    }
    train.dwell_min * (0.5 + f64::from(hash % 1000) / 1000.0)
}

/// The passing sidings (`passing`), or the through tracks of stations that have passing
/// sidings (the other tracks that are not platforms).
fn passing_tracks(layout: &Layout, passing: bool) -> BTreeSet<PieceId> {
    let mut tracks = BTreeSet::new();
    for (module_id, (module, _)) in &layout.modules {
        for station in &module.stations {
            if !station.tracks.iter().any(|t| t.passing) {
                continue;
            }
            for track in &station.tracks {
                if track.passing == passing && !track.platform {
                    tracks.insert(PieceId::new(module_id, &track.track));
                }
            }
        }
    }
    tracks
}

/// How far behind a faster train must be for a slower one to step aside for it.
const OVERTAKE_WITHIN_MM: f64 = 5000.0;

/// Each station's platform tracks, by station name, for plans that name a station.
fn stations(layout: &Layout) -> BTreeMap<String, Vec<(PieceId, String)>> {
    let mut stations: BTreeMap<String, Vec<(PieceId, String)>> = BTreeMap::new();
    for (module_id, (module, _)) in &layout.modules {
        for station in &module.stations {
            let platforms = station
                .tracks
                .iter()
                .filter(|track| track.platform)
                .map(|track| (PieceId::new(module_id, &track.track), track.name.clone()));
            stations
                .entry(station.name.clone())
                .or_default()
                .extend(platforms);
        }
    }
    stations.retain(|_, platforms| !platforms.is_empty());
    stations
}

/// How far before a signal at stop a train halts.
const SIGNAL_MARGIN_MM: f64 = 30.0;

/// How many signals ahead of a train are cleared for it at most.
const SECTIONS_AHEAD: usize = 2;

/// A transfer block at a district border: fed by the sending district while a train comes
/// in, by the receiving district once the train is fully inside and nothing is left on the
/// sending side, and by the sending district again once it is empty.
#[derive(Debug, Clone)]
pub struct Transfer {
    pub source: String,
    pub receiving: String,
    /// The piece on the sending side of the border.
    pub source_piece: PieceId,
    pub fed_by: String,
}

fn transfers(topology: &Topology) -> BTreeMap<String, Transfer> {
    let joints = topology.border_joints();
    let mut transfers = BTreeMap::new();
    for (key, block) in &topology.blocks {
        if block.role.as_deref() != Some("transfer") {
            continue;
        }
        let other = joints.iter().find_map(|(a, b)| {
            if a == &block.piece {
                Some(b.clone())
            } else if b == &block.piece {
                Some(a.clone())
            } else {
                None
            }
        });
        let Some(source_piece) = other else {
            continue;
        };
        let source = topology
            .piece(&source_piece)
            .map(|p| p.district.clone())
            .unwrap_or_default();
        transfers.insert(
            key.clone(),
            Transfer {
                receiving: block.district.clone(),
                source_piece,
                fed_by: source.clone(),
                source,
            },
        );
    }
    transfers
}

/// A signal on a piece, facing trains running in `heading` along it.
#[derive(Debug, Clone)]
struct SignalInfo {
    key: String,
    district: String,
    piece: PieceId,
    heading: Heading,
    /// Where it stands, in mm from the piece's `from` point.
    at_mm: Option<f64>,
    aspects: Vec<String>,
    address: u16,
}

/// The signals of every module: devices of type `signal` on a track, with their running
/// direction turned into a heading along the piece (from the module's geometry).
fn signals(layout: &Layout) -> Vec<SignalInfo> {
    let mut signals = Vec::new();
    for (module_id, (module, _)) in &layout.modules {
        for device in module.devices.iter().filter(|d| d.kind == "signal") {
            let Some(track) = &device.track else {
                continue;
            };
            let heading = device
                .direction
                .as_deref()
                .and_then(|direction| heading_for(module, track, direction))
                .unwrap_or(Heading::Forward);
            let aspects = device
                .aspects
                .as_ref()
                .and_then(|a| a.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_else(|| vec!["red".to_owned(), "green".to_owned()]);
            signals.push(SignalInfo {
                key: format!("{module_id}/{}", device.id),
                district: layout.district_of(module_id).unwrap_or("").to_owned(),
                piece: PieceId::new(module_id, track),
                heading,
                at_mm: device.at_mm.map(f64::from),
                aspects,
                address: device.address,
            });
        }
    }
    signals
}

/// The pieces of the open line, run one way only: on a module without points every piece
/// with a running direction (right-hand running on double track). In a station a train
/// may run against a track's regular direction, to reach a platform or to turn back.
fn one_way(layout: &Layout) -> BTreeMap<PieceId, Heading> {
    let mut one_way = BTreeMap::new();
    for (module_id, (module, _)) in &layout.modules {
        if !module.turnouts.is_empty() {
            continue;
        }
        for piece in &module.track {
            let Some(direction) = &piece.direction else {
                continue;
            };
            let first = direction.split('>').next().unwrap_or("");
            let end = |point: &str| point.split('.').next().unwrap_or("").to_owned();
            let heading = if end(&piece.from) == first {
                Some(Heading::Forward)
            } else if end(&piece.to) == first {
                Some(Heading::Backward)
            } else {
                heading_for(module, &piece.id, direction)
            };
            if let Some(heading) = heading {
                one_way.insert(PieceId::new(module_id, &piece.id), heading);
            }
        }
    }
    one_way
}

/// The heading along a piece that runs in a module direction such as `A>B`: forward when
/// the piece's drawing runs from the first end towards the second.
pub(crate) fn heading_for(module: &ModuleFile, piece: &str, direction: &str) -> Option<Heading> {
    let (from, to) = direction.split_once('>')?;
    let geometry = module.geometry.as_ref()?;
    let (a, b) = (geometry.ends.get(from)?.at, geometry.ends.get(to)?.at);
    let line = geometry.track.get(piece)?;
    let (first, last) = (line.first()?, line.last()?);
    let along = (last[0] - first[0]) * (b[0] - a[0]) + (last[1] - first[1]) * (b[1] - a[1]);
    Some(if along >= 0.0 {
        Heading::Forward
    } else {
        Heading::Backward
    })
}

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
    /// Stopped because its district has no power (off, emergency stop, short); it goes on
    /// when the power is back.
    NoPower,
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
    /// The route reserved to the next destination: every piece and heading from the head
    /// on. Empty until it could be reserved.
    pub route: Vec<(PieceId, Heading)>,
    /// Destinations, visited in turn: a block (`module/block`), or a station by name
    /// (`Bdorf`), where the router picks a free platform track.
    pub plan: Vec<String>,
    next_stop: usize,
    /// The piece this trip ends on: the plan's block, or the platform track picked at the
    /// station; chosen when the trip starts.
    pub target: Option<PieceId>,
    /// The usual stop at a destination, in fast clock minutes; each stop varies around it.
    pub dwell_min: f64,
    /// Stops made, so each stop's time varies but repeats run after run.
    stops_made: u32,
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
    /// A route was reserved for a train (its pieces in order), or a piece released.
    Route {
        train: String,
        to: String,
        pieces: usize,
    },
    /// A transfer block is now fed by this district.
    Feed { block: String, district: String },
    /// A signal changed its aspect.
    Signal {
        signal: String,
        district: String,
        aspect: String,
        address: u16,
    },
    /// A slower train waits in a passing siding to let a faster one by.
    Yield { train: String, to: String },
    /// A train stopped because its district lost power, or went on when it came back.
    Power {
        train: String,
        district: String,
        powered: bool,
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
    /// The fast clock's rate: a stop of one fast minute lasts 60 / rate real seconds. The web
    /// simulator follows the mesh's clock; without one, 4.
    pub clock_rate: f64,
    /// Each station's platform tracks with their numbers, for plans that name a station.
    stations: BTreeMap<String, Vec<(PieceId, String)>>,
    /// When each platform track was last arrived at, so trains share the platforms.
    platform_used: BTreeMap<PieceId, f64>,
    /// Passing sidings, and the through tracks beside them: a slower train with a faster
    /// one close behind takes the siding and waits there until the faster one has passed.
    passing: BTreeSet<PieceId>,
    through: BTreeSet<PieceId>,
    /// The trains waiting in a passing siding now, and for which train.
    pub yielding: BTreeMap<String, String>,
    /// Each district's power as its node reports it (ON, OFF, STOP, SHORT). A district with
    /// no report counts as powered, so the simulator runs on its own.
    pub district_power: BTreeMap<String, String>,
    /// Track pieces reserved for a train's route, by piece: the train's name.
    pub reserved: BTreeMap<PieceId, String>,
    /// The transfer blocks at district borders and which district feeds each now.
    pub transfers: BTreeMap<String, Transfer>,
    /// The open-line pieces trains run along one way only, and that way.
    one_way: BTreeMap<PieceId, Heading>,
    /// The signals and the aspect each shows now.
    signals: Vec<SignalInfo>,
    pub aspects: BTreeMap<String, String>,
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
            clock_rate: 4.0,
            stations: stations(layout),
            platform_used: BTreeMap::new(),
            passing: passing_tracks(layout, true),
            through: passing_tracks(layout, false),
            yielding: BTreeMap::new(),
            turnouts,
            occupied: BTreeSet::new(),
            crossing_trains: BTreeMap::new(),
            district_power: BTreeMap::new(),
            reserved: BTreeMap::new(),
            transfers: transfers(topology),
            one_way: one_way(layout),
            signals: signals(layout),
            aspects: BTreeMap::new(),
            time_s: 0.0,
        }
    }

    /// Puts a train with its head on `head`; its tail lies behind it on the same piece.
    /// Returns it, for a different speed or stop time.
    pub fn add_train(
        &mut self,
        name: &str,
        address: u16,
        length_mm: f64,
        head: Place,
        plan: &[&str],
    ) -> &mut Train {
        self.trains.push(Train {
            name: name.to_owned(),
            address,
            length_mm,
            speed_mm_s: 150.0,
            head,
            trail: Vec::new(),
            state: TrainState::Running,
            route: Vec::new(),
            plan: plan.iter().map(|stop| (*stop).to_owned()).collect(),
            next_stop: 0,
            target: None,
            dwell_min: 2.0,
            stops_made: 0,
        });
        self.trains.last_mut().expect("just added")
    }

    fn module(&self, id: &str) -> Option<&ModuleFile> {
        self.layout.modules.get(id).map(|(module, _)| module)
    }

    fn piece_length(&self, piece: &PieceId) -> f64 {
        self.topology
            .piece(piece)
            .map_or(0.0, |info| f64::from(info.length_mm))
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
        if let Some(target) = &train.target {
            return Some(target.clone());
        }
        let stop = train.plan.get(train.next_stop)?;
        self.topology
            .blocks
            .get(stop)
            .map(|block| block.piece.clone())
    }

    /// For a train heading for a station: whether its reservation has reached the station's
    /// module yet. `None` when its next stop is not a station.
    fn approaching_station(&self, index: usize) -> Option<bool> {
        let train = &self.trains[index];
        let platforms = self.stations.get(train.plan.get(train.next_stop)?)?;
        let end = train.route.last().map_or(&train.head.piece, |(p, _)| p);
        Some(
            platforms
                .iter()
                .any(|(piece, _)| piece.module == end.module),
        )
    }

    /// Where a train heading for a station runs before it picks a platform: the station's
    /// first platform (the way there is the same for every platform).
    fn station_entry(&self, train: &Train) -> Option<PieceId> {
        let stop = train.plan.get(train.next_stop)?;
        self.stations
            .get(stop)?
            .first()
            .map(|(piece, _)| piece.clone())
    }

    /// The piece the train's next trip ends on. A block is that block's piece. A station is
    /// the free platform track (no other train stands on it, has reserved it or is heading
    /// for it) with the lowest cost: a penalty for every piece of the way in and the way out
    /// again (towards the train's following stop) that another train holds or covers now,
    /// so a train does not cross one departing or arriving; a little for the length of
    /// both; less for a track that has stood empty longer. With every track taken, the
    /// first; the train then waits.
    fn pick_target(&self, index: usize) -> Option<PieceId> {
        let train = &self.trains[index];
        let stop = train.plan.get(train.next_stop)?;
        if let Some(block) = self.topology.blocks.get(stop) {
            return Some(block.piece.clone());
        }
        let platforms = self.stations.get(stop)?;
        let held: BTreeSet<PieceId> = self
            .reserved
            .iter()
            .filter(|(_, by)| **by != train.name)
            .map(|(piece, _)| piece.clone())
            .chain(
                self.trains
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != index)
                    .flat_map(|(_, other)| self.covered(other).into_iter().map(|(p, _, _)| p)),
            )
            .collect();
        let targeted = |piece: &PieceId| {
            self.trains
                .iter()
                .enumerate()
                .any(|(i, other)| i != index && other.target.as_ref() == Some(piece))
        };
        let start = train
            .route
            .last()
            .cloned()
            .unwrap_or((train.head.piece.clone(), train.head.heading));
        // Where it goes after this stop: a platform of the following station, or a block.
        let after = train
            .plan
            .get((train.next_stop + 1) % train.plan.len().max(1));
        let onward = after.and_then(|next| {
            self.topology
                .blocks
                .get(next)
                .map(|b| b.piece.clone())
                .or_else(|| {
                    self.stations
                        .get(next)
                        .and_then(|p| p.first())
                        .map(|(p, _)| p.clone())
                })
        });
        let mut best: Option<(f64, &PieceId)> = None;
        for (piece, _) in platforms {
            if held.contains(piece) || targeted(piece) {
                continue;
            }
            let Some(way_in) = self.find_way(&start, piece, None) else {
                continue;
            };
            let arrive_heading = way_in.last().map_or(Heading::Forward, |(_, h)| *h);
            let way_out = onward
                .as_ref()
                .and_then(|goal| {
                    self.find_way(&(piece.clone(), arrive_heading.reversed()), goal, None)
                })
                .unwrap_or_default();
            let conflicts = way_in
                .iter()
                .chain(way_out.iter())
                .filter(|(p, _)| p != piece && held.contains(p))
                .count();
            // Conflicts with moving trains weigh most; the length of the way in and out a
            // little; a track that has stood empty for a while (up to 10 minutes) is
            // preferred, so trains share the platforms instead of always taking the nearest.
            let idle_min = self
                .platform_used
                .get(piece)
                .map_or(10.0, |used| ((self.time_s - used) / 60.0).min(10.0));
            let cost = 4.0 * conflicts as f64 + 0.25 * (way_in.len() + way_out.len()) as f64
                - 0.3 * idle_min;
            if best.is_none_or(|(c, _)| cost < c) {
                best = Some((cost, piece));
            }
        }
        best.map(|(_, piece)| piece.clone())
            .or_else(|| platforms.first().map(|(piece, _)| piece.clone()))
    }

    /// Where a train running along a piece in `heading` stops when the way beyond is not
    /// set: just before the signal there, or at the end of the piece.
    fn stop_point(&self, piece: &PieceId, heading: Heading) -> f64 {
        let length = self.piece_length(piece);
        self.signals
            .iter()
            .find(|s| s.piece == *piece && s.heading == heading)
            .and_then(|s| s.at_mm)
            .map_or(length, |at| {
                let along = match heading {
                    Heading::Forward => at,
                    Heading::Backward => length - at,
                };
                (along - SIGNAL_MARGIN_MM).clamp(0.0, length)
            })
    }

    /// A faster train close behind on the same way: one whose way to its destination
    /// runs over this train's head piece (or the through track beside its passing siding),
    /// in the same direction, within `OVERTAKE_WITHIN_MM`.
    fn faster_follower(&self, index: usize) -> Option<String> {
        let train = &self.trains[index];
        let here = (train.head.piece.clone(), train.head.heading);
        self.trains.iter().enumerate().find_map(|(i, other)| {
            if i == index || other.speed_mm_s <= train.speed_mm_s {
                return None;
            }
            if matches!(
                other.state,
                TrainState::Dwelling { .. } | TrainState::NoPower
            ) {
                return None;
            }
            let goal = self
                .destination(other)
                .or_else(|| self.station_entry(other))?;
            let way =
                self.find_way(&(other.head.piece.clone(), other.head.heading), &goal, None)?;
            // Passing this train: over its head piece, or (when it stands in a passing
            // siding) over the through track beside it, in the same module and direction.
            let beside = |(piece, heading): &(PieceId, Heading)| {
                *heading == here.1
                    && (*piece == here.0
                        || (self.passing.contains(&here.0)
                            && piece.module == here.0.module
                            && self.through.contains(piece)))
            };
            let at = way.iter().position(beside)?;
            if at == 0 {
                return None;
            }
            let distance = self.piece_length(&other.head.piece) - other.head.along_mm
                + way[1..at]
                    .iter()
                    .map(|(p, _)| self.piece_length(p))
                    .sum::<f64>();
            (distance <= OVERTAKE_WITHIN_MM).then(|| other.name.clone())
        })
    }

    /// Whether a signal stands at the exit of this piece for trains running in `heading`.
    fn signal_at(&self, piece: &PieceId, heading: Heading) -> bool {
        self.signals
            .iter()
            .any(|s| s.piece == *piece && s.heading == heading)
    }

    /// The shortest way from a piece to another, through any turnout and along the open
    /// line only in its running direction; with `avoid`, only over pieces not in it.
    fn find_way(
        &self,
        start: &(PieceId, Heading),
        goal: &PieceId,
        avoid: Option<&BTreeSet<PieceId>>,
    ) -> Option<Vec<(PieceId, Heading)>> {
        if start.0 == *goal {
            return Some(vec![start.clone()]);
        }
        let mut parent: BTreeMap<(PieceId, Heading), (PieceId, Heading)> = BTreeMap::new();
        let mut queue = std::collections::VecDeque::from([start.clone()]);
        while let Some(state) = queue.pop_front() {
            for next in self.topology.next(&state.0, state.1, |_, _| None) {
                if next == *start
                    || parent.contains_key(&next)
                    || self.one_way.get(&next.0).is_some_and(|way| *way != next.1)
                    || avoid.is_some_and(|avoid| avoid.contains(&next.0))
                {
                    continue;
                }
                parent.insert(next.clone(), state.clone());
                if next.0 == *goal {
                    let mut way = vec![next.clone()];
                    let mut at = next;
                    while let Some(before) = parent.get(&at) {
                        way.push(before.clone());
                        at = before.clone();
                    }
                    way.reverse();
                    return Some(way);
                }
                queue.push_back(next);
            }
        }
        None
    }

    /// Reserves the next section of the train's way to its destination, all or nothing:
    /// from the end of what it holds up to the next signal (or the destination), over
    /// pieces no other train covers or has reserved, with every turnout on it set. False
    /// when the section is taken (the train waits at the signal) or the way is complete.
    fn reserve(&mut self, index: usize, events: &mut Vec<(f64, SimEvent)>) -> bool {
        // A station's platform is picked as the train comes in: once its reservation has
        // reached the station's module (its entry signal). Until then it runs towards the
        // station's first platform, the way in being the same for all.
        if self.trains[index].target.is_none() {
            let approach = self.approaching_station(index);
            if approach.is_none_or(|near| near) {
                self.trains[index].target = self.pick_target(index);
            }
        }
        let train = &self.trains[index];
        let Some(goal) = self
            .destination(train)
            .or_else(|| self.station_entry(train))
        else {
            return false;
        };
        let start = train
            .route
            .last()
            .cloned()
            .unwrap_or((train.head.piece.clone(), train.head.heading));
        if !train.route.is_empty() && start.0 == goal {
            return false;
        }
        // Signals already cleared ahead: the route is set as the train comes, not all the
        // way at once, so it holds no station throat it is still far from.
        let cleared = train.route[..train.route.len().saturating_sub(1)]
            .iter()
            .filter(|(p, h)| self.signal_at(p, *h))
            .count();
        if cleared >= SECTIONS_AHEAD {
            return false;
        }
        let mut blocked: BTreeSet<PieceId> = self
            .reserved
            .iter()
            .filter(|(_, by)| **by != train.name)
            .map(|(piece, _)| piece.clone())
            .collect();
        for (i, other) in self.trains.iter().enumerate() {
            if i != index {
                blocked.extend(self.covered(other).into_iter().map(|(p, _, _)| p));
            }
        }
        // A faster train close behind: wait in a passing siding until it has passed, and
        // on the way, prefer the siding over the through track.
        let follower = self.faster_follower(index);
        let name = self.trains[index].name.clone();
        // Its reservation ends in a passing siding (at the siding's exit signal): with a
        // faster train behind, it reserves no further and waits there.
        if self.passing.contains(&start.0) {
            match &follower {
                Some(faster) => {
                    if self.yielding.get(&name) != Some(faster) {
                        self.yielding.insert(name.clone(), faster.clone());
                        events.push((
                            self.time_s,
                            SimEvent::Yield {
                                train: name,
                                to: faster.clone(),
                            },
                        ));
                    }
                    return false;
                }
                None => {
                    self.yielding.remove(&name);
                }
            }
        }
        let mut avoid_through = blocked.clone();
        if follower.is_some() {
            avoid_through.extend(self.through.iter().cloned());
        }
        let train = &self.trains[index];
        // A free way if there is one (a passing siding instead of a taken track), else
        // the shortest, to wait for.
        let Some(way) = self
            .find_way(&start, &goal, Some(&avoid_through))
            .or_else(|| self.find_way(&start, &goal, Some(&blocked)))
            .or_else(|| self.find_way(&start, &goal, None))
        else {
            return false;
        };
        // The section: up to and including the next piece with a signal at its exit.
        let end = way
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, (piece, heading))| self.signal_at(piece, *heading))
            .map_or(way.len() - 1, |(i, _)| i);
        let section = &way[..=end];
        let first_new = usize::from(!train.route.is_empty());
        if section[first_new..]
            .iter()
            .any(|(p, _)| blocked.contains(p))
        {
            return false;
        }
        // The turnouts on the way.
        for pair in section.windows(2) {
            let Some(ahead) = self.topology.turnout_ahead(&pair[0].0, pair[0].1) else {
                continue;
            };
            let Some((leg, _, _)) = ahead.options.iter().find(|(_, p, _)| *p == pair[1].0) else {
                continue;
            };
            let closed =
                *leg == Leg::Straight || (*leg == Leg::Trunk && ahead.entry == Leg::Straight);
            let key = format!("{}/{}", ahead.module, ahead.id);
            if self.turnouts.get(&key) != Some(&closed) {
                self.turnouts.insert(key, closed);
                events.push((
                    self.time_s,
                    self.turnout_event(&ahead.module, &ahead.id, closed),
                ));
            }
        }
        let section = section[first_new..].to_vec();
        let train = &mut self.trains[index];
        events.push((
            self.time_s,
            SimEvent::Route {
                train: train.name.clone(),
                to: section
                    .last()
                    .map(|(p, _)| p.to_string())
                    .unwrap_or_default(),
                pieces: section.len(),
            },
        ));
        train.route.extend(section);
        self.update_reserved();
        true
    }

    /// The pieces reserved now: every train's route ahead of it.
    fn update_reserved(&mut self) {
        self.reserved = self
            .trains
            .iter()
            .flat_map(|train| {
                train
                    .route
                    .iter()
                    .map(move |(piece, _)| (piece.clone(), train.name.clone()))
            })
            .collect();
    }

    /// The aspect each signal shows. A signal guards the exit of its piece: it shows
    /// proceed while a train holds the section beyond it, stop otherwise. A three-aspect
    /// signal shows caution when the next signal on the way is at stop, or the way ends
    /// at the destination.
    fn update_signals(&mut self, events: &mut Vec<(f64, SimEvent)>) {
        let mut changes = Vec::new();
        for signal in &self.signals {
            let mut aspect = "red";
            for train in &self.trains {
                let Some(position) = train
                    .route
                    .iter()
                    .position(|(p, h)| *p == signal.piece && *h == signal.heading)
                else {
                    continue;
                };
                if position + 1 >= train.route.len() {
                    continue;
                }
                let next_clear = train.route[position + 1..]
                    .iter()
                    .enumerate()
                    .find(|(_, (p, h))| self.signal_at(p, *h))
                    .is_some_and(|(i, _)| position + 1 + i + 1 < train.route.len());
                aspect = if !next_clear && signal.aspects.iter().any(|a| a == "yellow") {
                    "yellow"
                } else {
                    "green"
                };
                break;
            }
            if self.aspects.get(&signal.key).map(String::as_str) != Some(aspect) {
                changes.push((signal.clone(), aspect.to_owned()));
            }
        }
        for (signal, aspect) in changes {
            self.aspects.insert(signal.key.clone(), aspect.clone());
            events.push((
                self.time_s,
                SimEvent::Signal {
                    signal: signal.key,
                    district: signal.district,
                    aspect,
                    address: signal.address,
                },
            ));
        }
    }

    /// Which district feeds each transfer block: the sending one while a train covers the
    /// sending side of the border, the receiving one while the block is occupied and the
    /// sending side clear, the sending one again once it is free.
    fn update_feeds(&mut self, events: &mut Vec<(f64, SimEvent)>) {
        let covered: BTreeSet<PieceId> = self
            .trains
            .iter()
            .flat_map(|t| self.covered(t).into_iter().map(|(p, _, _)| p))
            .collect();
        for (key, transfer) in self.transfers.iter_mut() {
            let feed = if self.occupied.contains(key) && !covered.contains(&transfer.source_piece) {
                &transfer.receiving
            } else {
                &transfer.source
            };
            if *feed != transfer.fed_by {
                transfer.fed_by = feed.clone();
                events.push((
                    self.time_s,
                    SimEvent::Feed {
                        block: key.clone(),
                        district: feed.clone(),
                    },
                ));
            }
        }
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
            // As far ahead as the track is free, section by section.
            if !matches!(self.trains[index].state, TrainState::Dwelling { .. }) {
                while self.reserve(index, &mut events) {}
            }
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
        self.update_feeds(&mut events);
        self.update_signals(&mut events);
        events
    }

    /// Whether a district has track power: no report counts as powered.
    pub fn powered(&self, district: &str) -> bool {
        self.district_power
            .get(district)
            .is_none_or(|power| power == "ON")
    }

    fn step_train(&mut self, index: usize, dt_s: f64, events: &mut Vec<(f64, SimEvent)>) {
        // A loco stops where its district has no power; the other districts run on.
        let district = self
            .topology
            .piece(&self.trains[index].head.piece)
            .map(|p| p.district.clone())
            .unwrap_or_default();
        let powered = self.powered(&district);
        let halted = self.trains[index].state == TrainState::NoPower;
        if !powered {
            if !halted && !matches!(self.trains[index].state, TrainState::Dwelling { .. }) {
                self.trains[index].state = TrainState::NoPower;
                events.push((
                    self.time_s,
                    SimEvent::Power {
                        train: self.trains[index].name.clone(),
                        district,
                        powered: false,
                    },
                ));
            }
            // A dwelling train cannot leave either; its time runs on.
            return;
        }
        if halted {
            self.trains[index].state = TrainState::Running;
            events.push((
                self.time_s,
                SimEvent::Power {
                    train: self.trains[index].name.clone(),
                    district,
                    powered: true,
                },
            ));
        }
        match self.trains[index].state {
            TrainState::Dwelling { until_s } => {
                if self.time_s >= until_s {
                    self.reverse(index, events);
                }
                return;
            }
            TrainState::Waiting | TrainState::Running | TrainState::NoPower => {}
        }
        if self.trains[index].route.is_empty() {
            self.trains[index].state = TrainState::Waiting;
            return;
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
                        until_s: self.time_s + stop_time(train) * 60.0 / self.clock_rate.max(1.0),
                    };
                    // The stop, with the track for a station: "Adorf track 2".
                    let stop = train.plan[train.next_stop].clone();
                    let track = self.stations.get(&stop).and_then(|tracks| {
                        tracks
                            .iter()
                            .find(|(piece, _)| *piece == train.head.piece)
                            .map(|(_, name)| name.clone())
                    });
                    let block = match track {
                        Some(name) => format!("{stop} track {name}"),
                        None => stop,
                    };
                    let arrived_on = train.head.piece.clone();
                    self.platform_used.insert(arrived_on, self.time_s);
                    let train = &mut self.trains[index];
                    train.route.clear();
                    let name = train.name.clone();
                    self.update_reserved();
                    events.push((self.time_s, SimEvent::Arrived { train: name, block }));
                    break;
                }
            }
            // With the way beyond not set, the train halts at the signal.
            let set_beyond = train.route.len() >= 2;
            let limit = if set_beyond {
                length
            } else {
                self.stop_point(&train.head.piece, train.head.heading)
            };
            if train.head.along_mm + travel <= limit {
                self.trains[index].head.along_mm += travel;
                self.trains[index].state = TrainState::Running;
                break;
            }
            if !set_beyond {
                let train = &mut self.trains[index];
                train.head.along_mm = limit.max(train.head.along_mm);
                train.state = TrainState::Waiting;
                break;
            }
            // The head reaches the end of its piece: on to the next one, if free.
            let rest = train.head.along_mm + travel - length;
            let (next, heading) = self.trains[index].route[1].clone();
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
            train.route.remove(0);
            // The piece left behind is no longer reserved (while the train still covers it,
            // the covered check keeps others off it).
            let left = train.head.piece.clone();
            if self
                .reserved
                .get(&left)
                .is_some_and(|by| *by == self.trains[index].name)
                && !self.trains[index].route.iter().any(|(p, _)| *p == left)
            {
                self.reserved.remove(&left);
            }
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
        train.target = None;
        train.stops_made += 1;
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
            SimEvent::Route { train, to, pieces } => {
                write!(f, "route for {train} to {to} reserved ({pieces} pieces)")
            }
            SimEvent::Feed { block, district } => {
                write!(f, "transfer block {block} now fed by {district}")
            }
            SimEvent::Signal {
                signal,
                district,
                aspect,
                address,
            } => write!(
                f,
                "{district}: signal {signal} shows {aspect} (LocoNet {address})"
            ),
            SimEvent::Yield { train, to } => {
                write!(f, "{train} waits in the passing siding to let {to} by")
            }
            SimEvent::Power {
                train,
                district,
                powered,
            } => {
                if *powered {
                    write!(f, "{district}: power back, {train} goes on")
                } else {
                    write!(f, "{district}: no track power, {train} stops")
                }
            }
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
    // Three trains start at each terminus and shuttle to the other; each picks a free
    // platform track. Train types differ in speed and stop time (fast clock minutes): a
    // slower train with a faster one close behind lets it by in Cdorf's passing sidings.
    let at = |module: &str, track: &str, along_mm: f64| Place {
        piece: PieceId::new(module, track),
        // Platform tracks are drawn from the throat; leaving the terminus runs backward.
        heading: Heading::Backward,
        along_mm,
    };
    // Name, loco, length, start, plan, speed (mm/s), usual stop (fast minutes).
    let trains = [
        (
            "IC 2001",
            3,
            600.0,
            at("timo-adorf", "1", 900.0),
            ["Bdorf", "Adorf"],
            150.0,
            1.0,
        ),
        (
            "RB 7701",
            7701,
            350.0,
            at("timo-adorf", "2", 900.0),
            ["Bdorf", "Adorf"],
            110.0,
            2.0,
        ),
        (
            "Ng 6203",
            6203,
            500.0,
            at("timo-adorf", "3", 800.0),
            ["Bdorf", "Adorf"],
            75.0,
            6.0,
        ),
        (
            "RE 4401",
            4401,
            500.0,
            at("anna-bdorf", "1", 900.0),
            ["Adorf", "Bdorf"],
            130.0,
            1.5,
        ),
        (
            "Dg 5501",
            5501,
            700.0,
            at("anna-bdorf", "2", 1000.0),
            ["Adorf", "Bdorf"],
            95.0,
            4.0,
        ),
        (
            "RB 7702",
            7702,
            350.0,
            at("anna-bdorf", "3", 800.0),
            ["Adorf", "Bdorf"],
            110.0,
            2.0,
        ),
    ];
    for (name, address, length, place, plan, speed, stop) in trains {
        let train = sim.add_train(name, address, length, place, &plan);
        train.speed_mm_s = speed;
        train.dwell_min = stop;
    }
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
        assert!(ic_events.iter().any(
            |e| matches!(e, SimEvent::Arrived { train, block } if train == "IC 2001" && block.starts_with("Bdorf track"))
        ));
        for name in ["Feldweg", "Dorfstraße"] {
            assert!(events.iter().any(
                |(_, e)| matches!(e, SimEvent::Crossing { name: n, closed: true, .. } if n == name)
            ));
            assert!(events.iter().any(
                |(_, e)| matches!(e, SimEvent::Crossing { name: n, closed: false, .. } if n == name)
            ));
        }
        // At Bdorf the throat's turnouts were set for its platform.
        assert!(events.iter().any(|(_, e)| matches!(e, SimEvent::Turnout { turnout, .. } if turnout.starts_with("anna-bdorf/"))));
    }

    /// Both trains complete round trips: every destination is reached more than once, and
    /// both cross the border both ways.
    #[test]
    fn every_train_shuttles_and_crosses_the_border_both_ways() {
        let events = run(1500.0);
        for train in [
            "IC 2001", "RB 7701", "Ng 6203", "RE 4401", "Dg 5501", "RB 7702",
        ] {
            for stop in ["Adorf", "Bdorf"] {
                let count = events
                    .iter()
                    .filter(|(_, e)| matches!(e, SimEvent::Arrived { train: t, block } if t == train && block.starts_with(stop)))
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
                piece: PieceId::new("timo-adorf", "out"),
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

    /// No piece is ever held by two trains: not reserved by both, not reserved by one
    /// while the other stands on it. A train held at a signal stops before it.
    #[test]
    fn no_two_trains_ever_hold_one_piece() {
        let layout = layout();
        let topology = Topology::build(&layout);
        let mut sim = LayoutSim::new(&layout, &topology);
        example_trains(&mut sim);
        for _ in 0..3000 {
            sim.step(0.1);
            for train in &sim.trains {
                if train.state == TrainState::Waiting && train.route.len() < 2 {
                    let stop = sim.stop_point(&train.head.piece, train.head.heading);
                    assert!(
                        train.head.along_mm <= stop + 1e-6,
                        "{} past the signal on {} at {:.1} s",
                        train.name,
                        train.head.piece,
                        sim.time_s
                    );
                }
            }
            for (a, train) in sim.trains.iter().enumerate() {
                let covered: BTreeSet<PieceId> =
                    sim.covered(train).into_iter().map(|(p, _, _)| p).collect();
                for (piece, by) in &sim.reserved {
                    assert!(
                        by == &train.name || !covered.contains(piece),
                        "{piece} reserved by {by} under {} at {:.1} s",
                        train.name,
                        sim.time_s
                    );
                }
                for other in &sim.trains[a + 1..] {
                    let theirs: BTreeSet<PieceId> =
                        sim.covered(other).into_iter().map(|(p, _, _)| p).collect();
                    assert!(
                        covered.is_disjoint(&theirs),
                        "trains collide at {:.1} s",
                        sim.time_s
                    );
                }
            }
        }
    }

    /// With a train standing on the line beyond the border, the IC is held at the border
    /// signal, which stays at stop, short of it; once the line is clear it goes on.
    #[test]
    fn a_train_is_held_at_a_signal_until_the_section_is_clear() {
        let layout = layout();
        let topology = Topology::build(&layout);
        let mut sim = LayoutSim::new(&layout, &topology);
        example_trains(&mut sim);
        sim.add_train(
            "Lok 5",
            5,
            300.0,
            Place {
                piece: PieceId::new("anna-2e96-east", "1"),
                heading: Heading::Forward,
                along_mm: 600.0,
            },
            &[],
        );
        for _ in 0..400 {
            sim.step(0.1);
        }
        let ic = &sim.trains[0];
        assert_eq!(ic.state, TrainState::Waiting);
        assert_eq!(ic.head.piece, PieceId::new("timo-2e96-border", "1"));
        assert!(ic.head.along_mm <= 850.0 - SIGNAL_MARGIN_MM + 1e-6);
        assert_eq!(sim.aspects["timo-2e96-border/sig_1"], "red");

        sim.trains.retain(|t| t.name != "Lok 5");
        let mut crossed = false;
        for _ in 0..200 {
            crossed |= sim
                .step(0.1)
                .iter()
                .any(|(_, e)| matches!(e, SimEvent::Border { train, .. } if train == "IC 2001"));
        }
        assert!(crossed);
    }

    /// The border signal clears for the IC as its route is set beyond it (caution first,
    /// while the next signal is still at stop, then proceed) and falls back to stop
    /// once the train has passed.
    #[test]
    fn a_signal_clears_ahead_of_the_train_and_drops_behind_it() {
        let events = run(30.0);
        let aspects: Vec<(f64, &str)> = events
            .iter()
            .filter_map(|(t, e)| match e {
                SimEvent::Signal { signal, aspect, .. } if signal == "timo-2e96-border/sig_1" => {
                    Some((*t, aspect.as_str()))
                }
                _ => None,
            })
            .collect();
        let order: Vec<&str> = aspects.iter().map(|(_, a)| *a).collect();
        assert_eq!(order, ["red", "yellow", "green", "red"], "{aspects:?}");
        let border = events
            .iter()
            .find(|(_, e)| matches!(e, SimEvent::Border { train, .. } if train == "IC 2001"))
            .unwrap()
            .0;
        assert!(
            aspects[2].0 < border && aspects[3].0 >= border,
            "{aspects:?}"
        );
    }

    /// The transfer block on Windows' side of the border is fed by MacBook while the IC
    /// comes in, by Windows once the train is fully inside, by MacBook again once free.
    #[test]
    fn a_transfer_block_changes_feed_when_the_train_is_inside() {
        let events = run(30.0);
        let feeds: Vec<&str> = events
            .iter()
            .filter_map(|(_, e)| match e {
                SimEvent::Feed { block, district } if block == "anna-2e96-border/t1" => {
                    Some(district.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(feeds, ["Windows", "MacBook"]);
        let layout = layout();
        let topology = Topology::build(&layout);
        let sim = LayoutSim::new(&layout, &topology);
        assert_eq!(sim.transfers["anna-2e96-border/t1"].fed_by, "MacBook");
        assert_eq!(sim.transfers["timo-2e96-border/t2"].source, "Windows");
    }

    /// A short in one district stops the trains there; the others run on, and the stopped
    /// train goes on when the power is back.
    #[test]
    fn a_district_without_power_stops_only_its_trains() {
        let layout = layout();
        let topology = Topology::build(&layout);
        let mut sim = LayoutSim::new(&layout, &topology);
        example_trains(&mut sim);
        let district_of = |sim: &LayoutSim, i: usize| {
            topology
                .piece(&sim.trains[i].head.piece)
                .unwrap()
                .district
                .clone()
        };
        // Run until the trains are in different districts.
        let mut steps = 0;
        while district_of(&sim, 0) == district_of(&sim, 1) && steps < 2000 {
            sim.step(0.1);
            steps += 1;
        }
        let (a, b) = (district_of(&sim, 0), district_of(&sim, 1));
        assert_ne!(a, b);
        sim.district_power.insert(a.clone(), "SHORT".to_owned());
        let events = sim.step(0.1);
        assert!(events.iter().any(
            |(_, e)| matches!(e, SimEvent::Power { powered: false, district, .. } if *district == a)
        ));
        assert_eq!(sim.trains[0].state, TrainState::NoPower);
        let (held, other) = (sim.trains[0].head.clone(), sim.trains[1].head.clone());
        for _ in 0..30 {
            sim.step(0.1);
        }
        assert_eq!(sim.trains[0].head.piece, held.piece);
        assert_eq!(
            sim.trains[0].head.along_mm, held.along_mm,
            "the train in {a} must stand"
        );
        assert!(
            sim.trains[1].head.piece != other.piece
                || sim.trains[1].head.along_mm != other.along_mm,
            "the train in {b} runs on"
        );
        sim.district_power.insert(a.clone(), "ON".to_owned());
        let events = sim.step(0.1);
        assert!(events
            .iter()
            .any(|(_, e)| matches!(e, SimEvent::Power { powered: true, .. })));
        assert_ne!(sim.trains[0].state, TrainState::NoPower);
    }

    /// An hour on the three-district bench with Cdorf's passing sidings: all six trains keep
    /// going (no deadlock), never touch, the sidings get used, and slower trains wait in them
    /// to let faster ones by.
    #[test]
    fn six_trains_run_an_hour_on_the_bench_with_sidings() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/layout");
        let layout = crate::layout::load(&dir.join("bench-m5.json"), &dir).unwrap();
        let topology = Topology::build(&layout);
        let mut sim = LayoutSim::new(&layout, &topology);
        example_trains(&mut sim);
        let mut arrivals: BTreeMap<String, usize> = BTreeMap::new();
        let mut sidings = 0;
        let mut yields = 0;
        for _ in 0..36_000 {
            for (_, event) in sim.step(0.1) {
                match event {
                    SimEvent::Arrived { train, .. } => *arrivals.entry(train).or_default() += 1,
                    SimEvent::Yield { .. } => yields += 1,
                    SimEvent::Block {
                        block,
                        occupied: true,
                        ..
                    } if block.starts_with("timo-cdorf")
                        && (block.ends_with("_3") || block.ends_with("_4")) =>
                    {
                        sidings += 1
                    }
                    _ => {}
                }
            }
            let covered: Vec<BTreeSet<PieceId>> = sim
                .trains
                .iter()
                .map(|t| sim.covered(t).into_iter().map(|(p, _, _)| p).collect())
                .collect();
            for i in 0..covered.len() {
                for j in i + 1..covered.len() {
                    assert!(
                        covered[i].is_disjoint(&covered[j]),
                        "trains collide at {:.1} s",
                        sim.time_s
                    );
                }
            }
        }
        for train in sim.trains.iter().map(|t| t.name.as_str()) {
            let count = arrivals.get(train).copied().unwrap_or(0);
            assert!(
                count >= 4,
                "{train} arrived only {count} times in an hour: {arrivals:?}"
            );
        }
        assert!(sidings > 0, "no train used a passing siding");
        assert!(yields > 0, "no slower train let a faster one by");
    }
}
