//! The layout as one track graph, built from a checked layout and its module files: every
//! track piece of every module, joined at module connections, turnouts and buffer stops.
//! Train tracking, the simulator and the views move along it.
//!
//! Movement follows the track: a train runs along a piece from one end to the other, and
//! through a turnout only from the trunk to the straight or diverging leg, or back, never
//! from one leg to the other. With the turnout's position known, only along the set leg.
//!
//! The device map turns a bus address into its meaning: in district MacBook, LocoNet
//! sensor 203 is `timo-adorf/occ_1`, the occupancy detector of block `adorf_1`.

// Used by the simulator, train tracking and the views, which come next.
#![allow(dead_code)]

use crate::layout::{address_space, split_end, Layout, Point};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// A track piece, by module and piece id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PieceId {
    pub module: String,
    pub piece: String,
}

impl PieceId {
    pub fn new(module: &str, piece: &str) -> Self {
        Self {
            module: module.to_owned(),
            piece: piece.to_owned(),
        }
    }
}

impl std::fmt::Display for PieceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.module, self.piece)
    }
}

/// Which way a train runs along a piece: `Forward` from its `from` point to its `to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Heading {
    Forward,
    Backward,
}

impl Heading {
    pub fn reversed(self) -> Self {
        match self {
            Heading::Forward => Heading::Backward,
            Heading::Backward => Heading::Forward,
        }
    }
}

/// One end of a piece: 0 is its `from` point, 1 its `to` point.
type PieceEnd = (usize, u8);

