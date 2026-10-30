//! Module and layout files (schema version 2), as designed in the wiki's "Module and
//! Layout Files": every module owner describes their modules once, the planning team's
//! layout file puts them together for one meeting, and Hive-Rail checks both before the
//! meeting, so problems are found at home and not on the floor.
//!
//! Additions to the wiki design, from building it:
//! - LocoNet has two address spaces: sensors (`OPC_INPUT_REP`) and switches and other
//!   outputs (`OPC_SW_REQ`). A sensor and a turnout with the same number do not clash.
//! - A district border carries only standard single or double track: an end at a border
//!   may have one or two tracks, never more.
//! - The checksum of a module file is a 64-bit FNV-1a hash of the file, written as 16 hex
//!   digits: a check that the file changed, not a security measure.

// Names, stations, platforms, aspects and geometry are read by the layout graph, the
// simulator and the web view, which come next.
#![allow(dead_code)]

use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt, fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

pub const SCHEMA_VERSION: u32 = 2;

// ---------------------------------------------------------------------------------------
// The module file
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleFile {
    pub schema_version: u32,
    pub module_id: String,
    pub friendly_name: String,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub scale: Option<String>,
    pub length_mm: u32,
    pub ends: Vec<End>,
    pub track: Vec<TrackPiece>,
    #[serde(default)]
    pub turnouts: Vec<Turnout>,
    #[serde(default)]
    pub stations: Vec<Station>,
    #[serde(default)]
    pub blocks: Vec<Block>,
    #[serde(default)]
    pub devices: Vec<Device>,
    #[serde(default)]
    pub buildings: Vec<Building>,
    #[serde(default)]
    pub crossings: Vec<Crossing>,
    #[serde(default)]
    pub geometry: Option<Geometry>,
}

/// A building on the module, with the lights in it (devices of type `light`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Building {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub lights: Vec<String>,
}

/// A level crossing: where the road crosses each track, its warning lights and barriers
/// (outputs), and the sensors that close it before a train and open it after.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Crossing {
    pub id: String,
    pub name: String,
    pub tracks: Vec<CrossingTrack>,
    /// The warning lights: a device of type `light`.
    #[serde(default)]
    pub lights: Option<String>,
    /// The barriers: a device of type `animation`.
    #[serde(default)]
    pub barriers: Option<String>,
    /// Sensors that close the crossing when a train approaches.
    #[serde(default)]
    pub close: Vec<String>,
    /// Sensors that open it again once the train has passed.
    #[serde(default)]
    pub open: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrossingTrack {
    pub track: String,
    pub at_mm: u32,
}

/// A module end: its FREMO end profile and its tracks, from left to right as seen
/// standing on the module and looking out of that end.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct End {
    pub id: String,
    pub profile: String,
    pub tracks: Vec<String>,
}

/// A piece of track between two points: an end track (`A.1`), a turnout leg
/// (`W1.trunk`, `W1.straight`, `W1.diverging`) or a buffer stop (`end`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackPiece {
    pub id: String,
    pub from: String,
    pub to: String,
    pub length_mm: u32,
    /// The regular running direction on double track, for example `A>B`.
    #[serde(default)]
    pub direction: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Turnout {
    pub id: String,
    /// The device that switches it.
    #[serde(default)]
    pub device: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Station {
    pub name: String,
    #[serde(default)]
    pub code: Option<String>,
    pub tracks: Vec<StationTrack>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StationTrack {
    pub name: String,
    pub track: String,
    pub usable_mm: u32,
    #[serde(default)]
    pub platform: bool,
}

/// A stretch of track for occupancy and train positions, bounded by two rail cuts.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    pub id: String,
    pub track: String,
    /// The two cuts along the track, from its `from` end.
    pub cuts_mm: [u32; 2],
    /// `left`, `right`, `outer` or `both` (a transfer block).
    pub detected_rail: String,
    #[serde(default)]
    pub occupancy: Option<String>,
    /// `edge`, `transfer` or `entry` at a district border.
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default, rename = "for")]
    pub direction: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// `dcc`, `loconet`, `dccex` or `board`.
    pub bus: String,
    pub address: u16,
    #[serde(default)]
    pub block: Option<String>,
    #[serde(default)]
    pub track: Option<String>,
    #[serde(default)]
    pub at_mm: Option<u32>,
    /// A point such as an end track, `A.1`.
    #[serde(default)]
    pub at: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default, rename = "for")]
    pub direction: Option<String>,
    #[serde(default)]
    pub aspects: Option<serde_json::Value>,
    #[serde(default)]
    pub functions: Option<serde_json::Value>,
}

