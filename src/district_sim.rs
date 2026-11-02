//! The district simulator: one district's hardware, without its logic. It plays what a
//! Hive-Rail node board and the track under it would do, and nothing more (wiki "District
//! Simulators and Train Tracking"). The layout simulator (`layout_sim.rs`) stays beside it:
//! that one plays the whole layout, logic included.
//!
//! **Bodies, not trains.** A body is a loco with its cars: a decoder address, a length and
//! a top speed. It moves at the speed its decoder holds, along the turnouts as they lie,
//! not along a route. A decoder keeps its last speed until the command station of the
//! track it stands on tells it otherwise, as DCC decoders do; without track power it
//! stops. The simulator knows the decoder address (it is part of the loco) but never
//! reports it: what comes out is only what real hardware reports, the occupancy of a block
//! by its detector, a turnout's position, a track overload.
//!
//! **Faults** are what the logic must prevent: two bodies running into each other, a
//! body running through a turnout set against it, a body running into a buffer stop. The
//! first two short the track (a derailment), as a real one does.

use crate::layout::{Layout, ModuleFile};
use crate::layout_sim::Place;
use crate::topology::{occupancy_word, position_word, Heading, Leg, PieceId, Topology, Trail};
use std::collections::{BTreeMap, BTreeSet};

/// The H0 scale, for speeds in scale km/h.
const SCALE: f64 = 87.0;

/// Scale km/h to mm/s on the model.
pub fn mm_s(scale_kmh: f64) -> f64 {
    scale_kmh / 3.6 * 1000.0 / SCALE
}

/// Model mm/s to scale km/h.
pub fn scale_kmh(mm_s: f64) -> f64 {
    mm_s * SCALE * 3.6 / 1000.0
}

/// What a decoder holds: the last speed step (0 to 126) and direction it was given.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Decoder {
    pub speed: u8,
    pub forward: bool,
}

#[derive(Debug, Clone)]
pub struct Body {
    /// The simulator's own name for the body, kept across districts; never the loco's.
    pub id: String,
    /// How often the body was handed to another district's simulator: a copy with fewer is
    /// out of date.
    pub hops: u32,
    pub address: u16,
    pub length_mm: f64,
    /// At speed step 126.
    pub top_mm_s: f64,
    /// The end running first; with `front_first` the loco's front, else its rear.
    pub head: Place,
    /// The pieces behind the head, oldest first; the head's own piece is not in it.
    trail: Trail,
    /// Whether the head is the loco's front: then forward on the decoder runs head first.
    front_first: bool,
    pub decoder: Decoder,
    /// Why the body cannot move on, if it is held: a buffer stop, the border, a fault.
    pub held: Option<String>,
}

impl Body {
    pub fn speed_mm_s(&self) -> f64 {
        self.top_mm_s * f64::from(self.decoder.speed) / 126.0
    }

    /// The stretches of track it covers, head first (`Topology::spans`).
    pub fn spans(&self, topology: &Topology) -> Vec<(PieceId, Heading, f64, f64)> {
        let head = &self.head;
        topology.spans(
            &head.piece,
            head.heading,
            head.along_mm,
            &self.trail,
            self.length_mm,
        )
    }

    /// Where its leading end is when its decoder runs it `forward`: the head when that runs
    /// it head first, else the tail end (the body turned round).
    pub fn leading_end(
        &self,
        topology: &Topology,
        forward: bool,
    ) -> Option<(PieceId, Heading, f64)> {
        let head = &self.head;
        if forward == self.front_first {
            return Some((head.piece.clone(), head.heading, head.along_mm));
        }
        let (piece, heading, along, _) = topology.turned(
            &head.piece,
            head.heading,
            head.along_mm,
            &self.trail,
            self.length_mm,
        )?;
        Some((piece, heading, along))
    }

    pub fn to_wire(&self, owner: &str) -> BodyWire {
        let place = |piece: &PieceId, heading: Heading| {
            (
                piece.module.clone(),
                piece.piece.clone(),
                heading == Heading::Forward,
            )
        };
        let (module, piece, forward) = place(&self.head.piece, self.head.heading);
        BodyWire {
            id: self.id.clone(),
            hops: self.hops,
            owner: owner.to_owned(),
            address: self.address,
            length_mm: self.length_mm,
            top_mm_s: self.top_mm_s,
            head: (module, piece, forward, self.head.along_mm),
            trail: self.trail.iter().map(|(p, h)| place(p, *h)).collect(),
            front_first: self.front_first,
            speed: self.decoder.speed,
            forward: self.decoder.forward,
            from: None,
        }
    }

    pub fn from_wire(wire: &BodyWire) -> Self {
        let heading = |forward: bool| {
            if forward {
                Heading::Forward
            } else {
                Heading::Backward
            }
        };
        let (module, piece, forward, along_mm) = &wire.head;
        Body {
            id: wire.id.clone(),
            hops: wire.hops,
            address: wire.address,
            length_mm: wire.length_mm,
            top_mm_s: wire.top_mm_s,
            head: Place {
                piece: PieceId::new(module, piece),
                heading: heading(*forward),
                along_mm: *along_mm,
            },
            trail: wire
                .trail
                .iter()
                .map(|(m, p, f)| (PieceId::new(m, p), heading(*f)))
                .collect(),
            front_first: wire.front_first,
            decoder: Decoder {
                speed: wire.speed,
                forward: wire.forward,
            },
            held: None,
        }
    }
}

/// A body on the world channel between district simulators (`hive-sim/<session>/world/`,
/// which Hive-Rail never reads): where it is, and the district whose simulator moves it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BodyWire {
    pub id: String,
    pub hops: u32,
    pub owner: String,
    pub address: u16,
    pub length_mm: f64,
    pub top_mm_s: f64,
    /// Module, piece, forward, along.
    pub head: (String, String, bool, f64),
    pub trail: Vec<(String, String, bool)>,
    pub front_first: bool,
    pub speed: u8,
    pub forward: bool,
    /// On a handover: the district it comes from.
    #[serde(default)]
    pub from: Option<String>,
}

/// A device on a bus: its bus (`loconet`, `dcc`, `dccex`, `board`) and address.
pub type BusAddress = (String, u16);

/// What happened in the district, as its hardware shows it.
#[derive(Debug, Clone, PartialEq)]
pub enum WorldEvent {
    /// A block's detector reports occupied or free.
    Occupancy {
        block: String,
        detector: Option<BusAddress>,
        occupied: bool,
    },
    /// A turnout moved (closed: straight).
    Turnout { turnout: String, closed: bool },
    /// A signal's decoder was set: Hp 1 (green) or Hp 0 (red).
    Signal { signal: String, proceed: bool },
    /// Something the logic must prevent; `short` when it shorts the track.
    Fault { what: String, short: bool },
    /// A body's head reached the end of this district's track, towards `district`, and
    /// waits there: no simulator for that district is on the world channel.
    Border { district: String, piece: PieceId },
    /// A body's head crossed into `to`, whose simulator moves it from now on.
    Handover { body: BodyWire, to: String },
    /// A body came in from a neighbouring district: this simulator moves it now.
    Arrived { id: String, from: String },
    /// A body stopped at a buffer stop or the end of the track, at no speed.
    Stopped { what: String },
}