/// What a piece end meets.
#[derive(Debug, Clone)]
enum Link {
    /// Another piece, at a module connection or a plain joint.
    Piece(PieceEnd),
    /// A turnout leg; the other legs are found through the turnout.
    Turnout { turnout: usize, leg: Leg },
    /// A buffer stop.
    Buffer,
    /// A module end with nothing connected: the end of the layout.
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leg {
    Trunk,
    Straight,
    Diverging,
}

impl Leg {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "trunk" => Some(Leg::Trunk),
            "straight" => Some(Leg::Straight),
            "diverging" => Some(Leg::Diverging),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PieceInfo {
    pub id: PieceId,
    pub length_mm: u32,
    pub district: String,
}

#[derive(Debug, Clone)]
pub struct TurnoutInfo {
    pub module: String,
    pub id: String,
    pub district: String,
    /// The device that switches it, `module/device`.
    pub device: Option<String>,
    /// The piece ends at its trunk, straight and diverging legs.
    legs: [Option<PieceEnd>; 3],
}

#[derive(Debug, Clone)]
pub struct BlockInfo {
    pub module: String,
    pub id: String,
    pub piece: PieceId,
    pub cuts_mm: [u32; 2],
    pub role: Option<String>,
    pub district: String,
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub module: String,
    pub id: String,
    pub kind: String,
    pub bus: String,
    pub address: u16,
    pub district: String,
    /// The block an occupancy detector covers, `module/block`.
    pub block: Option<String>,
}

#[derive(Debug)]
pub struct Topology {
    pieces: Vec<PieceInfo>,
    index: HashMap<PieceId, usize>,
    links: HashMap<PieceEnd, Link>,
    turnouts: Vec<TurnoutInfo>,
    turnout_index: HashMap<(String, String), usize>,
    pub blocks: BTreeMap<String, BlockInfo>,
    devices: HashMap<(String, &'static str, u16), DeviceInfo>,
}

impl Topology {
    /// Builds the graph of a layout. Assumes `layout.check()` found no errors; parts it
    /// cannot place (a piece of a missing module) are left out.
    pub fn build(layout: &Layout) -> Self {
        let mut topology = Topology {
            pieces: Vec::new(),
            index: HashMap::new(),
            links: HashMap::new(),
            turnouts: Vec::new(),
            turnout_index: HashMap::new(),
            blocks: BTreeMap::new(),
            devices: HashMap::new(),
        };
        let district = |module: &str| layout.district_of(module).unwrap_or("").to_owned();

        for (module_id, (module, _)) in &layout.modules {
            for turnout in &module.turnouts {
                topology.turnout_index.insert(
                    (module_id.clone(), turnout.id.clone()),
                    topology.turnouts.len(),
                );
                topology.turnouts.push(TurnoutInfo {
                    module: module_id.clone(),
                    id: turnout.id.clone(),
                    district: district(module_id),
                    device: turnout.device.as_ref().map(|d| format!("{module_id}/{d}")),
                    legs: [None; 3],
                });
            }
            for piece in &module.track {
                let id = PieceId::new(module_id, &piece.id);
                topology.index.insert(id.clone(), topology.pieces.len());
                topology.pieces.push(PieceInfo {
                    id,
                    length_mm: piece.length_mm,
                    district: district(module_id),
                });
            }
        }

        // What each piece end meets inside its module: a turnout leg, a buffer stop, or for
        // now an open module end (connections below join those).
        for (module_id, (module, _)) in &layout.modules {
            for piece in &module.track {
                let index = topology.index[&PieceId::new(module_id, &piece.id)];
                for (end, text) in [(0_u8, &piece.from), (1, &piece.to)] {
                    let link = match module.point(text) {
                        Ok(Point::Leg { turnout, leg }) => {
                            let turnout = topology.turnout_index[&(module_id.clone(), turnout)];
                            let leg = Leg::parse(&leg).unwrap_or(Leg::Trunk);
                            topology.turnouts[turnout].legs[leg as usize] = Some((index, end));
                            Link::Turnout { turnout, leg }
                        }
                        Ok(Point::Buffer) => Link::Buffer,
                        Ok(Point::End { .. }) | Err(_) => Link::Open,
                    };
                    topology.links.insert((index, end), link);
                }
            }
        }

        // Module connections: the left track of one end meets the right track of the other.
        for connection in &layout.file.connections {
            let side = |text: &str| {
                let (module_id, end_id) = split_end(text)?;
                let (module, _) = layout.modules.get(module_id)?;
                Some((module_id.to_owned(), module, module.end(end_id)?))
            };
            let (Some((ma, moda, ea)), Some((mb, modb, eb))) =
                (side(&connection.a), side(&connection.b))
            else {
                continue;
            };
            let count = ea.tracks.len().min(eb.tracks.len());
            for i in 0..count {
                let ta = &ea.tracks[i];
                let tb = &eb.tracks[eb.tracks.len() - 1 - i];
                let end_of = |module_id: &str,
                              module: &crate::layout::ModuleFile,
                              end: &str,
                              track: &str| {
                    let piece = module.piece_at(end, track)?;
                    let point = format!("{end}.{track}");
                    let index = topology.index[&PieceId::new(module_id, &piece.id)];
                    Some((index, u8::from(piece.to == point)))
                };
                if let (Some(a), Some(b)) =
                    (end_of(&ma, moda, &ea.id, ta), end_of(&mb, modb, &eb.id, tb))
                {
                    topology.links.insert(a, Link::Piece(b));
                    topology.links.insert(b, Link::Piece(a));
                }
            }
        }

        for (module_id, (module, _)) in &layout.modules {
            for block in &module.blocks {
                let key = format!("{module_id}/{}", block.id);
                topology.blocks.insert(
                    key,
                    BlockInfo {
                        module: module_id.clone(),
                        id: block.id.clone(),
                        piece: PieceId::new(module_id, &block.track),
                        cuts_mm: block.cuts_mm,
                        role: block.role.clone(),
                        district: district(module_id),
                    },
                );
            }
            for device in &module.devices {
                // An occupancy detector's block: named by the device, or by the block.
                let block = device.block.clone().or_else(|| {
                    module
                        .blocks
                        .iter()
                        .find(|block| block.occupancy.as_deref() == Some(device.id.as_str()))
                        .map(|block| block.id.clone())
                });
                let info = DeviceInfo {
                    module: module_id.clone(),
                    id: device.id.clone(),
                    kind: device.kind.clone(),
                    bus: device.bus.clone(),
                    address: device.address,
                    district: district(module_id),
                    block: block.map(|block| format!("{module_id}/{block}")),
                };
                topology.devices.insert(
                    (info.district.clone(), address_space(device), device.address),
                    info,
                );
            }
        }
        topology
    }