/// Optional, for drawing: millimetres on the module seen from above, x from end A towards
/// end B, y from the front edge (the operator's side) towards the back.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Geometry {
    #[serde(default)]
    pub outline: Vec<[f64; 2]>,
    #[serde(default)]
    pub ends: BTreeMap<String, EndPlace>,
    #[serde(default)]
    pub points: BTreeMap<String, [f64; 2]>,
    #[serde(default)]
    pub track: BTreeMap<String, Vec<[f64; 2]>>,
    #[serde(default)]
    pub platforms: BTreeMap<String, Vec<[f64; 2]>>,
    #[serde(default)]
    pub devices: BTreeMap<String, [f64; 2]>,
    /// Building outlines, by building id.
    #[serde(default)]
    pub buildings: BTreeMap<String, Vec<[f64; 2]>>,
    /// Roads, by name: a polyline, for example across a level crossing.
    #[serde(default)]
    pub roads: BTreeMap<String, Vec<[f64; 2]>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndPlace {
    pub at: [f64; 2],
    /// The direction out of the end, in degrees, 0 along x.
    pub facing: f64,
}

// ---------------------------------------------------------------------------------------
// The layout file
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutFile {
    pub schema_version: u32,
    pub meeting: String,
    pub modules: Vec<LayoutModule>,
    pub connections: Vec<Connection>,
    pub districts: Vec<District>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutModule {
    pub module_id: String,
    /// The module file's checksum when the layout was made (16 hex digits, or a prefix).
    #[serde(default)]
    pub checksum: Option<String>,
}

/// Two module ends that meet, written `module.end`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub a: String,
    pub b: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct District {
    pub node: String,
    pub modules: Vec<String>,
}

// ---------------------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum LayoutError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("invalid {path}: {source}")]
    Parse {
        path: String,
        source: serde_json::Error,
    },
}

/// A layout with its module files, as loaded; `check` finds what is wrong with it.
#[derive(Debug)]
pub struct Layout {
    pub file: LayoutFile,
    /// The module files found, by module id, with their checksum.
    pub modules: BTreeMap<String, (ModuleFile, String)>,
    /// Problems found while loading: a module file missing or unreadable.
    pub load_errors: Vec<String>,
}

/// Loads a layout file and the module files it names, `<module_id>.json` in `modules_dir`.
pub fn load(path: &Path, modules_dir: &Path) -> Result<Layout, LayoutError> {
    let file: LayoutFile = read_json(path)?;
    let mut modules = BTreeMap::new();
    let mut load_errors = Vec::new();
    for listed in &file.modules {
        let module_path: PathBuf = modules_dir.join(format!("{}.json", listed.module_id));
        match fs::read(&module_path) {
            Ok(bytes) => match serde_json::from_slice::<ModuleFile>(&bytes) {
                Ok(module) => {
                    modules.insert(listed.module_id.clone(), (module, checksum(&bytes)));
                }
                Err(error) => load_errors.push(format!("{}: {error}", module_path.display())),
            },
            Err(error) => load_errors.push(format!(
                "module {}: cannot read {}: {error}",
                listed.module_id,
                module_path.display()
            )),
        }
    }
    Ok(Layout {
        file,
        modules,
        load_errors,
    })
}

/// Loads one module file.
pub fn load_module(path: &Path) -> Result<ModuleFile, LayoutError> {
    read_json(path)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, LayoutError> {
    let text = fs::read_to_string(path).map_err(|source| LayoutError::Read {
        path: path.display().to_string(),
        source,
    })?;
    serde_json::from_str(&text).map_err(|source| LayoutError::Parse {
        path: path.display().to_string(),
        source,
    })
}

/// A module file's checksum: 64-bit FNV-1a, 16 hex digits.
pub fn checksum(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("{hash:016x}")
}

// ---------------------------------------------------------------------------------------
// Checks
// ---------------------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct Report {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl Report {
    fn error(&mut self, text: String) {
        self.errors.push(text);
    }

    fn warning(&mut self, text: String) {
        self.warnings.push(text);
    }
}

const TURNOUT_LEGS: [&str; 3] = ["trunk", "straight", "diverging"];
const DEVICE_TYPES: [&str; 9] = [
    "turnout",
    "signal",
    "sensor",
    "occupancy",
    "relay",
    "button",
    "transponder",
    "light",
    "animation",
];
const BUSES: [&str; 4] = ["dcc", "loconet", "dccex", "board"];
const DETECTED_RAILS: [&str; 4] = ["left", "right", "outer", "both"];
const BLOCK_ROLES: [&str; 3] = ["edge", "transfer", "entry"];
const DEVICE_ROLES: [&str; 6] = [
    "entry",
    "stop",
    "edge",
    "boundary",
    "transfer_entry",
    "transfer_relay",
];