pub struct DistrictWorld<'a> {
    layout: &'a Layout,
    topology: &'a Topology,
    pub district: String,
    pub bodies: Vec<Body>,
    /// This district's turnouts by `module/turnout`: closed is straight.
    pub turnouts: BTreeMap<String, bool>,
    /// The turnout each switching device's bus address moves.
    turnout_devices: BTreeMap<BusAddress, String>,
    /// This district's signals by their decoder's bus address, and what each shows.
    signals: BTreeMap<BusAddress, String>,
    pub aspects: BTreeMap<String, bool>,
    /// Each block's detector, by `module/block`.
    detectors: BTreeMap<String, BusAddress>,
    /// This district's blocks on each piece, with their cuts: for the occupancy.
    blocks_on: BTreeMap<PieceId, Vec<(String, [u32; 2])>>,
    occupied: BTreeSet<String>,
    /// Track power from the district's command station; off while shorted.
    pub power: bool,
    pub shorted: bool,
    /// What the command station keeps sending on the track, by decoder address: a decoder
    /// on this district's track takes it on every step, as DCC refreshes its locos.
    refresh: BTreeMap<u16, Decoder>,
    /// Bodies moved by a neighbour's simulator, as last heard, by id, with their owner:
    /// for the occupancy and collisions on this district's track.
    pub foreign: BTreeMap<String, (Body, String)>,
    /// The neighbouring districts whose simulator is on the world channel now; a body only
    /// crosses into one of them.
    pub live_neighbours: BTreeSet<String>,
    next_id: u32,
    pub time_s: f64,
}

impl<'a> DistrictWorld<'a> {
    pub fn new(layout: &'a Layout, topology: &'a Topology, district: &str) -> Result<Self, String> {
        if !layout.file.districts.iter().any(|d| d.node == district) {
            let names: Vec<&str> = layout
                .file
                .districts
                .iter()
                .map(|d| d.node.as_str())
                .collect();
            return Err(format!(
                "no district {district} in the layout (districts: {})",
                names.join(", ")
            ));
        }
        let mut turnouts = BTreeMap::new();
        let mut turnout_devices = BTreeMap::new();
        for turnout in topology.turnouts_in(district) {
            turnouts.insert(turnout.key(), true);
            if let Some(device) = topology.drive_of(turnout) {
                turnout_devices.insert((device.bus.clone(), device.address), turnout.key());
            }
        }
        let signals = topology
            .devices_in(district)
            .filter(|d| d.kind == "signal")
            .map(|d| ((d.bus.clone(), d.address), d.key()))
            .collect();
        let detectors = topology
            .devices_in(district)
            .filter_map(|d| Some((d.block.clone()?, (d.bus.clone(), d.address))))
            .collect();
        let mut blocks_on: BTreeMap<PieceId, Vec<(String, [u32; 2])>> = BTreeMap::new();
        for (key, block) in topology.blocks_in(district) {
            blocks_on
                .entry(block.piece.clone())
                .or_default()
                .push((key.clone(), block.cuts_mm));
        }
        Ok(Self {
            layout,
            topology,
            blocks_on,
            signals,
            aspects: BTreeMap::new(),
            district: district.to_owned(),
            bodies: Vec::new(),
            turnouts,
            turnout_devices,
            detectors,
            occupied: BTreeSet::new(),
            power: false,
            shorted: false,
            refresh: BTreeMap::new(),
            foreign: BTreeMap::new(),
            live_neighbours: BTreeSet::new(),
            next_id: 1,
            time_s: 0.0,
        })
    }

    fn module(&self, id: &str) -> Option<&ModuleFile> {
        self.layout.modules.get(id).map(|(module, _)| module)
    }

    fn length(&self, piece: &PieceId) -> f64 {
        self.topology
            .piece(piece)
            .map_or(0.0, |p| f64::from(p.length_mm))
    }

    fn district_of(&self, piece: &PieceId) -> Option<&str> {
        self.topology.piece(piece).map(|p| p.district.as_str())
    }

    fn here(&self, piece: &PieceId) -> bool {
        self.district_of(piece) == Some(self.district.as_str())
    }

    /// A turnout's position, for the track graph: every turnout of every district (other
    /// districts' turnouts are unknown here, and only matter at the border).
    fn position(&self, module: &str, turnout: &str) -> Option<bool> {
        self.turnouts.get(&format!("{module}/{turnout}")).copied()
    }

    /// Puts a body with its head at `head`, its front first, its cars behind it along the
    /// track as the turnouts lie. The head must be on this district's track.
    pub fn add_body(
        &mut self,
        address: u16,
        length_mm: f64,
        top_mm_s: f64,
        head: Place,
    ) -> Result<(), String> {
        if !self.here(&head.piece) {
            return Err(format!(
                "{} is not in district {}",
                head.piece, self.district
            ));
        }
        let length = self.length(&head.piece);
        let head = Place {
            along_mm: head.along_mm.clamp(0.0, length),
            ..head
        };
        // Behind the head: back along the track, piece by piece, until the length fits.
        let mut trail = Vec::new();
        let mut remaining = length_mm - head.along_mm;
        let (mut piece, mut heading) = (head.piece.clone(), head.heading);
        while remaining > 0.0 {
            let Some((back, back_heading)) = self
                .topology
                .next(&piece, heading.reversed(), |m, t| self.position(m, t))
                .into_iter()
                .next()
            else {
                return Err(format!(
                    "body {address} ({length_mm} mm) does not fit behind {}",
                    head.piece
                ));
            };
            heading = back_heading.reversed();
            piece = back;
            trail.push((piece.clone(), heading));
            remaining -= self.length(&piece);
        }
        trail.reverse();
        let body = Body {
            id: format!("{}-{}", self.district, self.next_id),
            hops: 0,
            address,
            length_mm,
            top_mm_s,
            head,
            trail,
            front_first: true,
            decoder: Decoder {
                speed: 0,
                forward: true,
            },
            held: None,
        };
        let others = self.bodies.len();
        self.bodies.push(body);
        if (0..others).any(|other| self.touches(others, other)) {
            self.bodies.pop();
            return Err(format!("body {address} would stand on another body"));
        }
        self.next_id += 1;
        self.occupied = self.occupancy();
        Ok(())
    }

    // ---- What the command station does: DCC-EX commands, as the node sends them.

    pub fn power_on(&mut self) {
        self.power = true;
        self.shorted = false;
    }

    pub fn power_off(&mut self) {
        self.power = false;
    }

    /// `<!>`: every loco DCC-EX drives to an emergency stop; track power stays on.
    pub fn emergency_stop(&mut self) {
        for decoder in self.refresh.values_mut() {
            decoder.speed = 0;
        }
        self.apply_refresh();
    }

    /// `<t cab speed dir>`: speed 0 to 126, or -1 for an emergency stop.
    pub fn drive(&mut self, address: u16, speed: i16, forward: bool) {
        let speed = u8::try_from(speed.clamp(0, 126)).unwrap_or(0);
        self.refresh.insert(address, Decoder { speed, forward });
        self.apply_refresh();
    }

    /// `<- cab>`: DCC-EX stops the loco (an emergency stop) and stops refreshing it.
    pub fn forget(&mut self, address: u16) {
        if let Some(decoder) = self.refresh.get_mut(&address) {
            decoder.speed = 0;
        }
        self.apply_refresh();
        self.refresh.remove(&address);
    }

