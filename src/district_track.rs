//! A node's own track: its district's part of the layout, and what each device address on
//! its buses means (wiki "District Simulators and Train Tracking", step 3). With it the
//! node no longer reports "LocoNet sensor 203 high" but "block timo-adorf/adorf_1_b
//! occupied", and it keeps the state of its blocks and turnouts for the steps that follow
//! (following trains, the handoff).
//!
//! **Any bus.** The map is keyed by the address space a bus has (`address_space` in
//! `layout.rs`: LocoNet sensors and LocoNet switches apart, DCC accessories, DCC-EX, board
//! pins) and an address. Addresses are numbers today; RocNet modules (node and port) and
//! CBUS or LCC (events) bring other kinds, which the `Address` type already has room for.
//! An address only has to be unique within a district: each district has its own bus.

use crate::layout::{address_space, Layout};
use crate::topology::Topology;
use std::collections::BTreeMap;

/// A device's address on its bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Address {
    /// LocoNet, DCC accessories, DCC-EX, board pins.
    Number(u16),
    /// A RocNet module's port (later).
    #[allow(dead_code)]
    NodePort { node: u16, port: u16 },
    /// A CBUS or LCC event (later).
    #[allow(dead_code)]
    Event(u64),
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Address::Number(n) => write!(f, "{n}"),
            Address::NodePort { node, port } => write!(f, "{node}:{port}"),
            Address::Event(event) => write!(f, "event {event:016X}"),
        }
    }
}

/// What a device on the district's track is.
#[derive(Debug, Clone, PartialEq)]
pub enum Meaning {
    /// An occupancy detector: the block it covers (`module/block`).
    Block { block: String, device: String },
    /// A turnout's drive (`module/turnout`).
    Turnout { turnout: String, device: String },
    /// Any other device: a signal, a sensor at a point, a light (`module/device`).
    Device { device: String, kind: String },
}

/// A change on the district's track, named.
#[derive(Debug, Clone, PartialEq)]
pub enum TrackChange {
    Block {
        block: String,
        occupied: bool,
    },
    Turnout {
        turnout: String,
        closed: bool,
    },
    /// A known device that is neither, such as a sensor at a point.
    Device {
        device: String,
        kind: String,
        on: bool,
    },
    /// An address that is no device of this district.
    Unknown {
        space: String,
        address: Address,
    },
}

impl std::fmt::Display for TrackChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrackChange::Block { block, occupied } => {
                write!(
                    f,
                    "block {block} {}",
                    if *occupied { "occupied" } else { "free" }
                )
            }
            TrackChange::Turnout { turnout, closed } => {
                write!(
                    f,
                    "turnout {turnout} {}",
                    if *closed { "closed" } else { "thrown" }
                )
            }
            TrackChange::Device { device, kind, on } => {
                write!(f, "{kind} {device} {}", if *on { "on" } else { "off" })
            }
            TrackChange::Unknown { space, address } => {
                write!(f, "{space} {address} is no device of this district")
            }
        }
    }
}

/// The district's track state, as the node publishes it on `hive/<session>/node/<id>/track`
/// (Hive-Rail's own state, so JSON; the sensor reports themselves go out as RocNet).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TrackState {
    pub district: String,
    /// Blocks occupied now, `module/block`.
    pub occupied: Vec<String>,
    /// Turnout positions as last set, `module/turnout` → closed; unknown ones left out.
    pub turnouts: BTreeMap<String, bool>,
}

pub struct DistrictTrack {
    pub district: String,
    map: BTreeMap<(&'static str, Address), Meaning>,
    /// Every block of the district, and whether it is occupied, once a detector said so.
    blocks: BTreeMap<String, Option<bool>>,
    /// Every turnout of the district, and its position once known.
    turnouts: BTreeMap<String, Option<bool>>,
}

impl DistrictTrack {
    /// The district named `district` (a node's `--id`); an error when the layout has none.
    pub fn build(layout: &Layout, topology: &Topology, district: &str) -> Result<Self, String> {
        let modules: Vec<&String> = layout
            .file
            .districts
            .iter()
            .filter(|d| d.node == district)
            .flat_map(|d| &d.modules)
            .collect();
        if modules.is_empty() {
            return Err(format!("the layout has no district {district}"));
        }
        let drives: BTreeMap<String, String> = topology
            .turnouts()
            .filter(|t| t.district == district)
            .filter_map(|t| Some((t.device.clone()?, format!("{}/{}", t.module, t.id))))
            .collect();
        let mut map = BTreeMap::new();
        for module_id in modules {
            let Some((module, _)) = layout.modules.get(module_id) else {
                continue;
            };
            for device in &module.devices {
                let key = format!("{module_id}/{}", device.id);
                // An occupancy detector's block: named by the device, or by the block.
                let block = device.block.clone().or_else(|| {
                    module
                        .blocks
                        .iter()
                        .find(|block| block.occupancy.as_deref() == Some(device.id.as_str()))
                        .map(|block| block.id.clone())
                });
                let meaning = if let Some(turnout) = drives.get(&key) {
                    Meaning::Turnout {
                        turnout: turnout.clone(),
                        device: key,
                    }
                } else if let Some(block) = block {
                    Meaning::Block {
                        block: format!("{module_id}/{block}"),
                        device: key,
                    }
                } else {
                    Meaning::Device {
                        device: key,
                        kind: device.kind.clone(),
                    }
                };
                map.insert(
                    (address_space(device), Address::Number(device.address)),
                    meaning,
                );
            }
        }
        let blocks = topology
            .blocks
            .iter()
            .filter(|(_, b)| b.district == district)
            .map(|(key, _)| (key.clone(), None))
            .collect();
        let turnouts = topology
            .turnouts()
            .filter(|t| t.district == district)
            .map(|t| (format!("{}/{}", t.module, t.id), None))
            .collect();
        Ok(Self {
            district: district.to_owned(),
            map,
            blocks,
            turnouts,
        })
    }