/// A point a track piece starts or ends at.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Point {
    End { end: String, track: String },
    Leg { turnout: String, leg: String },
    Buffer,
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Point::End { end, track } => write!(f, "{end}.{track}"),
            Point::Leg { turnout, leg } => write!(f, "{turnout}.{leg}"),
            Point::Buffer => write!(f, "end"),
        }
    }
}

impl ModuleFile {
    pub(crate) fn end(&self, id: &str) -> Option<&End> {
        self.ends.iter().find(|end| end.id == id)
    }

    fn piece(&self, id: &str) -> Option<&TrackPiece> {
        self.track.iter().find(|piece| piece.id == id)
    }

    fn device(&self, id: &str) -> Option<&Device> {
        self.devices.iter().find(|device| device.id == id)
    }

    pub(crate) fn point(&self, text: &str) -> Result<Point, String> {
        if text == "end" {
            return Ok(Point::Buffer);
        }
        let (left, right) = text
            .split_once('.')
            .ok_or_else(|| format!("'{text}' is not a point (A.1, W1.straight or end)"))?;
        if let Some(end) = self.end(left) {
            return if end.tracks.iter().any(|track| track == right) {
                Ok(Point::End {
                    end: left.to_owned(),
                    track: right.to_owned(),
                })
            } else {
                Err(format!("end {left} has no track '{right}'"))
            };
        }
        if self.turnouts.iter().any(|turnout| turnout.id == left) {
            return if TURNOUT_LEGS.contains(&right) {
                Ok(Point::Leg {
                    turnout: left.to_owned(),
                    leg: right.to_owned(),
                })
            } else {
                Err(format!(
                    "turnout {left} has no leg '{right}' (trunk, straight, diverging)"
                ))
            };
        }
        Err(format!("'{left}' is neither an end nor a turnout"))
    }

    /// The track piece that leaves this end on this track.
    pub(crate) fn piece_at(&self, end: &str, track: &str) -> Option<&TrackPiece> {
        let point = format!("{end}.{track}");
        self.track
            .iter()
            .find(|piece| piece.from == point || piece.to == point)
    }