    /// A switching device on a bus: a DCC accessory decoder (`<a address 1|0>`, closed 1)
    /// or a LocoNet one (`OPC_SW_REQ`). Moves its turnout, if it is one of this district's.
    pub fn switch(&mut self, bus: &str, address: u16, closed: bool) -> Option<WorldEvent> {
        let device = (bus.to_owned(), address);
        if let Some(signal) = self.signals.get(&device).cloned() {
            // Closed (green) is Hp 1, thrown (red) Hp 0, as a two-aspect decoder takes it.
            return (self.aspects.insert(signal.clone(), closed) != Some(closed)).then_some(
                WorldEvent::Signal {
                    signal,
                    proceed: closed,
                },
            );
        }
        let key = self.turnout_devices.get(&device)?.clone();
        let position = self.turnouts.get_mut(&key)?;
        if *position == closed {
            return None;
        }
        *position = closed;
        Some(WorldEvent::Turnout {
            turnout: key,
            closed,
        })
    }

    /// Decoders on this district's powered track take what the command station sends.
    fn apply_refresh(&mut self) {
        if !self.power {
            return;
        }
        for index in 0..self.bodies.len() {
            if !self.here(&self.bodies[index].head.piece) {
                continue;
            }
            let address = self.bodies[index].address;
            if let Some(decoder) = self.refresh.get(&address) {
                self.bodies[index].decoder = *decoder;
            }
        }
    }

    // ---- The track.

    /// The stretches of track a body covers, head first, per piece: (piece, heading, from,
    /// to), in mm from the piece's `from` point.
    pub fn covered(&self, body: &Body) -> Vec<(PieceId, Heading, f64, f64)> {
        body.spans(self.topology)
    }

    /// Whether a body covers track of another district, so the neighbour must know of it.
    pub fn spans_border(&self, body: &Body) -> bool {
        self.covered(body)
            .iter()
            .any(|(piece, ..)| !self.here(piece))
    }

    /// Whether two of this district's bodies overlap on the track.
    fn touches(&self, a: usize, b: usize) -> bool {
        self.overlap(&self.bodies[a], &self.bodies[b])
    }

    fn overlap(&self, a: &Body, b: &Body) -> bool {
        let spans_b = self.covered(b);
        self.covered(a).iter().any(|(piece, _, a0, a1)| {
            spans_b
                .iter()
                .any(|(other, _, b0, b1)| other == piece && a0 < b1 && b0 < a1)
        })
    }

    /// The blocks of this district a body covers now.
    fn occupancy(&self) -> BTreeSet<String> {
        let mut occupied = BTreeSet::new();
        for body in self
            .bodies
            .iter()
            .chain(self.foreign.values().map(|(body, _)| body))
        {
            for (piece, _, a, b) in self.covered(body) {
                for (key, [c0, c1]) in self.blocks_on.get(&piece).into_iter().flatten() {
                    if a < f64::from(*c1) && b > f64::from(*c0) {
                        occupied.insert(key.clone());
                    }
                }
            }
        }
        occupied
    }

    /// The blocks occupied now, and their detectors.
    #[cfg(test)]
    pub fn occupied(&self) -> impl Iterator<Item = (&String, Option<&BusAddress>)> {
        self.occupied
            .iter()
            .map(|block| (block, self.detectors.get(block)))
    }

    /// Every block of this district with a detector, and whether it is occupied now: what
    /// the detectors would all report if asked.
    pub fn detector_states(&self) -> Vec<(String, BusAddress, bool)> {
        self.detectors
            .iter()
            .map(|(block, detector)| {
                (
                    block.clone(),
                    detector.clone(),
                    self.occupied.contains(block),
                )
            })
            .collect()
    }

    // ---- Time.

    pub fn step(&mut self, dt_s: f64) -> Vec<WorldEvent> {
        let mut events = Vec::new();
        self.time_s += dt_s;
        self.apply_refresh();
        let mut handovers = Vec::new();
        if self.power {
            for index in 0..self.bodies.len() {
                if self.here(&self.bodies[index].head.piece) {
                    if let Some(to) = self.move_body(index, dt_s, &mut events) {
                        handovers.push((index, to));
                    }
                }
            }
        }
        // Bodies whose head crossed into a neighbour: its simulator moves them from now on;
        // this one keeps a copy for what is still on its track.
        for (index, to) in handovers.into_iter().rev() {
            let mut body = self.bodies.remove(index);
            body.hops += 1;
            events.push(WorldEvent::Handover {
                body: BodyWire {
                    from: Some(self.district.clone()),
                    ..body.to_wire(&to)
                },
                to: to.clone(),
            });
            self.foreign.insert(body.id.clone(), (body, to));
        }
        let occupied = self.occupancy();
        for block in occupied.symmetric_difference(&self.occupied) {
            events.push(WorldEvent::Occupancy {
                block: block.clone(),
                detector: self.detectors.get(block).cloned(),
                occupied: occupied.contains(block),
            });
        }
        self.occupied = occupied;
        events
    }

    /// Turns a body round when its decoder's direction runs it the other way: the tail
    /// end becomes the head.
    fn face(&mut self, index: usize) {
        let body = &self.bodies[index];
        if body.decoder.forward == body.front_first {
            return;
        }
        let head = &body.head;
        let Some((piece, heading, along_mm, trail)) = self.topology.turned(
            &head.piece,
            head.heading,
            head.along_mm,
            &body.trail,
            body.length_mm,
        ) else {
            return;
        };
        let body = &mut self.bodies[index];
        body.trail = trail;
        body.head = Place {
            piece,
            heading,
            along_mm,
        };
        body.front_first = !body.front_first;
        body.held = None;
    }

    /// Moves a body for `dt_s`; returns the neighbouring district its head crossed into.
    fn move_body(
        &mut self,
        index: usize,
        dt_s: f64,
        events: &mut Vec<WorldEvent>,
    ) -> Option<String> {
        self.face(index);
        let mut travel = self.bodies[index].speed_mm_s() * dt_s;
        if travel <= 0.0 {
            return None;
        }
        loop {
            let body = &self.bodies[index];
            let length = self.length(&body.head.piece);
            if body.head.along_mm + travel <= length {
                self.bodies[index].head.along_mm += travel;
                if let Some(fault) = self.collision(index) {
                    self.fault(fault, true, events);
                }
                break;
            }
            // The head reaches the end of its piece.
            let rest = body.head.along_mm + travel - length;
            let (piece, heading) = (body.head.piece.clone(), body.head.heading);
            let next = self
                .topology
                .next(&piece, heading, |m, t| self.position(m, t))
                .into_iter()
                .next();
            let Some((next, next_heading)) = next else {
                self.bodies[index].head.along_mm = length;
                self.blocked(index, &piece, heading, events);
                break;
            };
            if !self.here(&next) {
                let district = self.district_of(&next).unwrap_or("").to_owned();
                if self.live_neighbours.contains(&district) {
                    let along = rest.min(self.length(&next));
                    let body = &mut self.bodies[index];
                    body.trail.push((piece, heading));
                    body.head = Place {
                        piece: next,
                        heading: next_heading,
                        along_mm: along,
                    };
                    body.held = None;
                    self.trim_trail(index);
                    return Some(district);
                }
                self.bodies[index].head.along_mm = length;
                if self.bodies[index].held.is_none() {
                    self.bodies[index].held = Some(format!("at the border to {district}"));
                    events.push(WorldEvent::Border {
                        district,
                        piece: next,
                    });
                }
                break;
            }
            let body = &mut self.bodies[index];
            body.trail.push((piece, heading));
            body.head = Place {
                piece: next,
                heading: next_heading,
                along_mm: 0.0,
            };
            body.held = None;
            travel = rest;
        }
        self.trim_trail(index);
        None
    }