    pub fn piece(&self, id: &PieceId) -> Option<&PieceInfo> {
        self.index.get(id).map(|&index| &self.pieces[index])
    }

    pub fn pieces(&self) -> impl Iterator<Item = &PieceInfo> {
        self.pieces.iter()
    }

    pub fn turnouts(&self) -> impl Iterator<Item = &TurnoutInfo> {
        self.turnouts.iter()
    }

    /// The device at an address in a district: `space` as `address_space` names it, for
    /// example "LocoNet sensor".
    pub fn device(&self, district: &str, space: &str, address: u16) -> Option<&DeviceInfo> {
        self.devices
            .iter()
            .find(|((d, s, a), _)| d == district && *s == space && *a == address)
            .map(|(_, info)| info)
    }

    /// Where a train running along `piece` in `heading` can go next. `closed` gives a
    /// turnout's position (`module`, `turnout` → straight), or `None` when unknown: then
    /// both legs are possible. A trailing move against the set position is not possible.
    pub fn next<F>(&self, piece: &PieceId, heading: Heading, closed: F) -> Vec<(PieceId, Heading)>
    where
        F: Fn(&str, &str) -> Option<bool>,
    {
        let Some(&index) = self.index.get(piece) else {
            return Vec::new();
        };
        let exit = match heading {
            Heading::Forward => 1,
            Heading::Backward => 0,
        };
        let enter = |(piece, end): PieceEnd| {
            let heading = if end == 0 {
                Heading::Forward
            } else {
                Heading::Backward
            };
            (self.pieces[piece].id.clone(), heading)
        };
        match self.links.get(&(index, exit)) {
            Some(Link::Piece(other)) => vec![enter(*other)],
            Some(Link::Turnout { turnout, leg }) => {
                let info = &self.turnouts[*turnout];
                let position = closed(&info.module, &info.id);
                let legs: Vec<Leg> = match leg {
                    Leg::Trunk => match position {
                        Some(true) => vec![Leg::Straight],
                        Some(false) => vec![Leg::Diverging],
                        None => vec![Leg::Straight, Leg::Diverging],
                    },
                    Leg::Straight if position != Some(false) => vec![Leg::Trunk],
                    Leg::Diverging if position != Some(true) => vec![Leg::Trunk],
                    _ => Vec::new(),
                };
                legs.into_iter()
                    .filter_map(|leg| info.legs[leg as usize])
                    .map(enter)
                    .collect()
            }
            Some(Link::Buffer) | Some(Link::Open) | None => Vec::new(),
        }
    }

    /// Every piece and heading reachable from a start, with every turnout free to choose.
    pub fn reachable(&self, piece: &PieceId, heading: Heading) -> BTreeSet<(PieceId, Heading)> {
        let mut seen = BTreeSet::new();
        let mut todo = vec![(piece.clone(), heading)];
        while let Some(state) = todo.pop() {
            if seen.insert(state.clone()) {
                todo.extend(self.next(&state.0, state.1, |_, _| None));
            }
        }
        seen
    }

