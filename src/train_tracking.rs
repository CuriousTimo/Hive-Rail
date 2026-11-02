//! Following trains within a district (wiki "District Simulators and Train Tracking",
//! step 4; Architecture §10.2). A train is placed once by an operator; after that, a block
//! that becomes occupied next to the blocks a train holds is that train's, and a block that
//! goes free is left behind. A train whose blocks are all free is between its last block
//! and the next one (a gap without detection) until a block next to its last one reports.
//!
//! **Never guess.** Occupancy next to no train is a ghost; next to two trains it is
//! ambiguous. Both are reported, and the node does not choose.
//!
//! "Next to" comes from the track graph: the first blocks found along the track from each
//! end of a block, over every leg of a turnout (the positions are not trusted yet), and
//! over gaps without detection.

use crate::topology::{Heading, PieceId, Topology};
use std::collections::{BTreeMap, BTreeSet};

/// How far the search for a neighbouring block goes, in pieces.
const MAX_PIECES: usize = 40;

/// A followed train, by its loco address.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FollowedTrain {
    pub loco: u16,
    /// The blocks it holds, newest (its head, while it runs on) first.
    pub blocks: Vec<String>,
    /// The block it was last seen in, kept while it is between blocks.
    pub last: String,
    /// How often it was handed to another district: the copy with more hops is the newer
    /// claim, so a district lets go of a train its neighbour took.
    #[serde(default)]
    pub hops: u32,
    /// The district that took it while it still holds blocks here (its tail): it is let go
    /// once they are free.
    #[serde(default)]
    pub handing_to: Option<String>,
    /// The block its head came from into the block it is in now: whether it runs towards
    /// a border or away from it. None for a train just placed.
    #[serde(default)]
    pub came_from: Option<String>,
    /// The decoder's direction (forward) when the head entered that block: a train whose
    /// decoder runs the other way now has turned back.
    #[serde(default)]
    pub entered_forward: Option<bool>,
}

/// The train picture of one district, as its node publishes it on
/// `hive/<session>/node/<id>/trains` and every other node keeps it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TrainPicture {
    pub district: String,
    /// Raised with every change, so a copy can tell which is newer.
    pub version: u64,
    pub trains: Vec<FollowedTrain>,
}

/// What following found.
#[derive(Debug, Clone, PartialEq)]
pub enum Followed {
    Placed {
        loco: u16,
        block: String,
    },
    /// The train moved into a block.
    Entered {
        loco: u16,
        block: String,
    },
    /// It left a block; `between` when it holds none now (in a gap, after `block`).
    Left {
        loco: u16,
        block: String,
        between: bool,
    },
    /// A train from a neighbouring district entered a block of this one: this district
    /// follows it from now on (the handoff).
    Arrived {
        loco: u16,
        block: String,
        from: String,
    },
    /// The neighbour took a train and its last block here is free: it is the neighbour's.
    HandedOver {
        loco: u16,
        to: String,
    },
    /// Occupancy where no train can have come from.
    Ghost {
        block: String,
    },
    /// Occupancy next to more than one train: the operator decides.
    Ambiguous {
        block: String,
        locos: Vec<u16>,
    },
}

impl std::fmt::Display for Followed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Followed::Placed { loco, block } => write!(f, "train {loco} placed in block {block}"),
            Followed::Entered { loco, block } => write!(f, "train {loco} entered block {block}"),
            Followed::Left {
                loco,
                block,
                between: false,
            } => write!(f, "train {loco} left block {block}"),
            Followed::Left {
                loco,
                block,
                between: true,
            } => {
                write!(f, "train {loco} left block {block}; between blocks now")
            }
            Followed::Arrived { loco, block, from } => {
                write!(f, "train {loco} came in from {from} into block {block}; this district follows it now")
            }
            Followed::HandedOver { loco, to } => write!(f, "train {loco} handed over to {to}"),
            Followed::Ghost { block } => {
                write!(f, "GHOST block {block} occupied, but no train can be there: place one, or check the track")
            }
            Followed::Ambiguous { block, locos } => {
                let locos: Vec<String> = locos.iter().map(u16::to_string).collect();
                write!(
                    f,
                    "AMBIGUOUS block {block} occupied next to trains {}: the operator decides",
                    locos.join(" and ")
                )
            }
        }
    }
}