    /// Checks one module file on its own; `prefix` names it in the messages.
    pub fn check(&self, report: &mut Report) {
        let name = &self.module_id;
        let error = |report: &mut Report, text: String| report.error(format!("{name}: {text}"));
        if self.schema_version != SCHEMA_VERSION {
            error(
                report,
                format!(
                    "schema_version {} (expected {SCHEMA_VERSION})",
                    self.schema_version
                ),
            );
        }
        if self.module_id.trim().is_empty() {
            report.error("a module file has an empty module_id".to_owned());
        }
        if self.length_mm == 0 {
            error(report, "length_mm must be more than 0".to_owned());
        }
        for (kind, ids) in [
            ("end", self.ends.iter().map(|x| &x.id).collect::<Vec<_>>()),
            ("track", self.track.iter().map(|x| &x.id).collect()),
            ("turnout", self.turnouts.iter().map(|x| &x.id).collect()),
            ("block", self.blocks.iter().map(|x| &x.id).collect()),
            ("device", self.devices.iter().map(|x| &x.id).collect()),
            ("building", self.buildings.iter().map(|x| &x.id).collect()),
            ("crossing", self.crossings.iter().map(|x| &x.id).collect()),
        ] {
            let mut seen = BTreeSet::new();
            for id in ids {
                if !seen.insert(id) {
                    error(report, format!("{kind} id '{id}' is used twice"));
                }
            }
        }
        for end in &self.ends {
            if end.tracks.is_empty() {
                error(report, format!("end {} has no tracks", end.id));
            }
            let mut seen = BTreeSet::new();
            for track in &end.tracks {
                if !seen.insert(track) {
                    error(
                        report,
                        format!("end {} lists track '{track}' twice", end.id),
                    );
                }
            }
        }

        // The track graph: every end track and every turnout leg is used exactly once.
        let mut uses: BTreeMap<Point, Vec<&str>> = BTreeMap::new();
        for piece in &self.track {
            if piece.length_mm == 0 {
                error(report, format!("track {} has length_mm 0", piece.id));
            }
            for text in [&piece.from, &piece.to] {
                match self.point(text) {
                    Ok(Point::Buffer) => {}
                    Ok(point) => uses.entry(point).or_default().push(&piece.id),
                    Err(problem) => error(report, format!("track {}: {problem}", piece.id)),
                }
            }
            if let Some(direction) = &piece.direction {
                if let Err(problem) = self.check_direction(direction) {
                    error(report, format!("track {}: {problem}", piece.id));
                }
            }
        }
        let mut expected: Vec<Point> = Vec::new();
        for end in &self.ends {
            for track in &end.tracks {
                expected.push(Point::End {
                    end: end.id.clone(),
                    track: track.clone(),
                });
            }
        }
        for turnout in &self.turnouts {
            for leg in TURNOUT_LEGS {
                expected.push(Point::Leg {
                    turnout: turnout.id.clone(),
                    leg: leg.to_owned(),
                });
            }
            if let Some(device) = &turnout.device {
                match self.device(device) {
                    Some(found) if found.kind == "turnout" => {}
                    Some(found) => error(
                        report,
                        format!(
                            "turnout {}: device {device} is a {}, not a turnout",
                            turnout.id, found.kind
                        ),
                    ),
                    None => error(
                        report,
                        format!("turnout {}: no device '{device}'", turnout.id),
                    ),
                }
            }
        }
        for point in expected {
            match uses.get(&point).map(Vec::len).unwrap_or(0) {
                1 => {}
                0 => error(report, format!("{point} is not connected to any track")),
                count => error(
                    report,
                    format!("{point} is connected to {count} tracks; it can have one"),
                ),
            }
        }

        for block in &self.blocks {
            let prefix = format!("block {}", block.id);
            match self.piece(&block.track) {
                Some(piece) => {
                    let [start, stop] = block.cuts_mm;
                    if start >= stop || stop > piece.length_mm {
                        error(
                            report,
                            format!(
                                "{prefix}: cuts_mm [{start}, {stop}] must rise and lie on track {} ({} mm)",
                                piece.id, piece.length_mm
                            ),
                        );
                    }
                }
                None => error(report, format!("{prefix}: no track '{}'", block.track)),
            }
            if !DETECTED_RAILS.contains(&block.detected_rail.as_str()) {
                error(
                    report,
                    format!(
                        "{prefix}: detected_rail '{}' is not one of {DETECTED_RAILS:?}",
                        block.detected_rail
                    ),
                );
            }
            if let Some(occupancy) = &block.occupancy {
                match self.device(occupancy) {
                    Some(device) if device.kind == "occupancy" => {}
                    Some(device) => error(
                        report,
                        format!(
                            "{prefix}: {occupancy} is a {}, not an occupancy detector",
                            device.kind
                        ),
                    ),
                    None => error(report, format!("{prefix}: no device '{occupancy}'")),
                }
            }
            if let Some(role) = &block.role {
                if !BLOCK_ROLES.contains(&role.as_str()) {
                    error(
                        report,
                        format!("{prefix}: role '{role}' is not one of {BLOCK_ROLES:?}"),
                    );
                }
                if role == "transfer" && block.detected_rail != "both" {
                    error(
                        report,
                        format!(
                            "{prefix}: a transfer block is cut in both rails (detected_rail \"both\")"
                        ),
                    );
                }
            }
            if let Some(direction) = &block.direction {
                if let Err(problem) = self.check_direction(direction) {
                    error(report, format!("{prefix}: {problem}"));
                }
            }
        }

        for device in &self.devices {
            let prefix = format!("device {}", device.id);
            if !DEVICE_TYPES.contains(&device.kind.as_str()) {
                error(
                    report,
                    format!(
                        "{prefix}: type '{}' is not one of {DEVICE_TYPES:?}",
                        device.kind
                    ),
                );
            }
            if !BUSES.contains(&device.bus.as_str()) {
                error(
                    report,
                    format!("{prefix}: bus '{}' is not one of {BUSES:?}", device.bus),
                );
            }
            if let Some(block) = &device.block {
                if !self.blocks.iter().any(|b| &b.id == block) {
                    error(report, format!("{prefix}: no block '{block}'"));
                }
            }
            if let Some(track) = &device.track {
                match self.piece(track) {
                    Some(piece) => {
                        if device.at_mm.is_some_and(|at| at > piece.length_mm) {
                            error(
                                report,
                                format!(
                                    "{prefix}: at_mm is beyond the end of track {track} ({} mm)",
                                    piece.length_mm
                                ),
                            );
                        }
                    }
                    None => error(report, format!("{prefix}: no track '{track}'")),
                }
            } else if device.at_mm.is_some() {
                error(report, format!("{prefix}: at_mm needs a track"));
            }
            if let Some(at) = &device.at {
                if let Err(problem) = self.point(at) {
                    error(report, format!("{prefix}: {problem}"));
                }
            }
            if let Some(role) = &device.role {
                if !DEVICE_ROLES.contains(&role.as_str()) {
                    error(
                        report,
                        format!("{prefix}: role '{role}' is not one of {DEVICE_ROLES:?}"),
                    );
                }
            }
            if let Some(direction) = &device.direction {
                if let Err(problem) = self.check_direction(direction) {
                    error(report, format!("{prefix}: {problem}"));
                }
            }
        }

        for station in &self.stations {
            for track in &station.tracks {
                match self.piece(&track.track) {
                    Some(piece) if track.usable_mm > piece.length_mm => error(
                        report,
                        format!(
                            "station {} track {}: usable_mm {} is longer than track {} ({} mm)",
                            station.name, track.name, track.usable_mm, piece.id, piece.length_mm
                        ),
                    ),
                    Some(_) => {}
                    None => error(
                        report,
                        format!(
                            "station {} track {}: no track '{}'",
                            station.name, track.name, track.track
                        ),
                    ),
                }
            }
        }

        // A device named by a building or a crossing must exist and be of the right type.
        let expect =
            |report: &mut Report, owner: &str, id: &str, kinds: &[&str]| match self.device(id) {
                Some(device) if kinds.contains(&device.kind.as_str()) => {}
                Some(device) => report.error(format!(
                    "{name}: {owner}: {id} is a {}, expected {}",
                    device.kind,
                    kinds.join(" or ")
                )),
                None => report.error(format!("{name}: {owner}: no device '{id}'")),
            };
        for building in &self.buildings {
            for light in &building.lights {
                expect(
                    report,
                    &format!("building {}", building.id),
                    light,
                    &["light"],
                );
            }
        }
        for crossing in &self.crossings {
            let owner = format!("crossing {}", crossing.id);
            if crossing.tracks.is_empty() {
                error(report, format!("{owner} crosses no track"));
            }
            for place in &crossing.tracks {
                match self.piece(&place.track) {
                    Some(piece) if place.at_mm > piece.length_mm => error(
                        report,
                        format!(
                            "{owner}: at_mm is beyond the end of track {} ({} mm)",
                            piece.id, piece.length_mm
                        ),
                    ),
                    Some(_) => {}
                    None => error(report, format!("{owner}: no track '{}'", place.track)),
                }
            }
            if let Some(lights) = &crossing.lights {
                expect(report, &owner, lights, &["light"]);
            }
            if let Some(barriers) = &crossing.barriers {
                expect(report, &owner, barriers, &["animation"]);
            }
            for sensor in crossing.close.iter().chain(&crossing.open) {
                expect(report, &owner, sensor, &["sensor"]);
            }
            if crossing.close.is_empty() {
                report.warning(format!("{name}: {owner} has no sensor that closes it"));
            }
        }

        if let Some(geometry) = &self.geometry {
            let warn = |report: &mut Report, kind: &str, id: &str| {
                report.warning(format!(
                    "{name}: geometry names {kind} '{id}', which the module does not have"
                ));
            };
            for id in geometry.ends.keys() {
                if self.end(id).is_none() {
                    warn(report, "end", id);
                }
            }
            for id in geometry.points.keys() {
                if !self.turnouts.iter().any(|turnout| &turnout.id == id) {
                    warn(report, "turnout", id);
                }
            }
            for id in geometry.track.keys() {
                if self.piece(id).is_none() {
                    warn(report, "track", id);
                }
            }
            for id in geometry.devices.keys() {
                if self.device(id).is_none() {
                    warn(report, "device", id);
                }
            }
            for id in geometry.buildings.keys() {
                if !self.buildings.iter().any(|building| &building.id == id) {
                    warn(report, "building", id);
                }
            }
        }
    }