    /// The track ends ahead of a body: a turnout set against it (it runs through and
    /// derails), a buffer stop, or the open end of the layout.
    fn blocked(
        &mut self,
        index: usize,
        piece: &PieceId,
        heading: Heading,
        events: &mut Vec<WorldEvent>,
    ) {
        if self.bodies[index].held.is_some() {
            return;
        }
        let address = self.bodies[index].address;
        if let Some(turnout) = self.topology.turnout_ahead(piece, heading) {
            if turnout.entry != Leg::Trunk {
                let what = format!(
                    "loco {address} ran through turnout {}/{} set against it, from its {:?} leg, and derailed",
                    turnout.module, turnout.id, turnout.entry
                );
                self.bodies[index].held = Some("derailed".to_owned());
                self.fault(what, true, events);
                return;
            }
        }
        let kmh = scale_kmh(self.bodies[index].speed_mm_s());
        self.bodies[index].held = Some("at the end of the track".to_owned());
        let what = format!("loco {address} hit the end of track {piece} at {kmh:.0} km/h");
        if kmh > 15.0 {
            self.fault(what, false, events);
        } else {
            events.push(WorldEvent::Stopped { what });
        }
    }

    /// Another body this one runs into, as a fault.
    fn collision(&mut self, index: usize) -> Option<String> {
        let foreign = self
            .foreign
            .values()
            .find(|(body, _)| self.overlap(&self.bodies[index], body))
            .map(|(body, owner)| (body.address, owner.clone()));
        if let Some((b, owner)) = foreign {
            let a = self.bodies[index].address;
            self.bodies[index].head.along_mm = (self.bodies[index].head.along_mm - 1.0).max(0.0);
            self.bodies[index].held = Some(format!("ran into loco {b}"));
            return Some(format!(
                "loco {a} ran into loco {b} (moved by {owner}'s simulator)"
            ));
        }
        let other =
            (0..self.bodies.len()).find(|&other| other != index && self.touches(index, other))?;
        let (a, b) = (self.bodies[index].address, self.bodies[other].address);
        // Back to where it touched, more or less: the bodies stand against each other.
        self.bodies[index].head.along_mm = (self.bodies[index].head.along_mm - 1.0).max(0.0);
        self.bodies[index].held = Some(format!("ran into loco {b}"));
        self.bodies[other].held = Some(format!("hit by loco {a}"));
        Some(format!("loco {a} ran into loco {b}"))
    }

    fn fault(&mut self, what: String, short: bool, events: &mut Vec<WorldEvent>) {
        if short {
            self.power = false;
            self.shorted = true;
        }
        events.push(WorldEvent::Fault { what, short });
    }

    /// A body heard on the world channel: one handed to this district (its owner is this
    /// district and it has made one more hop than the copy here), or a neighbour's body,
    /// kept for the occupancy and collisions on this district's track. Out-of-date copies,
    /// including this simulator's own publications coming back, are dropped.
    pub fn receive(&mut self, wire: &BodyWire) -> Option<WorldEvent> {
        if self.bodies.iter().any(|body| body.id == wire.id) {
            return None; // moved here; any other copy is older
        }
        let known = self
            .foreign
            .get(&wire.id)
            .map(|(body, owner)| (body.hops, owner.clone()));
        if known.as_ref().is_some_and(|(hops, _)| wire.hops < *hops) {
            return None;
        }
        if wire.owner == self.district {
            let newer = known.as_ref().is_none_or(|(hops, _)| wire.hops > *hops);
            let body = Body::from_wire(wire);
            if !newer || !self.here(&body.head.piece) {
                return None;
            }
            self.foreign.remove(&wire.id);
            self.bodies.push(body);
            let from = wire
                .from
                .clone()
                .or(known.map(|(_, owner)| owner))
                .unwrap_or_else(|| "a neighbour".to_owned());
            return Some(WorldEvent::Arrived {
                id: wire.id.clone(),
                from,
            });
        }
        self.foreign
            .insert(wire.id.clone(), (Body::from_wire(wire), wire.owner.clone()));
        None
    }

    /// A neighbour's body no longer heard of (its simulator stopped).
    pub fn forget_foreign(&mut self, id: &str) {
        self.foreign.remove(id);
    }

    /// Keeps only the trail pieces the body still covers.
    fn trim_trail(&mut self, index: usize) {
        let covered = self.covered(&self.bodies[index]).len();
        let body = &mut self.bodies[index];
        let keep = covered.saturating_sub(1);
        if body.trail.len() > keep {
            let drop = body.trail.len() - keep;
            body.trail.drain(..drop);
        }
    }

    /// The modules of this district.
    pub fn modules(&self) -> Vec<&str> {
        self.layout
            .file
            .districts
            .iter()
            .filter(|d| d.node == self.district)
            .flat_map(|d| d.modules.iter().map(String::as_str))
            .filter(|m| self.module(m).is_some())
            .collect()
    }
}

// ---- `--sim-district`: the world as a Hive-Rail node board on the network.

/// DCC-EX's TCP port, as on its WiFi.
const DCCEX_PORT: u16 = 2560;
/// The node board's LocoNet buffer (LbServer).
const LOCONET_PORT: u16 = 1234;
const TICK: std::time::Duration = std::time::Duration::from_millis(50);
/// How often the bodies are listed in the log.
const STATUS_EVERY_S: f64 = 10.0;

/// One `--body` option: `<address>@<module>/<piece>[,length=600][,kmh=100]
/// [,heading=forward|backward][,along=<mm>]`.
struct BodySpec {
    address: u16,
    piece: PieceId,
    length_mm: f64,
    kmh: f64,
    heading: Heading,
    along_mm: Option<f64>,
}

fn parse_body(text: &str) -> Result<BodySpec, String> {
    let bad = || {
        format!("--body {text}: expected <address>@<module>/<piece>[,length=600][,kmh=100][,heading=forward|backward][,along=<mm>]")
    };
    let mut parts = text.split(',');
    let (address, place) = parts
        .next()
        .and_then(|p| p.split_once('@'))
        .ok_or_else(bad)?;
    let (module, piece) = place.split_once('/').ok_or_else(bad)?;
    let mut spec = BodySpec {
        address: address.parse().map_err(|_| bad())?,
        piece: PieceId::new(module, piece),
        length_mm: 600.0,
        kmh: 100.0,
        heading: Heading::Forward,
        along_mm: None,
    };
    for part in parts {
        let (key, value) = part.split_once('=').ok_or_else(bad)?;
        match key {
            "length" => spec.length_mm = value.parse().map_err(|_| bad())?,
            "kmh" => spec.kmh = value.parse().map_err(|_| bad())?,
            "along" => spec.along_mm = Some(value.parse().map_err(|_| bad())?),
            "heading" => {
                spec.heading = match value {
                    "forward" => Heading::Forward,
                    "backward" => Heading::Backward,
                    _ => return Err(bad()),
                }
            }
            _ => return Err(bad()),
        }
    }
    Ok(spec)
}