/// Every block's neighbours along the track: the first blocks found from each of its ends,
/// over every leg of every turnout.
pub fn block_graph(topology: &Topology) -> BTreeMap<String, BTreeSet<String>> {
    block_graph_with(topology, &|_, _| None)
}

/// The same, over the set leg of the turnouts whose position is known (`closed` gives it,
/// by module and turnout) and every leg of the others.
pub fn block_graph_with(
    topology: &Topology,
    closed: &dyn Fn(&str, &str) -> Option<bool>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut on_piece: BTreeMap<&PieceId, Vec<(&String, [u32; 2])>> = BTreeMap::new();
    for (key, block) in &topology.blocks {
        on_piece
            .entry(&block.piece)
            .or_default()
            .push((key, block.cuts_mm));
    }
    // The first block on `piece` running in `heading` from `at_mm` (from-based), if any.
    let first_on =
        |piece: &PieceId, heading: Heading, at_mm: f64, except: &str| -> Option<String> {
            let blocks = on_piece.get(piece)?;
            let ahead = blocks.iter().filter(|(key, _)| key.as_str() != except);
            match heading {
                Heading::Forward => ahead
                    .filter(|(_, [c0, _])| f64::from(*c0) >= at_mm)
                    .min_by_key(|(_, [c0, _])| *c0),
                Heading::Backward => ahead
                    .filter(|(_, [_, c1])| f64::from(*c1) <= at_mm)
                    .max_by_key(|(_, [_, c1])| *c1),
            }
            .map(|(key, _)| (*key).clone())
        };
    let mut graph = BTreeMap::new();
    for (key, block) in &topology.blocks {
        let mut neighbours = BTreeSet::new();
        let [c0, c1] = block.cuts_mm;
        for (heading, start) in [
            (Heading::Forward, f64::from(c1)),
            (Heading::Backward, f64::from(c0)),
        ] {
            let mut seen = BTreeSet::new();
            let mut todo = vec![(block.piece.clone(), heading, start, 0_usize)];
            while let Some((piece, heading, at, depth)) = todo.pop() {
                if depth > MAX_PIECES || !seen.insert((piece.clone(), heading, depth == 0)) {
                    continue;
                }
                if let Some(found) = first_on(&piece, heading, at, key) {
                    neighbours.insert(found);
                    continue;
                }
                for (next, next_heading) in topology.next(&piece, heading, closed) {
                    let length = topology
                        .piece(&next)
                        .map_or(0.0, |p| f64::from(p.length_mm));
                    let entry = match next_heading {
                        Heading::Forward => 0.0,
                        Heading::Backward => length,
                    };
                    todo.push((next, next_heading, entry, depth + 1));
                }
            }
        }
        graph.insert(key.clone(), neighbours);
    }
    graph
}

pub struct Tracker<'a> {
    pub district: String,
    topology: &'a Topology,
    /// Each block's neighbours, over the turnout positions known now.
    graph: BTreeMap<String, BTreeSet<String>>,
    /// Turnout positions as last set, `module/turnout` → closed.
    positions: BTreeMap<String, bool>,
    /// This district's blocks.
    blocks: BTreeSet<String>,
    occupied: BTreeSet<String>,
    trains: BTreeMap<u16, FollowedTrain>,
    /// The neighbouring districts' pictures, as last heard: a train entering from one of
    /// them is that district's train.
    foreign: BTreeMap<String, TrainPicture>,
    version: u64,
}

impl<'a> Tracker<'a> {
    pub fn new(topology: &'a Topology, district: &str) -> Self {
        Self {
            district: district.to_owned(),
            topology,
            graph: block_graph(topology),
            positions: BTreeMap::new(),
            blocks: topology
                .blocks_in(district)
                .map(|(key, _)| key.clone())
                .collect(),
            occupied: BTreeSet::new(),
            trains: BTreeMap::new(),
            foreign: BTreeMap::new(),
            version: 0,
        }
    }

    /// A turnout was set: trains only follow the way it leads now.
    pub fn set_turnout(&mut self, turnout: &str, closed: bool) {
        if self.positions.insert(turnout.to_owned(), closed) != Some(closed) {
            let positions = &self.positions;
            self.graph = block_graph_with(self.topology, &|module, id| {
                positions.get(&format!("{module}/{id}")).copied()
            });
        }
    }