    /// The pieces that join two districts: (piece in one, piece in the other).
    pub fn border_joints(&self) -> Vec<(PieceId, PieceId)> {
        let mut joints = Vec::new();
        for (&(piece, _), link) in &self.links {
            if let Link::Piece((other, _)) = link {
                let (a, b) = (&self.pieces[piece], &self.pieces[*other]);
                if a.district != b.district && a.id < b.id {
                    joints.push((a.id.clone(), b.id.clone()));
                }
            }
        }
        joints.sort();
        joints
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn example() -> Topology {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/layout");
        let layout = crate::layout::load(&dir.join("bench-double-track.json"), &dir).unwrap();
        assert!(layout.check().errors.is_empty());
        Topology::build(&layout)
    }

    fn piece(module: &str, piece: &str) -> PieceId {
        PieceId::new(module, piece)
    }

    /// The turn-back at Adorf: a train arriving westbound on track 2 crosses over to
    /// platform 1 (X2 diverging, then X1 diverging to trunk), and after reversing there
    /// leaves eastbound on track 1 towards the border.
    #[test]
    fn a_train_turns_back_at_adorf() {
        let topology = example();
        let arriving = topology.reachable(&piece("timo-adorf", "2b"), Heading::Forward);
        assert!(arriving.contains(&(piece("timo-adorf", "1"), Heading::Backward)));
        let leaving = topology.reachable(&piece("timo-adorf", "1"), Heading::Forward);
        assert!(leaving.contains(&(piece("timo-2e96-west", "1"), Heading::Forward)));
        assert!(leaving.contains(&(piece("anna-2e96-border", "1"), Heading::Forward)));
        assert!(leaving.contains(&(piece("anna-bdorf", "1"), Heading::Forward)));
    }

    /// The turn-back at Bdorf: eastbound on track 1 over to platform 2, and back west on
    /// track 2 across the border into MacBook's district.
    #[test]
    fn a_train_turns_back_at_bdorf() {
        let topology = example();
        let arriving = topology.reachable(&piece("anna-bdorf", "1a"), Heading::Forward);
        // Bdorf's track 2 is described from W4 (east) to W3 (west): arriving eastbound is
        // Backward along it, leaving westbound Forward.
        assert!(arriving.contains(&(piece("anna-bdorf", "2"), Heading::Backward)));
        let leaving = topology.reachable(&piece("anna-bdorf", "2"), Heading::Forward);
        assert!(leaving.contains(&(piece("timo-2e96-border", "2"), Heading::Backward)));
        assert!(leaving.contains(&(piece("timo-adorf", "2"), Heading::Forward)));
    }

    /// Through a turnout only trunk to leg and back: from the straight leg a train cannot
    /// get onto the diverging one, and with the position set only the set leg is open.
    #[test]
    fn turnouts_are_passed_trunk_to_leg_only() {
        let topology = example();
        // Westbound on Adorf platform 1 (piece 1 backward): through W1 straight to trunk.
        let next = topology.next(&piece("timo-adorf", "1"), Heading::Backward, |_, _| None);
        assert_eq!(next, [(piece("timo-adorf", "1a"), Heading::Backward)]);
        // Eastbound out of 1a into W1: straight or diverging, or only one when set.
        let both = topology.next(&piece("timo-adorf", "1a"), Heading::Forward, |_, _| None);
        assert_eq!(both.len(), 2);
        let set = topology.next(&piece("timo-adorf", "1a"), Heading::Forward, |_, id| {
            (id == "W1").then_some(false)
        });
        assert_eq!(set, [(piece("timo-adorf", "s1"), Heading::Forward)]);
        // Trailing through W2 against its position is not possible.
        let blocked = topology.next(&piece("timo-adorf", "1"), Heading::Forward, |_, id| {
            (id == "W2").then_some(false)
        });
        assert!(blocked.is_empty());
    }

    #[test]
    fn the_border_joins_both_tracks() {
        let topology = example();
        assert_eq!(
            topology.border_joints(),
            [
                (
                    piece("anna-2e96-border", "1"),
                    piece("timo-2e96-border", "1")
                ),
                (
                    piece("anna-2e96-border", "2"),
                    piece("timo-2e96-border", "2")
                ),
            ]
        );
    }

    /// The same LocoNet sensor number means a different device in each district.
    #[test]
    fn the_device_map_names_an_address_per_district() {
        let topology = example();
        let macbook = topology.device("MacBook", "LocoNet sensor", 203).unwrap();
        assert_eq!(
            (macbook.module.as_str(), macbook.id.as_str()),
            ("timo-adorf", "occ_1")
        );
        assert_eq!(macbook.block.as_deref(), Some("timo-adorf/adorf_1"));
        let windows = topology.device("Windows", "LocoNet sensor", 203).unwrap();
        assert_eq!(windows.block.as_deref(), Some("anna-2e96-east/line_1"));
        // LocoNet switch 21 is Adorf's point W1, and switch 111 the crossing's lights.
        let point = topology.device("MacBook", "LocoNet switch", 21).unwrap();
        assert_eq!(point.id, "point_w1");
        assert_eq!(
            topology
                .device("MacBook", "LocoNet switch", 111)
                .unwrap()
                .id,
            "lc_lights"
        );
    }
}