fn log(line: impl std::fmt::Display) {
    println!("{} {line}", chrono::Local::now().format("%H:%M:%S"));
}

/// What a switch command moved, for the log: a turnout or a signal.
fn log_switched(event: Option<WorldEvent>, source: &str) {
    match event {
        Some(WorldEvent::Turnout { turnout, closed }) => log(format!(
            "turnout {turnout} {} ({source})",
            position_word(closed)
        )),
        Some(WorldEvent::Signal { signal, proceed }) => log(format!(
            "signal {signal} shows {} ({source})",
            if proceed { "Hp 1" } else { "Hp 0" }
        )),
        _ => {}
    }
}

/// DCC-EX's broadcast of a loco's state: `<l cab register speedByte functions>`, the speed
/// byte 0 stop, 1 emergency stop, else the step plus one, with 128 for forward.
fn loco_broadcast(address: u16, speed: i16, forward: bool) -> String {
    let step = match speed {
        s if s < 0 => 1,
        0 => 0,
        s => s + 1,
    };
    format!("<l {address} 0 {} 0>", step + if forward { 128 } else { 0 })
}

/// Answers one DCC-EX command on the world, with what DCC-EX would send back.
fn dccex_command(world: &mut DistrictWorld, frame: &str) -> Vec<String> {
    let words: Vec<&str> = frame.split_whitespace().collect();
    let power = |on: bool| format!("<p{} A>", u8::from(on));
    match words.as_slice() {
        ["s"] => vec![
            format!(
                "<iDCC-EX V-5.6.6 / Hive-Rail district simulator / {} G-sim>",
                world.district
            ),
            power(world.power),
        ],
        ["="] => vec!["<= A MAIN>".into(), "<= B PROG>".into()],
        ["U", "0"] => vec!["<X>".into()], // a stock DCC-EX: no Hive-Rail board add-on
        ["1", ..] => {
            let was_shorted = world.shorted;
            world.power_on();
            log(format!(
                "track power ON{}",
                if was_shorted { " (short cleared)" } else { "" }
            ));
            let mut replies = vec![power(true)];
            if was_shorted {
                replies.push("<* TRACK A NORMAL *>".into());
            }
            replies
        }
        ["0", ..] => {
            world.power_off();
            log("track power OFF");
            vec![power(false)]
        }
        ["!"] => {
            world.emergency_stop();
            log("emergency stop: every loco to speed 0");
            Vec::new()
        }
        ["t", cab, speed, dir] => {
            let (Ok(cab), Ok(speed)) = (cab.parse::<u16>(), speed.parse::<i16>()) else {
                return Vec::new();
            };
            let forward = *dir == "1";
            world.drive(cab, speed, forward);
            vec![loco_broadcast(cab, speed, forward)]
        }
        ["-", cab] => {
            if let Ok(cab) = cab.parse() {
                world.forget(cab);
            }
            Vec::new()
        }
        ["a", address, activate] => {
            if let Ok(address) = address.parse() {
                log_switched(
                    world.switch("dcc", address, *activate == "1"),
                    &format!("DCC accessory {address}"),
                );
            }
            Vec::new()
        }
        _ => Vec::new(), // the fast clock, functions, the board status line
    }
}