    /// The borders: for each neighbouring district, this district's blocks next to one of
    /// its blocks (the edge blocks in front of the border).
    pub fn borders(&self) -> BTreeMap<String, BTreeSet<String>> {
        let district_of = |block: &str| self.topology.blocks.get(block).map(|b| b.district.clone());
        let graph = block_graph(self.topology);
        let mut borders: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for block in &self.blocks {
            for other in graph.get(block).into_iter().flatten() {
                if let Some(district) = district_of(other).filter(|d| *d != self.district) {
                    borders.entry(district).or_default().insert(block.clone());
                }
            }
        }
        borders
    }

    /// The decoder's direction as the train's head entered the block it is in, from the
    /// loco register: what tells later whether it has turned back.
    pub fn note_direction(&mut self, loco: u16, forward: Option<bool>) {
        if let Some(train) = self.trains.get_mut(&loco) {
            train.entered_forward = forward;
        }
    }

    /// The trains whose head is in an edge block and that run towards its border: they came
    /// in from this district's side and run on, or came in over the border and turned back.
    /// `forward` gives a loco's direction now, from the register. A train only placed there
    /// counts as running towards it: which way it faces is not known yet. With the
    /// neighbouring district each runs towards, and the edge block it is in.
    pub fn towards_borders(
        &self,
        borders: &BTreeMap<String, BTreeSet<String>>,
        forward: &dyn Fn(u16) -> Option<bool>,
    ) -> Vec<(u16, String, String)> {
        let district_of = |block: &str| self.topology.blocks.get(block).map(|b| b.district.clone());
        let mut found = Vec::new();
        for train in self.trains.values().filter(|t| t.handing_to.is_none()) {
            let head = train.blocks.first().unwrap_or(&train.last);
            for (district, edges) in borders {
                let came_over_it =
                    train.came_from.as_deref().and_then(district_of).as_ref() == Some(district);
                let turned_back = train
                    .entered_forward
                    .zip(forward(train.loco))
                    .is_some_and(|(then, now)| then != now);
                if edges.contains(head) && came_over_it == turned_back {
                    found.push((train.loco, district.clone(), head.clone()));
                }
            }
        }
        found
    }

    /// The heading along an edge block's piece that leads towards a neighbouring district:
    /// the way a boundary signal in it faces.
    pub fn heading_towards(&self, edge: &str, district: &str) -> Option<Heading> {
        let piece = &self.topology.blocks.get(edge)?.piece;
        let district_of = |piece: &PieceId| self.topology.piece(piece).map(|p| p.district.as_str());
        [Heading::Forward, Heading::Backward]
            .into_iter()
            .find(|heading| {
                let mut seen = BTreeSet::new();
                let mut todo = vec![(piece.clone(), *heading, 0_usize)];
                while let Some((piece, heading, depth)) = todo.pop() {
                    if depth > MAX_PIECES || !seen.insert((piece.clone(), heading)) {
                        continue;
                    }
                    for (next, next_heading) in self.topology.next(&piece, heading, |_, _| None) {
                        match district_of(&next) {
                            Some(d) if d == district => return true,
                            Some(d) if d == self.district => {
                                todo.push((next, next_heading, depth + 1))
                            }
                            _ => {}
                        }
                    }
                }
                false
            })
    }

    /// A neighbour's train at the border, in front of one edge block: one of its trains that
    /// holds a block next to it (on the same track, so double track stays two seams).
    pub fn neighbour_at_border(&self, district: &str, edge: &str) -> Option<u16> {
        let picture = self.foreign.get(district)?;
        let next_to: BTreeSet<&String> = self.neighbours(edge).collect();
        picture
            .trains
            .iter()
            .filter(|t| t.handing_to.as_deref() != Some(self.district.as_str()))
            .find(|t| t.blocks.iter().any(|b| next_to.contains(b)))
            .map(|t| t.loco)
    }

    /// Whether a block is one of this district's.
    pub fn has_block(&self, block: &str) -> bool {
        self.blocks.contains(block)
    }