    /// A running direction such as `A>B`, between two different ends of this module.
    fn check_direction(&self, direction: &str) -> Result<(), String> {
        match direction.split_once('>') {
            Some((from, to))
                if from != to && self.end(from).is_some() && self.end(to).is_some() =>
            {
                Ok(())
            }
            _ => Err(format!(
                "direction '{direction}' must be two ends of this module, such as A>B"
            )),
        }
    }
}

/// The address space a device lives in, for the clash check: per bus, and on LocoNet
/// sensors and outputs apart.
pub(crate) fn address_space(device: &Device) -> &'static str {
    match (device.bus.as_str(), device.kind.as_str()) {
        ("loconet", "sensor" | "occupancy" | "transponder" | "button") => "LocoNet sensor",
        ("loconet", _) => "LocoNet switch",
        ("dcc", _) => "DCC accessory",
        ("dccex", _) => "DCC-EX",
        ("board", _) => "board pin",
        _ => "unknown bus",
    }
}

/// `module.end` in a connection.
pub(crate) fn split_end(text: &str) -> Option<(&str, &str)> {
    text.rsplit_once('.')
}

/// A connection between two districts.
#[derive(Debug)]
pub struct Border {
    pub a: String,
    pub b: String,
    pub districts: (String, String),
}

impl Layout {
    /// The district (node) a module is in, if exactly one.
    pub fn district_of(&self, module_id: &str) -> Option<&str> {
        let mut found = self
            .file
            .districts
            .iter()
            .filter(|district| district.modules.iter().any(|m| m == module_id));
        let first = found.next()?;
        found.next().is_none().then_some(first.node.as_str())
    }