/// `--sim-district <layout.json> --district <name> [--body …]… [--dccex-port 2560]
/// [--loconet-port 1234] [--speed 1]`.
pub async fn run(args: Vec<String>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::{broadcast, mpsc};

    let mut args = args.into_iter();
    let path = args
        .next()
        .ok_or("expected a layout file after --sim-district")?;
    let path = std::path::Path::new(&path);
    let (mut district, mut bodies, mut session) = (None, Vec::new(), None);
    let (mut dccex_port, mut loconet_port, mut speed) = (DCCEX_PORT, LOCONET_PORT, 1.0_f64);
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("expected a value after {flag}"))
        };
        match flag.as_str() {
            "--district" => district = Some(value()?),
            "--body" => bodies.push(parse_body(&value()?)?),
            "--dccex-port" => dccex_port = value()?.parse().map_err(|_| "invalid --dccex-port")?,
            "--loconet-port" => {
                loconet_port = value()?.parse().map_err(|_| "invalid --loconet-port")?
            }
            "--speed" => speed = value()?.parse().map_err(|_| "invalid --speed")?,
            "--session" => session = Some(value()?),
            _ => return Err(format!("unknown --sim-district option: {flag}").into()),
        }
    }
    let district = district.ok_or("--sim-district needs --district <name>")?;
    let layout = crate::layout::load(path, path.parent().unwrap_or(std::path::Path::new(".")))?;
    let report = layout.check();
    if !report.errors.is_empty() {
        return Err(format!(
            "the layout has errors; see --check-layout {}",
            path.display()
        )
        .into());
    }
    let topology = Topology::build(&layout);
    let mut world = DistrictWorld::new(&layout, &topology, &district)?;
    for body in &bodies {
        let length = topology
            .piece(&body.piece)
            .map_or(0.0, |p| f64::from(p.length_mm));
        let along = body.along_mm.unwrap_or(body.length_mm.min(length));
        let head = Place {
            piece: body.piece.clone(),
            heading: body.heading,
            along_mm: along,
        };
        world.add_body(body.address, body.length_mm, mm_s(body.kmh), head)?;
    }

    // The node board: DCC-EX on TCP, and its LocoNet buffer.
    let (lb, mut lb_events) = crate::lbserver::start(loconet_port).await?;
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", dccex_port)).await?;
    let (replies, _) = broadcast::channel::<String>(256);
    let (commands, mut command_events) = mpsc::channel::<String>(256);
    {
        let replies = replies.clone();
        tokio::spawn(async move {
            while let Ok((stream, peer)) = listener.accept().await {
                let (mut reader, mut writer) = stream.into_split();
                let commands = commands.clone();
                let mut outgoing = replies.subscribe();
                log(format!("DCC-EX client {peer} connected"));
                tokio::spawn(async move {
                    let mut decoder = crate::dccex::FrameDecoder::default();
                    let mut buffer = [0_u8; 1024];
                    loop {
                        tokio::select! {
                            read = reader.read(&mut buffer) => match read {
                                Ok(0) | Err(_) => break,
                                Ok(count) => {
                                    for frame in decoder.push(&String::from_utf8_lossy(&buffer[..count])) {
                                        if commands.send(frame).await.is_err() {
                                            return;
                                        }
                                    }
                                }
                            },
                            reply = outgoing.recv() => match reply {
                                Ok(reply) => if writer.write_all(format!("{reply}\r\n").as_bytes()).await.is_err() { break },
                                Err(broadcast::error::RecvError::Lagged(_)) => {}
                                Err(_) => break,
                            },
                        }
                    }
                    log(format!("DCC-EX client {peer} disconnected"));
                });
            }
        });
    }

    log(format!(
        "district simulator for {district} ({}): DCC-EX on port {dccex_port}, LocoNet buffer on port {loconet_port}, {} {}, {} turnouts, {} detectors",
        world.modules().join(", "),
        world.bodies.len(),
        if world.bodies.len() == 1 { "body" } else { "bodies" },
        world.turnouts.len(),
        world.detectors.len(),
    ));
    for ((bus, address), turnout) in &world.turnout_devices {
        log(format!("turnout {turnout}: {bus} switch {address}"));
    }
    log(format!(
        "join it with: --dccex 127.0.0.1:{dccex_port} --loconet-connect 127.0.0.1:{loconet_port} --loconet-connect-slots node"
    ));
    let report_detector =
        |detector: &Option<BusAddress>, block: &str, occupied: bool| match detector {
            Some((bus, address)) if bus == "loconet" => {
                lb.send(&crate::loconet::LocoNetMessage::input_report(
                    *address, occupied,
                ));
                log(format!(
                    "block {block} {} (LocoNet sensor {address})",
                    occupancy_word(occupied)
                ));
            }
            Some((bus, address)) => log(format!(
                "block {block} {}: its detector is on bus {bus} ({address}), not simulated",
                occupancy_word(occupied)
            )),
            None => log(format!(
                "block {block} {} (no detector)",
                occupancy_word(occupied)
            )),
        };

    // The world channel to the neighbouring districts' simulators, with --session: who is
    // there, the bodies each moves, and the bodies handed over at a border.
    let prefix = session
        .as_ref()
        .map(|session| format!("hive-sim/{session}/world"));
    let zenoh = match &prefix {
        Some(_) => Some(zenoh::open(crate::node::zenoh_config(None, None, false)?).await?),
        None => None,
    };
    let subscriber = match (&zenoh, &prefix) {
        (Some(zenoh), Some(prefix)) => {
            Some(zenoh.declare_subscriber(format!("{prefix}/**")).await?)
        }
        _ => None,
    };
    if let Some(prefix) = &prefix {
        log(format!(
            "world channel {prefix}: bodies cross into the districts whose simulator is on it"
        ));
    }
    let mut simulators: BTreeMap<String, std::time::Instant> = BTreeMap::new();
    let mut heard: BTreeMap<String, std::time::Instant> = BTreeMap::new();
    let mut next_hello = std::time::Instant::now();
    let mut published: BTreeSet<String> = BTreeSet::new();
    let mut next_truth = std::time::Instant::now();

    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut next_status = 0.0;
    loop {
        tokio::select! {
            _ = tick.tick() => {
                for event in world.step(TICK.as_secs_f64() * speed) {
                    match event {
                        WorldEvent::Occupancy { block, detector, occupied } => report_detector(&detector, &block, occupied),
                        event @ (WorldEvent::Turnout { .. } | WorldEvent::Signal { .. }) => log_switched(Some(event), "the simulator"),
                        WorldEvent::Fault { what, short } => {
                            log(format!("FAULT {what}{}", if short { ": the track shorts" } else { "" }));
                            if short {
                                let _ = replies.send(format!("<* TRACK A POWER OVERLOAD {what} *>"));
                            }
                        }
                        WorldEvent::Border { district, piece } => log(format!("a body waits at the border to {district} ({piece}): no simulator for {district} on the world channel")),
                        WorldEvent::Stopped { what } => log(what),
                        WorldEvent::Handover { body, to } => {
                            log(format!("loco {} runs on into {to}; {to}'s simulator moves it now", body.address));
                            if let (Some(zenoh), Some(prefix)) = (&zenoh, &prefix) {
                                zenoh.put(format!("{prefix}/handover/{to}"), serde_json::to_string(&body)?).await?;
                            }
                        }
                        WorldEvent::Arrived { id, from } => log(format!("body {id} came in from {from}")),
                    }
                }
                if let (Some(zenoh), Some(prefix)) = (&zenoh, &prefix) {
                    let now = std::time::Instant::now();
                    if now >= next_hello {
                        next_hello = now + std::time::Duration::from_secs(1);
                        zenoh.put(format!("{prefix}/sim/{district}"), district.clone()).await?;
                    }
                    // The truth, for simulated operators and tests (never read by Hive-Rail):
                    // where every body of this district is, five times a second.
                    if now >= next_truth {
                        next_truth = now + std::time::Duration::from_millis(200);
                        for body in &world.bodies {
                            zenoh.put(format!("{prefix}/truth/{}", body.id), serde_json::to_string(&body.to_wire(&district))?).await?;
                        }
                    }
                    // Only a body over a border concerns a neighbour: published while it is, and
                    // once more when it is not, so the neighbour's blocks go free.
                    let mut spanning = BTreeSet::new();
                    for body in &world.bodies {
                        let over = world.spans_border(body);
                        if over || published.contains(&body.id) {
                            zenoh.put(format!("{prefix}/body/{}", body.id), serde_json::to_string(&body.to_wire(&district))?).await?;
                        }
                        if over {
                            spanning.insert(body.id.clone());
                        }
                    }
                    published = spanning;
                    // A neighbour is there while it says so; its bodies while they are heard of.
                    let live: BTreeSet<String> = simulators
                        .iter()
                        .filter(|(name, seen)| **name != district && now.duration_since(**seen).as_secs_f64() < 3.0)
                        .map(|(name, _)| name.clone())
                        .collect();
                    for gone in world.live_neighbours.difference(&live) {
                        log(format!("{gone}'s simulator is gone from the world channel: bodies wait at its border"));
                    }
                    for new in live.difference(&world.live_neighbours) {
                        log(format!("{new}'s simulator is on the world channel"));
                    }
                    world.live_neighbours = live;
                    let stale: Vec<String> = world
                        .foreign
                        .keys()
                        .filter(|id| heard.get(*id).is_none_or(|seen| now.duration_since(*seen).as_secs_f64() > 2.0))
                        .cloned()
                        .collect();
                    for id in stale {
                        if let Some((body, owner)) = world.foreign.get(&id) {
                            log(format!("no news of loco {} from {owner}'s simulator: forgotten", body.address));
                        }
                        world.forget_foreign(&id);
                    }
                }
                if world.time_s >= next_status {
                    next_status = world.time_s + STATUS_EVERY_S;
                    for body in &world.bodies {
                        log(format!(
                            "loco {} at {} {:.0} mm, decoder {:.0} km/h{}{}{}",
                            body.address,
                            body.head.piece,
                            body.head.along_mm,
                            scale_kmh(body.speed_mm_s()),
                            if body.decoder.forward { " forward" } else { " reverse" },
                            if world.power { "" } else { ", no track power" },
                            body.held.as_ref().map(|h| format!(", {h}")).unwrap_or_default(),
                        ));
                    }
                }
            }
            Some(frame) = command_events.recv() => {
                for reply in dccex_command(&mut world, &frame) {
                    let _ = replies.send(reply);
                }
            }
            Some(event) = lb_events.recv() => match event {
                crate::lbserver::LbEvent::Connected(peer) => {
                    log(format!("LocoNet client {peer} connected; sending every detector"));
                    for (_, (bus, address), occupied) in world.detector_states() {
                        if bus == "loconet" {
                            lb.send(&crate::loconet::LocoNetMessage::input_report(address, occupied));
                        }
                    }
                }
                crate::lbserver::LbEvent::Received { message, .. } => {
                    // The interrogate (switch commands to 1017 to 1020, from JMRI or a node at
                    // start-up): every detector reports its state, as real ones do.
                    if let Some((1017, _, true)) = message.switch_request_of() {
                        log("interrogate: every detector reports");
                        for (_, (bus, address), occupied) in world.detector_states() {
                            if bus == "loconet" {
                                lb.send(&crate::loconet::LocoNetMessage::input_report(address, occupied));
                            }
                        }
                    }
                    if let Some((address, closed, true)) = message.switch_request_of() {
                        log_switched(world.switch("loconet", address, closed), &format!("LocoNet switch {address}"));
                    }
                }
                crate::lbserver::LbEvent::Disconnected { peer, .. } => log(format!("LocoNet client {peer} disconnected")),
                _ => {}
            },
            sample = async { subscriber.as_ref().expect("checked").recv_async().await }, if subscriber.is_some() => {
                let Ok(sample) = sample else { continue };
                let key = sample.key_expr().as_str().to_owned();
                let payload = String::from_utf8_lossy(&sample.payload().to_bytes()).into_owned();
                let Some(prefix) = &prefix else { continue };
                let what = key.strip_prefix(prefix.as_str()).unwrap_or("");
                if let Some(name) = what.strip_prefix("/sim/") {
                    simulators.insert(name.to_owned(), std::time::Instant::now());
                } else if what.starts_with("/body/") || what.starts_with("/handover/") {
                    let Ok(wire) = serde_json::from_str::<BodyWire>(&payload) else {
                        log(format!("unreadable body on {key}"));
                        continue;
                    };
                    heard.insert(wire.id.clone(), std::time::Instant::now());
                    if let Some(WorldEvent::Arrived { id, from }) = world.receive(&wire) {
                        log(format!("loco {} came in from {from} (body {id}); this simulator moves it now", wire.address));
                        // What the logic must prevent: a train running into a district whose
                        // track has no power.
                        if !world.power {
                            log(format!("INVARIANT VIOLATED: loco {} ran into {district}, which has no track power", wire.address));
                        }
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => return Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn layout() -> Layout {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/layout");
        let layout = crate::layout::load(&dir.join("bench-m5.json"), &dir).unwrap();
        assert!(layout.check().errors.is_empty());
        layout
    }

    fn place(module: &str, piece: &str, heading: Heading, along_mm: f64) -> Place {
        Place {
            piece: PieceId::new(module, piece),
            heading,
            along_mm,
        }
    }

    /// Runs the world, collecting its events.
    fn run(world: &mut DistrictWorld, seconds: f64) -> Vec<WorldEvent> {
        let mut events = Vec::new();
        for _ in 0..(seconds / 0.1) as usize {
            events.extend(world.step(0.1));
        }
        events
    }

    #[test]
    fn only_a_district_of_the_layout() {
        let layout = layout();
        let topology = Topology::build(&layout);
        assert!(DistrictWorld::new(&layout, &topology, "Nowhere").is_err());
        let world = DistrictWorld::new(&layout, &topology, "MacBook").unwrap();
        assert!(world.modules().contains(&"timo-adorf"));
        assert!(!world.turnouts.is_empty());
    }

    /// A body leaving Adorf's platform 1 at speed runs out over the throat and its
    /// detectors report it, block after block, with their LocoNet addresses.
    #[test]
    fn a_body_moves_at_its_decoders_speed_and_occupies_blocks() {
        let layout = layout();
        let topology = Topology::build(&layout);
        let mut world = DistrictWorld::new(&layout, &topology, "MacBook").unwrap();
        world
            .add_body(
                4444,
                600.0,
                mm_s(120.0),
                place("timo-adorf", "1", Heading::Backward, 900.0),
            )
            .unwrap();
        let start: Vec<String> = world.occupied().map(|(b, _)| b.clone()).collect();
        assert!(!start.is_empty(), "the body occupies its platform");
        // No power, no move; power but no speed, no move.
        run(&mut world, 2.0);
        assert_eq!(world.bodies[0].head.along_mm, 900.0);
        world.power_on();
        run(&mut world, 2.0);
        assert_eq!(world.bodies[0].head.along_mm, 900.0);
        world.drive(4444, 63, true);
        let events = run(&mut world, 10.0);
        let moved = events.iter().any(|e| {
            matches!(e, WorldEvent::Occupancy { occupied: true, detector: Some((bus, _)), .. } if bus == "loconet")
        });
        assert!(moved, "{events:#?}");
        assert_ne!(world.bodies[0].head.piece, PieceId::new("timo-adorf", "1"));
    }

    /// The decoder holds its last speed after the command station stops sending, and stops
    /// with the track power; it takes up its speed again when the power returns.
    #[test]
    fn a_decoder_keeps_its_speed_and_stops_without_power() {
        let layout = layout();
        let topology = Topology::build(&layout);
        let mut world = DistrictWorld::new(&layout, &topology, "MacBook").unwrap();
        world
            .add_body(
                3,
                300.0,
                mm_s(100.0),
                place("timo-adorf", "1", Heading::Backward, 900.0),
            )
            .unwrap();
        world.power_on();
        world.drive(3, 40, true);
        run(&mut world, 1.0);
        world.forget(3);
        assert_eq!(world.bodies[0].decoder.speed, 0, "<- cab> stops the loco");
        world.drive(3, 40, true);
        world.refresh.clear(); // the command station no longer sends to it
        let before = world.bodies[0].head.clone();
        run(&mut world, 1.0);
        assert_ne!(world.bodies[0].head, before, "it runs on at its last speed");
        world.power_off();
        let before = world.bodies[0].head.clone();
        run(&mut world, 1.0);
        assert_eq!(world.bodies[0].head, before);
        assert_eq!(world.bodies[0].decoder.speed, 40);
    }

    /// Reversing the decoder runs the body the other way, tail first.
    #[test]
    fn reversing_the_decoder_runs_the_body_back() {
        let layout = layout();
        let topology = Topology::build(&layout);
        let mut world = DistrictWorld::new(&layout, &topology, "MacBook").unwrap();
        world
            .add_body(
                3,
                300.0,
                mm_s(100.0),
                place("timo-adorf", "1", Heading::Backward, 600.0),
            )
            .unwrap();
        world.power_on();
        world.drive(3, 30, true);
        run(&mut world, 1.0);
        let out = world.bodies[0].head.along_mm;
        world.drive(3, 30, false);
        run(&mut world, 1.0);
        let body = &world.bodies[0];
        assert_eq!(body.head.heading, Heading::Forward, "the tail end leads");
        assert!(!body.front_first);
        // Back on platform 1, nearer its buffer than where it turned.
        assert_eq!(body.head.piece, PieceId::new("timo-adorf", "1"));
        assert!(body.head.along_mm > 1102.0 - out - 300.0, "{body:?}");
    }

    /// Turnouts move by their device's address on its bus, not by another bus's.
    #[test]
    fn turnouts_move_by_their_bus_address() {
        let layout = layout();
        let topology = Topology::build(&layout);
        let mut world = DistrictWorld::new(&layout, &topology, "MacBook").unwrap();
        let (bus, address) = world.turnout_devices.keys().next().cloned().unwrap();
        let key = world.turnout_devices[&(bus.clone(), address)].clone();
        assert_eq!(bus, "loconet");
        assert!(
            world.switch("dcc", address, false).is_none(),
            "a DCC decoder is not this one"
        );
        assert_eq!(
            world.switch("loconet", address, false),
            Some(WorldEvent::Turnout {
                turnout: key.clone(),
                closed: false
            })
        );
        assert!(!world.turnouts[&key]);
        assert!(
            world.switch("loconet", address, false).is_none(),
            "already there"
        );
    }

    /// A body runs into one standing ahead of it: a fault, and the track shorts.
    #[test]
    fn running_into_another_body_shorts_the_track() {
        let layout = layout();
        let topology = Topology::build(&layout);
        let mut world = DistrictWorld::new(&layout, &topology, "MacBook").unwrap();
        world
            .add_body(
                3,
                200.0,
                mm_s(100.0),
                place("timo-adorf", "1", Heading::Backward, 300.0),
            )
            .unwrap();
        world
            .add_body(
                4,
                200.0,
                mm_s(100.0),
                place("timo-adorf", "1", Heading::Backward, 900.0),
            )
            .unwrap();
        world.power_on();
        // Loco 3 stands behind loco 4 (along runs the way they face) and runs into it.
        world.drive(3, 60, true);
        let events = run(&mut world, 10.0);
        let crash = events
            .iter()
            .find(|e| matches!(e, WorldEvent::Fault { short: true, .. }));
        assert!(crash.is_some(), "{events:#?}");
        assert!(world.shorted && !world.power);
    }

    /// A body trailing through a turnout from its straight leg while it lies diverging
    /// runs through it and derails: a fault that shorts the track.
    #[test]
    fn running_through_a_turnout_set_against_derails() {
        let layout = layout();
        let topology = Topology::build(&layout);
        let mut world = DistrictWorld::new(&layout, &topology, "MacBook").unwrap();
        // A piece that ends at a turnout's straight leg, long enough for a short body.
        let (piece, heading, turnout) = topology
            .pieces()
            .filter(|p| p.district == "MacBook" && p.length_mm >= 300)
            .flat_map(|p| [Heading::Forward, Heading::Backward].map(|h| (p.id.clone(), h)))
            .find_map(|(piece, heading)| {
                let ahead = topology.turnout_ahead(&piece, heading)?;
                (ahead.entry == Leg::Straight)
                    .then(|| (piece, heading, format!("{}/{}", ahead.module, ahead.id)))
            })
            .expect("a straight leg into a turnout");
        world.turnouts.insert(turnout.clone(), false);
        world
            .add_body(
                8,
                100.0,
                mm_s(60.0),
                Place {
                    piece,
                    heading,
                    along_mm: 150.0,
                },
            )
            .unwrap();
        world.power_on();
        world.drive(8, 80, true);
        let events = run(&mut world, 30.0);
        let derailed = events.iter().any(|e| {
            matches!(e, WorldEvent::Fault { what, short: true } if what.contains(&turnout) && what.contains("derailed"))
        });
        assert!(derailed, "{turnout}: {events:#?}");
        assert!(world.shorted);
    }

    /// Two district simulators side by side, passing bodies as the world channel does.
    fn exchange(
        a: &mut DistrictWorld,
        b: &mut DistrictWorld,
        events: &[WorldEvent],
    ) -> Vec<WorldEvent> {
        let mut arrived = Vec::new();
        for event in events {
            if let WorldEvent::Handover { body, .. } = event {
                arrived.extend(b.receive(body));
            }
        }
        let wires: Vec<BodyWire> = a
            .bodies
            .iter()
            .map(|body| body.to_wire(&a.district))
            .collect();
        for wire in &wires {
            b.receive(wire);
        }
        // Its own publications come back to it too, and are dropped.
        for wire in &wires {
            assert!(a.receive(wire).is_none());
        }
        arrived
    }

    /// A body driven from MacBook crosses into the M5StickC's district at its last speed:
    /// the M5StickC's simulator moves it on, its detectors report it, and MacBook's border
    /// block goes free once the tail has left. The M5StickC's command station never sent to
    /// that decoder: it runs on, as a DCC decoder does.
    #[test]
    fn a_body_crosses_the_border_at_its_last_speed() {
        let layout = layout();
        let topology = Topology::build(&layout);
        let mut macbook = DistrictWorld::new(&layout, &topology, "MacBook").unwrap();
        let mut m5 = DistrictWorld::new(&layout, &topology, "M5StickC").unwrap();
        macbook.live_neighbours.insert("M5StickC".into());
        m5.live_neighbours.insert("MacBook".into());
        // A piece of MacBook's that joins the M5StickC's track, and the heading towards it.
        let (piece, heading) = topology
            .border_joints()
            .into_iter()
            .flat_map(|(a, b)| [(a.clone(), b.clone()), (b, a)])
            .filter(|(a, b)| {
                topology.piece(a).unwrap().district == "MacBook"
                    && topology.piece(b).unwrap().district == "M5StickC"
            })
            .find_map(|(a, b)| {
                [Heading::Forward, Heading::Backward]
                    .into_iter()
                    .find(|h| {
                        topology
                            .next(&a, *h, |_, _| Some(true))
                            .iter()
                            .any(|(n, _)| *n == b)
                    })
                    .map(|h| (a, h))
            })
            .expect("a border from MacBook to the M5StickC");
        let length = f64::from(topology.piece(&piece).unwrap().length_mm);
        macbook
            .add_body(
                77,
                300.0,
                mm_s(80.0),
                Place {
                    piece,
                    heading,
                    along_mm: length.min(320.0),
                },
            )
            .unwrap();
        macbook.power_on();
        m5.power_on();
        macbook.drive(77, 60, true);
        let mut m5_events = Vec::new();
        let mut arrived = Vec::new();
        for _ in 0..600 {
            let events = macbook.step(0.1);
            arrived.extend(exchange(&mut macbook, &mut m5, &events));
            let events = m5.step(0.1);
            arrived.extend(exchange(&mut m5, &mut macbook, &events));
            m5_events.extend(events);
        }
        assert!(
            arrived
                .iter()
                .any(|e| matches!(e, WorldEvent::Arrived { from, .. } if from == "MacBook")),
            "{arrived:#?}"
        );
        assert!(macbook.bodies.is_empty() && m5.bodies.len() == 1);
        assert_eq!(m5.bodies[0].decoder.speed, 60, "it kept its last speed");
        assert!(m5_events
            .iter()
            .any(|e| matches!(e, WorldEvent::Occupancy { occupied: true, .. })));
        // On MacBook's track nothing is left: its blocks are free again.
        assert!(macbook.occupancy().is_empty(), "{:?}", macbook.occupancy());
    }

    /// Without the neighbour's simulator on the world channel the body waits at the border:
    /// the head stops at the border to the next district: that district's simulator takes
    /// it over (step 2); here it waits there.
    #[test]
    fn a_body_stops_at_the_border() {
        let layout = layout();
        let topology = Topology::build(&layout);
        let mut world = DistrictWorld::new(&layout, &topology, "M5StickC").unwrap();
        let piece = topology
            .pieces()
            .find(|p| p.district == "M5StickC" && p.id.module == "m5-2e96-in")
            .unwrap()
            .id
            .clone();
        let length = f64::from(topology.piece(&piece).unwrap().length_mm);
        world
            .add_body(
                5,
                200.0,
                mm_s(100.0),
                Place {
                    piece,
                    heading: Heading::Forward,
                    along_mm: length.min(250.0),
                },
            )
            .unwrap();
        world.power_on();
        world.drive(5, 100, true);
        let events = run(&mut world, 60.0);
        let border = events
            .iter()
            .find(|e| matches!(e, WorldEvent::Border { .. }));
        assert!(border.is_some(), "{events:#?}");
        assert!(world.bodies[0]
            .held
            .as_deref()
            .is_some_and(|h| h.starts_with("at the border")));
    }
}