    pub fn neighbours(&self, block: &str) -> impl Iterator<Item = &String> {
        self.graph.get(block).into_iter().flatten()
    }

    /// An operator puts a train in a block of this district ("place loco here"). A train of
    /// that loco elsewhere in the district moves there.
    pub fn place(&mut self, loco: u16, block: &str) -> Result<Followed, String> {
        if !self.blocks.contains(block) {
            return Err(format!(
                "block {block} is not in district {}",
                self.district
            ));
        }
        let holds = |t: &FollowedTrain| {
            t.blocks.iter().any(|b| b == block) || (t.blocks.is_empty() && t.last == block)
        };
        if let Some(other) = self.trains.values().find(|t| t.loco != loco && holds(t)) {
            return Err(format!("block {block} holds train {} already", other.loco));
        }
        // The train covers the occupied blocks joined to this one that no other train holds:
        // a train is longer than a block.
        let mut blocks = Vec::new();
        if self.occupied.contains(block) {
            let held: BTreeSet<String> = self
                .trains
                .values()
                .filter(|t| t.loco != loco)
                .flat_map(|t| t.blocks.iter().cloned())
                .collect();
            let mut todo = vec![block.to_owned()];
            while let Some(next) = todo.pop() {
                if blocks.contains(&next) || held.contains(&next) || !self.occupied.contains(&next)
                {
                    continue;
                }
                todo.extend(
                    self.neighbours(&next)
                        .filter(|b| self.blocks.contains(*b))
                        .cloned(),
                );
                blocks.push(next);
            }
        }
        self.trains.insert(
            loco,
            FollowedTrain {
                loco,
                blocks,
                last: block.to_owned(),
                hops: 0,
                handing_to: None,
                came_from: None,
                entered_forward: None,
            },
        );
        self.version += 1;
        Ok(Followed::Placed {
            loco,
            block: block.to_owned(),
        })
    }

    /// A block of this district reports occupied or free. `moving` tells from the loco
    /// register whether a loco runs (a speed above stop): when more than one train could
    /// have entered a block, only one that runs can have.
    pub fn report(
        &mut self,
        block: &str,
        occupied: bool,
        moving: &dyn Fn(u16) -> bool,
    ) -> Vec<Followed> {
        if !self.blocks.contains(block) {
            return Vec::new();
        }
        let changed = if occupied {
            self.occupied.insert(block.to_owned())
        } else {
            self.occupied.remove(block)
        };
        if !changed {
            return Vec::new();
        }
        let found = if occupied {
            self.entered(block, moving)
        } else {
            self.left(block)
        };
        if found
            .iter()
            .any(|f| !matches!(f, Followed::Ghost { .. } | Followed::Ambiguous { .. }))
        {
            self.version += 1;
        }
        found
    }

    fn entered(&mut self, block: &str, moving: &dyn Fn(u16) -> bool) -> Vec<Followed> {
        // A train placed here before the detector reported.
        if let Some(train) = self
            .trains
            .values_mut()
            .find(|t| t.blocks.is_empty() && t.last == block)
        {
            train.blocks.push(block.to_owned());
            return vec![Followed::Entered {
                loco: train.loco,
                block: block.to_owned(),
            }];
        }
        let next_to: BTreeSet<&String> = self.neighbours(block).collect();
        let candidates: Vec<u16> = self
            .trains
            .values()
            .filter(|t| {
                let held: Vec<&String> = if t.blocks.is_empty() {
                    vec![&t.last]
                } else {
                    t.blocks.iter().collect()
                };
                held.iter().any(|b| next_to.contains(b))
            })
            .map(|t| t.loco)
            .collect();
        // Of several, the one that runs; a standing train enters no block.
        let running: Vec<u16> = candidates
            .iter()
            .copied()
            .filter(|loco| moving(*loco))
            .collect();
        let candidates = if candidates.len() > 1 && running.len() == 1 {
            running
        } else {
            candidates
        };
        // None of this district's: a train of a neighbouring district at the border.
        if candidates.is_empty() {
            let next_to: BTreeSet<String> = next_to.into_iter().cloned().collect();
            if let Some(found) = self.take_from_neighbour(block, &next_to, moving) {
                return vec![found];
            }
        }
        match candidates.as_slice() {
            [loco] => {
                let train = self.trains.get_mut(loco).expect("a candidate");
                train.came_from = Some(train.blocks.first().unwrap_or(&train.last).clone());
                train.blocks.insert(0, block.to_owned());
                train.last = block.to_owned();
                vec![Followed::Entered {
                    loco: *loco,
                    block: block.to_owned(),
                }]
            }
            [] => vec![Followed::Ghost {
                block: block.to_owned(),
            }],
            locos => vec![Followed::Ambiguous {
                block: block.to_owned(),
                locos: locos.to_vec(),
            }],
        }
    }