    /// Every connection between two districts.
    pub fn borders(&self) -> Vec<Border> {
        let mut borders = Vec::new();
        for connection in &self.file.connections {
            let (Some((ma, _)), Some((mb, _))) =
                (split_end(&connection.a), split_end(&connection.b))
            else {
                continue;
            };
            if let (Some(da), Some(db)) = (self.district_of(ma), self.district_of(mb)) {
                if da != db {
                    borders.push(Border {
                        a: connection.a.clone(),
                        b: connection.b.clone(),
                        districts: (da.to_owned(), db.to_owned()),
                    });
                }
            }
        }
        borders
    }

    /// Checks the layout and every module file in it.
    pub fn check(&self) -> Report {
        let mut report = Report::default();
        for problem in &self.load_errors {
            report.error(problem.clone());
        }
        if self.file.schema_version != SCHEMA_VERSION {
            report.error(format!(
                "layout: schema_version {} (expected {SCHEMA_VERSION})",
                self.file.schema_version
            ));
        }

        let mut listed = BTreeSet::new();
        for module in &self.file.modules {
            if !listed.insert(&module.module_id) {
                report.error(format!(
                    "layout: module {} is listed twice",
                    module.module_id
                ));
            }
        }
        for (id, (module, sum)) in &self.modules {
            if &module.module_id != id {
                report.error(format!(
                    "{id}.json: its module_id is '{}'; the file name and the id must match",
                    module.module_id
                ));
            }
            module.check(&mut report);
            let expected = self
                .file
                .modules
                .iter()
                .find(|m| &m.module_id == id)
                .and_then(|m| m.checksum.as_deref());
            if let Some(expected) = expected {
                let expected = expected.trim_end_matches('…').trim_end_matches("...");
                if !sum.starts_with(expected) {
                    report.warning(format!(
                        "{id}: the module file changed since the layout was made (checksum {sum}, the layout has {expected})"
                    ));
                }
            }
        }

        // Connections: ends that exist, are used once, fit and carry as many tracks.
        let mut ends_used: HashMap<&str, usize> = HashMap::new();
        for connection in &self.file.connections {
            let mut sides = Vec::new();
            for text in [&connection.a, &connection.b] {
                *ends_used.entry(text.as_str()).or_default() += 1;
                let Some((module_id, end_id)) = split_end(text) else {
                    report.error(format!("connection: '{text}' is not module.end"));
                    continue;
                };
                if !listed.contains(&module_id.to_owned()) {
                    report.error(format!(
                        "connection {text}: module {module_id} is not in the layout"
                    ));
                    continue;
                }
                let Some((module, _)) = self.modules.get(module_id) else {
                    continue; // already reported as missing
                };
                match module.end(end_id) {
                    Some(end) => sides.push(end),
                    None => report.error(format!(
                        "connection {text}: module {module_id} has no end {end_id}"
                    )),
                }
            }
            if let [a, b] = sides[..] {
                let name = format!("{} – {}", connection.a, connection.b);
                if a.profile != b.profile {
                    report.error(format!(
                        "connection {name}: end profiles '{}' and '{}' do not fit",
                        a.profile, b.profile
                    ));
                }
                if a.tracks.len() != b.tracks.len() {
                    report.error(format!(
                        "connection {name}: {} tracks meet {} tracks",
                        a.tracks.len(),
                        b.tracks.len()
                    ));
                }
            }
        }
        for (end, count) in &ends_used {
            if *count > 1 {
                report.error(format!("module end {end} is in {count} connections"));
            }
        }

        // Districts: every module in exactly one; addresses unique within a district.
        let mut nodes = BTreeSet::new();
        for district in &self.file.districts {
            if !nodes.insert(&district.node) {
                report.error(format!("node {} has two districts", district.node));
            }
            for module in &district.modules {
                if !listed.contains(module) {
                    report.error(format!(
                        "district {}: module {module} is not in the layout",
                        district.node
                    ));
                }
            }
        }
        for id in &listed {
            let count = self
                .file
                .districts
                .iter()
                .filter(|district| district.modules.contains(id))
                .count();
            match count {
                1 => {}
                0 => report.error(format!("module {id} is in no district")),
                _ => report.error(format!("module {id} is in {count} districts")),
            }
        }
        for district in &self.file.districts {
            let mut addresses: BTreeMap<(&str, u16), Vec<String>> = BTreeMap::new();
            for module_id in &district.modules {
                let Some((module, _)) = self.modules.get(module_id) else {
                    continue;
                };
                for device in &module.devices {
                    addresses
                        .entry((address_space(device), device.address))
                        .or_default()
                        .push(format!("{module_id}/{}", device.id));
                }
            }
            for ((space, address), devices) in addresses {
                if devices.len() > 1 {
                    report.error(format!(
                        "district {}: {space} address {address} is used by {}; re-address one, or move a module to another district",
                        district.node,
                        devices.join(" and ")
                    ));
                }
            }
        }

        // Borders: single or double track only, and a transfer block for each track.
        for border in self.borders() {
            self.check_border(&border, &mut report);
            // A level crossing never sits on a module at a district border: its sensors
            // and outputs would belong to the district on the other side of the handover.
            for side in [&border.a, &border.b] {
                let Some((module_id, _)) = split_end(side) else {
                    continue;
                };
                if let Some((module, _)) = self.modules.get(module_id) {
                    for crossing in &module.crossings {
                        report.error(format!(
                            "{module_id}: level crossing {} is on a module at the district border {} – {}; move it to a module inside a district",
                            crossing.name, border.districts.0, border.districts.1
                        ));
                    }
                }
            }
        }
        report
    }