    /// What an address in an address space ("LocoNet sensor") is in this district.
    pub fn meaning(&self, space: &str, address: Address) -> Option<&Meaning> {
        self.map
            .iter()
            .find(|((s, a), _)| *s == space && *a == address)
            .map(|(_, meaning)| meaning)
    }

    /// A sensor input: a detector reports its block, another sensor itself. Repeats of a
    /// state already known change nothing and give `None`.
    pub fn sensor(&mut self, space: &str, address: Address, on: bool) -> Option<TrackChange> {
        match self.meaning(space, address).cloned() {
            Some(Meaning::Block { block, .. }) => {
                let state = self.blocks.entry(block.clone()).or_default();
                (*state != Some(on)).then(|| {
                    *state = Some(on);
                    TrackChange::Block {
                        block,
                        occupied: on,
                    }
                })
            }
            Some(Meaning::Device { device, kind }) => {
                Some(TrackChange::Device { device, kind, on })
            }
            Some(Meaning::Turnout { device, .. }) => Some(TrackChange::Device {
                device,
                kind: "turnout drive".to_owned(),
                on,
            }),
            None => Some(TrackChange::Unknown {
                space: space.to_owned(),
                address,
            }),
        }
    }

    /// A switch command: the turnout it sets, or the device it switches.
    pub fn switch(&mut self, space: &str, address: Address, closed: bool) -> Option<TrackChange> {
        match self.meaning(space, address).cloned() {
            Some(Meaning::Turnout { turnout, .. }) => {
                let state = self.turnouts.entry(turnout.clone()).or_default();
                (*state != Some(closed)).then(|| {
                    *state = Some(closed);
                    TrackChange::Turnout { turnout, closed }
                })
            }
            Some(Meaning::Device { device, kind }) => Some(TrackChange::Device {
                device,
                kind,
                on: closed,
            }),
            Some(Meaning::Block { .. }) => None,
            None => Some(TrackChange::Unknown {
                space: space.to_owned(),
                address,
            }),
        }
    }

    pub fn state(&self) -> TrackState {
        TrackState {
            district: self.district.clone(),
            occupied: self
                .blocks
                .iter()
                .filter(|(_, occupied)| **occupied == Some(true))
                .map(|(block, _)| block.clone())
                .collect(),
            turnouts: self
                .turnouts
                .iter()
                .filter_map(|(turnout, closed)| Some((turnout.clone(), (*closed)?)))
                .collect(),
        }
    }

    /// How many blocks, turnouts and devices the district has, for the log.
    pub fn summary(&self) -> String {
        format!(
            "{} blocks, {} turnouts, {} device addresses",
            self.blocks.len(),
            self.turnouts.len(),
            self.map.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn layout() -> Layout {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/layout");
        crate::layout::load(&dir.join("bench-m5.json"), &dir).unwrap()
    }

    #[test]
    fn a_node_only_has_a_district_of_the_layout() {
        let layout = layout();
        let topology = Topology::build(&layout);
        assert!(DistrictTrack::build(&layout, &topology, "Nowhere").is_err());
        assert!(DistrictTrack::build(&layout, &topology, "MacBook").is_ok());
    }

    /// LocoNet sensor 203 in MacBook is Adorf's platform 1 braking section, as the district
    /// simulator reported it in step 1; it is named, and a repeat changes nothing.
    #[test]
    fn a_detector_names_its_block() {
        let layout = layout();
        let topology = Topology::build(&layout);
        let mut track = DistrictTrack::build(&layout, &topology, "MacBook").unwrap();
        let change = track.sensor("LocoNet sensor", Address::Number(203), true);
        assert_eq!(
            change,
            Some(TrackChange::Block {
                block: "timo-adorf/adorf_1_b".into(),
                occupied: true
            })
        );
        assert_eq!(
            track.sensor("LocoNet sensor", Address::Number(203), true),
            None
        );
        assert_eq!(track.state().occupied, ["timo-adorf/adorf_1_b"]);
        assert!(matches!(
            track.sensor("LocoNet sensor", Address::Number(3999), true),
            Some(TrackChange::Unknown { .. })
        ));
    }

    /// LocoNet switch 21 sets Adorf's turnout 3, and its position is kept.
    #[test]
    fn a_switch_command_sets_its_turnout() {
        let layout = layout();
        let topology = Topology::build(&layout);
        let mut track = DistrictTrack::build(&layout, &topology, "MacBook").unwrap();
        assert_eq!(
            track.switch("LocoNet switch", Address::Number(21), false),
            Some(TrackChange::Turnout {
                turnout: "timo-adorf/3".into(),
                closed: false
            })
        );
        assert_eq!(track.state().turnouts.get("timo-adorf/3"), Some(&false));
        assert_eq!(
            track.switch("LocoNet switch", Address::Number(21), false),
            None
        );
    }

    /// An address that two districts both use means a different device in each: every
    /// district has its own bus.
    #[test]
    fn one_address_means_a_different_device_in_each_district() {
        let layout = layout();
        let topology = Topology::build(&layout);
        let districts: Vec<DistrictTrack> = layout
            .file
            .districts
            .iter()
            .map(|d| DistrictTrack::build(&layout, &topology, &d.node).unwrap())
            .collect();
        let shared = districts[0]
            .map
            .iter()
            .find_map(|((space, address), meaning)| {
                districts[1..].iter().find_map(|other| {
                    other
                        .meaning(space, *address)
                        .map(|m| (m.clone(), meaning.clone()))
                })
            })
            .expect("an address used in two districts");
        assert_ne!(shared.0, shared.1);
    }
}