    /// A train of a neighbouring district next to `block`, taken over by this district.
    fn take_from_neighbour(
        &mut self,
        block: &str,
        next_to: &BTreeSet<String>,
        moving: &dyn Fn(u16) -> bool,
    ) -> Option<Followed> {
        let mut candidates: Vec<(String, FollowedTrain)> = Vec::new();
        for (district, picture) in &self.foreign {
            for train in &picture.trains {
                let held: Vec<&String> = if train.blocks.is_empty() {
                    vec![&train.last]
                } else {
                    train.blocks.iter().collect()
                };
                if train.handing_to.is_none() && held.iter().any(|b| next_to.contains(*b)) {
                    candidates.push((district.clone(), train.clone()));
                }
            }
        }
        if candidates.len() > 1 {
            candidates.retain(|(_, train)| moving(train.loco));
        }
        let [(from, train)] = <[(String, FollowedTrain); 1]>::try_from(candidates).ok()?;
        self.trains.insert(
            train.loco,
            FollowedTrain {
                loco: train.loco,
                blocks: vec![block.to_owned()],
                last: block.to_owned(),
                hops: train.hops + 1,
                handing_to: None,
                came_from: train.blocks.first().or(Some(&train.last)).cloned(),
                entered_forward: None,
            },
        );
        Some(Followed::Arrived {
            loco: train.loco,
            block: block.to_owned(),
            from,
        })
    }

    /// A neighbouring district's picture. A train of this district's that the neighbour has
    /// taken (with more hops) is let go: at once if it holds no block here any more, else
    /// once its last block here is free.
    pub fn neighbour_picture(&mut self, picture: TrainPicture) -> Vec<Followed> {
        if picture.district == self.district {
            return Vec::new();
        }
        let mut found = Vec::new();
        for theirs in &picture.trains {
            let Some(ours) = self.trains.get_mut(&theirs.loco) else {
                continue;
            };
            if theirs.hops <= ours.hops {
                continue;
            }
            if ours.blocks.is_empty() {
                self.trains.remove(&theirs.loco);
                found.push(Followed::HandedOver {
                    loco: theirs.loco,
                    to: picture.district.clone(),
                });
            } else {
                ours.handing_to = Some(picture.district.clone());
            }
        }
        if !found.is_empty() {
            self.version += 1;
        }
        self.foreign.insert(picture.district.clone(), picture);
        found
    }

    fn left(&mut self, block: &str) -> Vec<Followed> {
        let Some(train) = self
            .trains
            .values_mut()
            .find(|t| t.blocks.iter().any(|b| b == block))
        else {
            return Vec::new();
        };
        train.blocks.retain(|b| b != block);
        let between = train.blocks.is_empty();
        if !between && train.last == block {
            train.last = train.blocks[0].clone();
        }
        if let (true, Some(to)) = (between, train.handing_to.clone()) {
            let loco = train.loco;
            self.trains.remove(&loco);
            return vec![
                Followed::Left {
                    loco,
                    block: block.to_owned(),
                    between: false,
                },
                Followed::HandedOver { loco, to },
            ];
        }
        vec![Followed::Left {
            loco: train.loco,
            block: block.to_owned(),
            between,
        }]
    }