    fn check_border(&self, border: &Border, report: &mut Report) {
        let name = format!(
            "border {} – {} ({} – {})",
            border.a, border.b, border.districts.0, border.districts.1
        );
        let side = |text: &str| {
            let (module_id, end_id) = split_end(text)?;
            let (module, _) = self.modules.get(module_id)?;
            Some((module, module.end(end_id)?))
        };
        let (Some((ma, ea)), Some((mb, eb))) = (side(&border.a), side(&border.b)) else {
            return;
        };
        if ea.tracks.len() > 2 || eb.tracks.len() > 2 {
            report.error(format!(
                "{name}: a district border carries only single or double track, here {} tracks",
                ea.tracks.len().max(eb.tracks.len())
            ));
            return;
        }
        let count = ea.tracks.len().min(eb.tracks.len());
        for i in 0..count {
            // The left track of one end meets the right track of the other.
            let track_a = &ea.tracks[i];
            let track_b = &eb.tracks[eb.tracks.len() - 1 - i];
            let piece_a = ma.piece_at(&ea.id, track_a);
            let piece_b = mb.piece_at(&eb.id, track_b);
            // Which way do trains run on this track: out of A's end, or into it?
            let towards_a_end = piece_a
                .and_then(|piece| piece.direction.as_deref())
                .and_then(|direction| direction.split_once('>'))
                .map(|(_, to)| to == ea.id);
            let transfer = |module: &ModuleFile, piece: Option<&TrackPiece>| {
                piece.is_some_and(|piece| {
                    module.blocks.iter().any(|block| {
                        block.track == piece.id && block.role.as_deref() == Some("transfer")
                    })
                })
            };
            let found = match towards_a_end {
                // Trains run from A into B: B, the receiving side, holds the transfer block.
                Some(true) => transfer(mb, piece_b),
                Some(false) => transfer(ma, piece_a),
                // Single track or no direction: either side may hold it.
                None => transfer(ma, piece_a) || transfer(mb, piece_b),
            };
            if !found {
                let towards = match towards_a_end {
                    Some(true) => format!(" (towards {})", border.districts.1),
                    Some(false) => format!(" (towards {})", border.districts.0),
                    None => String::new(),
                };
                report.warning(format!(
                    "{name}: track {track_a}{towards} has no transfer block on its receiving side; a train cannot be handed over there"
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> Layout {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/layout");
        load(&dir.join("bench-double-track.json"), &dir).unwrap()
    }

    /// The example meeting: double track, a station with a passing loop, two districts,
    /// a border with transfer blocks for both tracks.
    #[test]
    fn the_example_layout_is_clean() {
        let layout = example();
        let report = layout.check();
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert!(report.warnings.is_empty(), "{:#?}", report.warnings);
        let borders = layout.borders();
        assert_eq!(borders.len(), 1);
        assert_eq!(borders[0].districts, ("MacBook".into(), "Windows".into()));
    }

    #[test]
    fn finds_an_address_clash_in_a_district_but_not_across() {
        let mut layout = example();
        // Move the Windows border module into the MacBook district: its addresses now
        // share the bus with the MacBook modules.
        let windows = layout
            .file
            .districts
            .iter_mut()
            .find(|d| d.node == "Windows")
            .unwrap();
        let module = windows.modules.remove(0);
        layout
            .file
            .districts
            .iter_mut()
            .find(|d| d.node == "MacBook")
            .unwrap()
            .modules
            .push(module);
        let report = layout.check();
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("board pin address 17")),
            "{:#?}",
            report.errors
        );
    }

    #[test]
    fn a_loconet_sensor_and_switch_with_one_number_do_not_clash() {
        let layout = example();
        let (module, _) = &layout.modules["timo-adorf"];
        let occupancy = module
            .devices
            .iter()
            .find(|d| d.kind == "occupancy")
            .unwrap();
        let turnout = module.devices.iter().find(|d| d.kind == "turnout").unwrap();
        assert_ne!(address_space(occupancy), address_space(turnout));
    }

    #[test]
    fn finds_broken_track_graphs() {
        let layout = example();
        let (module, _) = &layout.modules["timo-adorf"];
        let mut broken = module.clone();
        // The siding no longer joins W2's diverging leg.
        let siding = broken
            .track
            .iter_mut()
            .find(|piece| piece.id == "s1")
            .unwrap();
        siding.to = "end".into();
        let mut report = Report::default();
        broken.check(&mut report);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("W2.diverging is not connected")),
            "{:#?}",
            report.errors
        );
    }

    #[test]
    fn a_border_without_transfer_blocks_warns_and_more_than_double_track_is_an_error() {
        let mut layout = example();
        for (module, _) in layout.modules.values_mut() {
            for block in &mut module.blocks {
                if block.role.as_deref() == Some("transfer") {
                    block.role = None;
                }
            }
        }
        let report = layout.check();
        assert_eq!(
            report
                .warnings
                .iter()
                .filter(|w| w.contains("no transfer block"))
                .count(),
            2,
            "{:#?}",
            report.warnings
        );

        let mut layout = example();
        for (module, _) in layout.modules.values_mut() {
            for end in &mut module.ends {
                end.tracks.push("3".into());
            }
        }
        let report = layout.check();
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("only single or double track")),
            "{:#?}",
            report.errors
        );
    }

    /// A level crossing's barriers must be an animation, its lights a light and its
    /// sensors sensors; a building's lights lights.
    #[test]
    fn checks_crossings_and_buildings() {
        let layout = example();
        let (module, _) = &layout.modules["timo-2e96-west"];
        assert_eq!(module.crossings[0].name, "Feldweg");
        let mut broken = module.clone();
        broken.crossings[0].barriers = Some("lc_lights".into());
        broken.crossings[0].close.push("missing".into());
        broken.buildings[0].lights.push("lc_close_1".into());
        let mut report = Report::default();
        broken.check(&mut report);
        for expected in [
            "lc_lights is a light, expected animation",
            "no device 'missing'",
            "lc_close_1 is a sensor, expected light",
        ] {
            assert!(
                report.errors.iter().any(|e| e.contains(expected)),
                "{expected}: {:#?}",
                report.errors
            );
        }
    }

    #[test]
    fn a_level_crossing_on_a_border_module_is_an_error() {
        let mut layout = example();
        let crossing = layout.modules["timo-2e96-west"].0.crossings[0].clone();
        layout
            .modules
            .get_mut("timo-2e96-border")
            .unwrap()
            .0
            .crossings
            .push(crossing);
        let report = layout.check();
        assert!(
            report.errors.iter().any(|e| e.contains("level crossing Feldweg is on a module at the district border")),
            "{:#?}",
            report.errors
        );
    }

    #[test]
    fn a_changed_module_file_warns() {
        let mut layout = example();
        layout.file.modules[0].checksum = Some("0123456789abcdef".into());
        let report = layout.check();
        assert!(report
            .warnings
            .iter()
            .any(|w| w.contains("changed since the layout was made")));
    }
}