    /// Raised with every change of this district's picture.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn picture(&self) -> TrainPicture {
        TrainPicture {
            district: self.district.clone(),
            version: self.version,
            trains: self.trains.values().cloned().collect(),
        }
    }

    /// The block a train is in now (its head), or was last seen in.
    #[cfg(test)]
    pub fn where_is(&self, loco: u16) -> Option<&str> {
        self.trains
            .get(&loco)
            .map(|t| t.blocks.first().unwrap_or(&t.last).as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn topology() -> Topology {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/layout");
        let layout = crate::layout::load(&dir.join("bench-m5.json"), &dir).unwrap();
        Topology::build(&layout)
    }

    /// Adorf's platform 1 has a braking and a stop section; the line beyond the station
    /// follows the throat. Every block of the bench has a neighbour, and the graph is
    /// symmetric: if a is next to b, b is next to a.
    #[test]
    fn the_block_graph_joins_neighbours_both_ways() {
        let topology = topology();
        let graph = block_graph(&topology);
        assert!(
            graph["timo-adorf/adorf_1_b"].contains("timo-adorf/adorf_1_e"),
            "{:?}",
            graph["timo-adorf/adorf_1_b"]
        );
        for (block, neighbours) in &graph {
            assert!(!neighbours.is_empty(), "{block} has no neighbour");
            for other in neighbours {
                assert!(
                    graph[other].contains(block),
                    "{block} → {other} but not back"
                );
            }
        }
    }

    /// A train placed on platform 1 is followed out of Adorf as the simulator's detectors
    /// reported it in step 1: 203 and 242 (platform 1), then line_2.
    #[test]
    fn a_train_is_followed_block_by_block() {
        let topology = topology();
        let mut tracker = Tracker::new(&topology, "MacBook");
        tracker.report("timo-adorf/adorf_1_b", true, &|_| true);
        tracker.report("timo-adorf/adorf_1_e", true, &|_| true);
        assert!(tracker.place(4444, "timo-adorf/adorf_1_b").is_ok());
        // The other block it already stands in, joined to the first, is the same train's.
        assert_eq!(tracker.picture().trains[0].blocks.len(), 2);
        assert_eq!(
            tracker.report("timo-adorf/adorf_1_e", false, &|_| true),
            [Followed::Left {
                loco: 4444,
                block: "timo-adorf/adorf_1_e".into(),
                between: false
            }]
        );
        let mut seen = Vec::new();
        // The way out: whatever lies between platform 1 and line_2 is in the graph.
        let path = path_between(&tracker, "timo-adorf/adorf_1_b", "timo-2e96-west/line_2");
        for block in &path[1..] {
            seen.extend(tracker.report(block, true, &|_| true));
        }
        for block in &path[..path.len() - 1] {
            seen.extend(tracker.report(block, false, &|_| true));
        }
        assert!(
            seen.iter()
                .all(|f| !matches!(f, Followed::Ghost { .. } | Followed::Ambiguous { .. })),
            "{seen:#?}"
        );
        assert_eq!(tracker.where_is(4444), Some("timo-2e96-west/line_2"));
        assert_eq!(
            tracker.picture().trains[0].blocks,
            ["timo-2e96-west/line_2"]
        );
    }

    /// A shortest path of neighbouring blocks, for driving the tracker in tests.
    fn path_between(tracker: &Tracker<'_>, from: &str, to: &str) -> Vec<String> {
        let mut back: BTreeMap<String, String> = BTreeMap::new();
        let mut todo = std::collections::VecDeque::from([from.to_owned()]);
        while let Some(block) = todo.pop_front() {
            if block == to {
                break;
            }
            for next in tracker.neighbours(&block) {
                if next != from && !back.contains_key(next) {
                    back.insert(next.clone(), block.clone());
                    todo.push_back(next.clone());
                }
            }
        }
        let mut path = vec![to.to_owned()];
        while let Some(previous) = back.get(path.last().unwrap()) {
            path.push(previous.clone());
        }
        path.reverse();
        assert_eq!(path[0], from, "no way from {from} to {to}");
        path
    }

    /// Occupancy with no train next to it is a ghost; next to two trains it is ambiguous;
    /// the tracker never picks one.
    #[test]
    fn ghosts_and_ambiguities_go_to_the_operator() {
        let topology = topology();
        let mut tracker = Tracker::new(&topology, "MacBook");
        assert!(
            tracker
                .report("anna-bdorf/no-such-block", true, &|_| true)
                .is_empty(),
            "not a block of MacBook's"
        );
        let ghost = tracker.report("timo-adorf/adorf_2_b", true, &|_| true);
        assert!(
            matches!(ghost.as_slice(), [Followed::Ghost { .. }]),
            "{ghost:?}"
        );
        // Two trains on either side of one block: it could be either.
        let middle = "timo-2e96-west/line_2";
        let sides: Vec<String> = tracker.neighbours(middle).cloned().collect();
        assert!(sides.len() >= 2, "{sides:?}");
        tracker.place(1, &sides[0]).unwrap();
        tracker.place(2, &sides[1]).unwrap();
        let found = tracker.report(middle, true, &|_| true);
        assert!(
            matches!(found.as_slice(), [Followed::Ambiguous { locos, .. }] if locos.len() == 2),
            "{found:?}"
        );
        assert_eq!(tracker.where_is(1), Some(sides[0].as_str()));
        // When only one of them runs, it is that one.
        tracker.report(middle, false, &|_| true);
        let found = tracker.report(middle, true, &|loco| loco == 2);
        assert_eq!(
            found,
            [Followed::Entered {
                loco: 2,
                block: middle.into()
            }]
        );
    }

    /// With a turnout's position known, only the leg it is set to leads on: the platform
    /// whose turnout is set against it is no neighbour of the line.
    #[test]
    fn a_set_turnout_narrows_the_neighbours() {
        let topology = topology();
        let mut tracker = Tracker::new(&topology, "MacBook");
        let line = "timo-2e96-west/line_2";
        let all: BTreeSet<String> = tracker.neighbours(line).cloned().collect();
        let turnouts: Vec<String> = topology.turnouts_in("MacBook").map(|t| t.key()).collect();
        for turnout in &turnouts {
            tracker.set_turnout(turnout, true);
        }
        let set: BTreeSet<String> = tracker.neighbours(line).cloned().collect();
        assert!(set.is_subset(&all), "{set:?} {all:?}");
        assert!(set.len() < all.len(), "{set:?} {all:?}");
    }

    /// The handoff, as stage 2 of the district simulators showed it: train 4444 in
    /// MacBook's border block edge_1 runs into the M5StickC's t1. The M5StickC knows it from
    /// MacBook's picture and takes it; MacBook lets go once edge_1 is free; an old copy of
    /// MacBook's picture does not take it back.
    #[test]
    fn a_train_is_handed_over_at_the_border() {
        let topology = topology();
        let (edge, t1) = ("timo-2e96-border/edge_1", "m5-2e96-in/t1");
        let mut macbook = Tracker::new(&topology, "MacBook");
        let mut m5 = Tracker::new(&topology, "M5StickC");
        assert!(
            macbook.neighbours(edge).any(|b| b == t1),
            "{:?}",
            macbook.neighbours(edge).collect::<Vec<_>>()
        );
        macbook.report(edge, true, &|_| true);
        macbook.place(4444, edge).unwrap();
        m5.neighbour_picture(macbook.picture());
        // Without MacBook's picture it would be a ghost; with it, MacBook's train.
        let arrived = m5.report(t1, true, &|_| true);
        assert_eq!(
            arrived,
            [Followed::Arrived {
                loco: 4444,
                block: t1.into(),
                from: "MacBook".into()
            }]
        );
        assert_eq!(m5.picture().trains[0].hops, 1);
        // MacBook hears of it while its tail is still in edge_1, and lets go when that frees.
        assert!(macbook.neighbour_picture(m5.picture()).is_empty());
        let freed = macbook.report(edge, false, &|_| true);
        assert!(
            freed.contains(&Followed::HandedOver {
                loco: 4444,
                to: "M5StickC".into()
            }),
            "{freed:?}"
        );
        assert!(macbook.picture().trains.is_empty());
        // An old copy of MacBook's picture, with 4444 at fewer hops, changes nothing.
        let mut old = m5.picture();
        old.district = "MacBook".into();
        old.trains[0].hops = 0;
        m5.neighbour_picture(old);
        assert_eq!(m5.picture().trains.len(), 1);
    }

    /// MacBook's border to the M5StickC is its edge blocks in front of it; a train placed in
    /// one runs towards it, a train that came in over it does not; a neighbour's train in
    /// front of the edge block is seen there.
    #[test]
    fn trains_towards_a_border_and_the_neighbour_at_it() {
        let topology = topology();
        let (edge, t1) = ("timo-2e96-border/edge_1", "m5-2e96-in/t1");
        let mut macbook = Tracker::new(&topology, "MacBook");
        let borders = macbook.borders();
        assert!(borders["M5StickC"].contains(edge), "{borders:?}");
        assert!(!borders.contains_key("MacBook"));
        macbook.report(edge, true, &|_| true);
        macbook.place(4444, edge).unwrap();
        assert_eq!(
            macbook.towards_borders(&borders, &|_| Some(true)),
            [(4444, "M5StickC".to_owned(), edge.to_owned())]
        );
        // The M5StickC's train in t1, in front of edge_1, is at the border.
        let mut m5 = Tracker::new(&topology, "M5StickC");
        m5.report(t1, true, &|_| true);
        m5.place(5, t1).unwrap();
        macbook.neighbour_picture(m5.picture());
        assert_eq!(macbook.neighbour_at_border("M5StickC", edge), Some(5));
        // A train that came into MacBook over that border runs away from it.
        let mut m5 = Tracker::new(&topology, "M5StickC");
        m5.report(t1, true, &|_| true);
        m5.place(6, t1).unwrap();
        let mut macbook = Tracker::new(&topology, "MacBook");
        macbook.neighbour_picture(m5.picture());
        assert!(matches!(
            macbook.report(edge, true, &|_| true).as_slice(),
            [Followed::Arrived { .. }]
        ));
        assert!(macbook
            .towards_borders(&borders, &|_| Some(true))
            .is_empty());
    }

    /// A train that ran into the edge block from inside and has turned back runs away from
    /// the border; one that came in over it and turned back runs towards it again.
    #[test]
    fn a_train_that_turns_back_runs_the_other_way() {
        let topology = topology();
        let (inner, edge, t1) = (
            "timo-2e96-west/line_1",
            "timo-2e96-border/edge_1",
            "m5-2e96-in/t1",
        );
        let mut macbook = Tracker::new(&topology, "MacBook");
        let borders = macbook.borders();
        // From inside: the edge block's neighbour on MacBook's side.
        let inside = macbook
            .neighbours(edge)
            .find(|b| b.starts_with("timo-"))
            .cloned()
            .unwrap_or_else(|| inner.to_owned());
        macbook.report(&inside, true, &|_| true);
        macbook.place(4444, &inside).unwrap();
        macbook.report(edge, true, &|_| true);
        macbook.note_direction(4444, Some(true));
        assert_eq!(
            macbook.towards_borders(&borders, &|_| Some(true)).len(),
            1,
            "runs on towards it"
        );
        assert!(
            macbook
                .towards_borders(&borders, &|_| Some(false))
                .is_empty(),
            "turned back"
        );
        // Came in over the border, then turned back towards it.
        let mut m5 = Tracker::new(&topology, "M5StickC");
        m5.report(t1, true, &|_| true);
        m5.place(6, t1).unwrap();
        let mut macbook = Tracker::new(&topology, "MacBook");
        macbook.neighbour_picture(m5.picture());
        macbook.report(edge, true, &|_| true);
        macbook.note_direction(6, Some(false));
        assert!(
            macbook
                .towards_borders(&borders, &|_| Some(false))
                .is_empty(),
            "runs in"
        );
        assert_eq!(
            macbook.towards_borders(&borders, &|_| Some(true)).len(),
            1,
            "turned back to it"
        );
    }

    /// Without a neighbour's picture, the occupancy at the border is a ghost.
    #[test]
    fn a_train_from_an_unknown_neighbour_is_a_ghost() {
        let topology = topology();
        let mut m5 = Tracker::new(&topology, "M5StickC");
        assert!(matches!(
            m5.report("m5-2e96-in/t1", true, &|_| true).as_slice(),
            [Followed::Ghost { .. }]
        ));
    }

    #[test]
    fn a_train_is_only_placed_in_its_district_and_on_a_free_block() {
        let topology = topology();
        let mut tracker = Tracker::new(&topology, "MacBook");
        assert!(tracker.place(1, "anna-bdorf/bdorf_1_b").is_err());
        tracker.place(1, "timo-adorf/adorf_1_b").unwrap();
        assert!(tracker.place(2, "timo-adorf/adorf_1_b").is_err());
    }
}
