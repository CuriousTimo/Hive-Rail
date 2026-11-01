//! Module files of schema version 3: a module drawn from SpDrS60 tiles on a grid (the
//! module editor's format, `assets/spdrs60/`), read into the same `ModuleFile` the rest of
//! Hive-Rail uses. The tiles are the one source: the track pieces, turnouts, devices'
//! places along the track, blocks, stations, level crossings and the drawing geometry are
//! all derived here.
//!
//! - Track meets a tile edge only at the middle of an edge. A tile's paths join its ports
//!   (`assets/spdrs60/tiles.json`); the ports turn and mirror with the tile.
//! - Pieces run between points: a module end's track (`A.1`), a turnout leg (`3.trunk`) or
//!   a buffer stop (`end`). A double slip is two turnout halves (`4a`, `4b`) joined by a
//!   short piece, as in schema version 2.
//! - Lengths come from the module's real size: a cell is its share of the real length and
//!   depth; a straight tile is one cell long, a curve tile its 45 degree chord.
//! - A station block with several occupancy sections becomes one block per section, split
//!   halfway between the sections' sensors.

use crate::layout::{
    Block, Crossing, CrossingTrack, Device, End, EndPlace, Geometry, ModuleFile, Station,
    StationTrack, TrackPiece, Turnout,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

const TILES_JSON: &str = include_str!("../assets/spdrs60/tiles.json");

#[derive(Deserialize)]
struct TileSet {
    cell_mm: f64,
    tiles: BTreeMap<String, TileKind>,
}

#[derive(Deserialize)]
struct TileKind {
    cells: i32,
    paths: BTreeMap<String, Vec<String>>,
}

#[derive(Deserialize)]
struct ModuleV3 {
    module_id: String,
    friendly_name: String,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    scale: Option<String>,
    size_mm: [f64; 2],
    #[serde(default)]
    ends: Vec<EndV3>,
    #[serde(default)]
    tracks: Vec<TrackV3>,
    tiles: Vec<TileV3>,
    #[serde(default)]
    stations: Vec<StationV3>,
    #[serde(default)]
    passing_loops: Vec<StationV3>,
    #[serde(default)]
    blocks: Vec<BlockV3>,
    #[serde(default)]
    crossings: Vec<CrossingV3>,
    #[serde(default)]
    devices: Vec<DeviceV3>,
}

#[derive(Deserialize)]
struct EndV3 {
    id: String,
    side: String,
    profile: String,
}

#[derive(Deserialize)]
struct TrackV3 {
    name: String,
    #[serde(default)]
    direction: Option<String>,
}

#[derive(Deserialize, Clone)]
struct TileV3 {
    tile: String,
    x: i32,
    y: i32,
    #[serde(default)]
    turn: i32,
    #[serde(default)]
    mirror: bool,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    track: Option<String>,
    #[serde(default)]
    block: Option<String>,
    #[serde(default)]
    device: Option<String>,
    #[serde(default)]
    options: Vec<String>,
    #[serde(default)]
    text: Option<String>,
}

/// A module's tiles as the map draws them: the grid and each tile with what it shows, so
/// its lamps can follow the layout.
#[derive(Debug, Clone, Serialize)]
pub struct TileGrid {
    /// Cells across and down.
    pub cells: [i32; 2],
    pub size_mm: [f64; 2],
    pub tiles: Vec<PlacedTile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlacedTile {
    pub tile: String,
    pub x: i32,
    pub y: i32,
    pub turn: i32,
    pub mirror: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The tile's own name: a turnout's number, a signal's name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The track pieces it lies on (a turnout: every leg's piece).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pieces: Vec<String>,
    /// The block (occupancy section) it lies in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<String>,
    /// The turnouts it shows (a double slip: both halves).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub turnouts: Vec<String>,
    /// The signal it shows: its device.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
    /// The level crossing it shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crossing: Option<String>,
}

#[derive(Deserialize)]
struct StationV3 {
    name: String,
    #[serde(default)]
    code: Option<String>,
    tracks: Vec<StationTrackV3>,
}

#[derive(Deserialize)]
struct StationTrackV3 {
    name: String,
    track: String,
    #[serde(default)]
    platform: bool,
    #[serde(default)]
    passing: bool,
    #[serde(default)]
    usable_mm: Option<u32>,
}

#[derive(Deserialize)]
struct BlockV3 {
    id: String,
    #[serde(default = "outer")]
    detected_rail: String,
    #[serde(default)]
    sections: Vec<SectionV3>,
}

fn outer() -> String {
    "outer".into()
}

#[derive(Deserialize)]
struct SectionV3 {
    device: String,
    #[serde(default)]
    role: Option<String>,
    #[serde(default, rename = "for")]
    direction: Option<String>,
}

#[derive(Deserialize)]
struct CrossingV3 {
    id: String,
    name: String,
    device: String,
    #[serde(default)]
    close: Vec<String>,
    #[serde(default)]
    open: Vec<String>,
}

#[derive(Deserialize, Clone)]
struct DeviceV3 {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default = "loconet")]
    bus: String,
    #[serde(default)]
    address: Option<u16>,
    #[serde(default)]
    second_address: Option<u16>,
    #[serde(default)]
    barriers: Option<u16>,
    #[serde(default)]
    aspects: Option<serde_json::Value>,
}

fn loconet() -> String {
    "loconet".into()
}

/// A tile edge: a vertical edge left of cell (x, y) or a horizontal edge above it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
enum Edge {
    Vertical(i32, i32),
    Horizontal(i32, i32),
}

/// A tile's port on the grid: its cell and the side of the cell.
#[derive(Clone, Copy, Debug)]
struct Port {
    x: i32,
    y: i32,
    side: char,
}

impl Port {
    fn edge(self) -> Edge {
        match self.side {
            'W' => Edge::Vertical(self.x, self.y),
            'E' => Edge::Vertical(self.x + 1, self.y),
            'N' => Edge::Horizontal(self.x, self.y),
            _ => Edge::Horizontal(self.x, self.y + 1),
        }
    }

    /// The port's place in cells: the middle of its edge.
    fn at(self) -> (f64, f64) {
        let (x, y) = (f64::from(self.x), f64::from(self.y));
        match self.side {
            'W' => (x, y + 0.5),
            'E' => (x + 1.0, y + 0.5),
            'N' => (x + 0.5, y),
            _ => (x + 0.5, y + 1.0),
        }
    }
}

/// Where a port of a tile's path ("S0": the bottom edge of its first cell, as drawn) lies
/// on the grid, after mirroring and turning; the same as the module editor's portOnGrid.
fn port_on_grid(tile: &TileV3, cells: i32, port: &str) -> Port {
    let mut side = port.chars().next().unwrap_or('W');
    let i: i32 = port[1..].parse().unwrap_or(0);
    if tile.mirror {
        side = match side {
            'N' => 'S',
            'S' => 'N',
            other => other,
        };
    }
    let turn = tile.turn.rem_euclid(4);
    for _ in 0..turn {
        side = match side {
            'W' => 'N',
            'N' => 'E',
            'E' => 'S',
            _ => 'W',
        };
    }
    let (dx, dy) = [(i, 0), (0, i), (cells - 1 - i, 0), (0, cells - 1 - i)][turn as usize];
    Port {
        x: tile.x + dx,
        y: tile.y + dy,
        side,
    }
}

/// What meets at a tile edge: a plain tile's port (a tile with one path through it), or a
/// point where pieces end (a turnout leg, a buffer stop, a module end).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Thing {
    Plain { tile: usize, port: usize },
    Point(usize),
}

/// A point where pieces end, with its place on the drawing (cells).
struct Point {
    name: String,
    edge: Edge,
    /// Where the piece starts: a turnout's middle, the port of a module end.
    anchor: (f64, f64),
    port_at: (f64, f64),
}

/// The real size of one cell, across and along.
struct Scale {
    cell_x: f64,
    cell_y: f64,
    depth: f64,
}

impl Scale {
    fn mm(&self, (u, v): (f64, f64)) -> [f64; 2] {
        // Drawing coordinates: x from the left edge, y from the front (bottom) edge up.
        [u * self.cell_x, self.depth - v * self.cell_y]
    }

    fn length(&self, a: (f64, f64), b: (f64, f64)) -> f64 {
        ((b.0 - a.0) * self.cell_x).hypot((b.1 - a.1) * self.cell_y)
    }
}

/// A piece found by walking the tiles: its points, length, drawing, and the tiles on it
/// with how far along their middle lies and whether they face along the walk.
struct Walk {
    from: usize,
    to: usize,
    length: f64,
    line: Vec<(f64, f64)>,
    tiles: Vec<(usize, f64, bool)>,
}

/// A piece inside a double slip, between its halves: point a, point b, length, line.
type InnerPiece = (usize, usize, f64, Vec<(f64, f64)>);

/// Reads a module file of schema version 3 into a `ModuleFile`.
pub fn read(bytes: &[u8]) -> Result<ModuleFile, String> {
    let set: TileSet = serde_json::from_str(TILES_JSON).map_err(|e| e.to_string())?;
    let module: ModuleV3 = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let (w, h) = (
        (module.size_mm[0] / set.cell_mm).round().max(1.0) as i32,
        (module.size_mm[1] / set.cell_mm).round().max(1.0) as i32,
    );
    let scale = Scale {
        cell_x: module.size_mm[0] / f64::from(w),
        cell_y: module.size_mm[1] / f64::from(h),
        depth: module.size_mm[1],
    };
    let kind = |t: &TileV3| set.tiles.get(&t.tile);
    for t in &module.tiles {
        if kind(t).is_none() {
            return Err(format!("unknown tile '{}' at {}, {}", t.tile, t.x, t.y));
        }
        if t.tile == "crossing" {
            return Err(format!(
                "crossing at {}, {}: plain crossings are not supported yet",
                t.x, t.y
            ));
        }
    }

    // --- points: turnout legs, buffer stops, the double slips' halves ---
    let mut points: Vec<Point> = Vec::new();
    let mut things: HashMap<Edge, Vec<Thing>> = HashMap::new();
    let mut turnouts: Vec<Turnout> = Vec::new();
    let mut geometry_points: BTreeMap<String, [f64; 2]> = BTreeMap::new();
    // Pieces inside a double slip, between its halves.
    let mut inner: Vec<InnerPiece> = Vec::new();
    let mut extra_devices: Vec<Device> = Vec::new();
    let device_of = |id: &Option<String>| {
        id.as_ref()
            .and_then(|id| module.devices.iter().find(|d| &d.id == id))
    };
    for (index, t) in module.tiles.iter().enumerate() {
        let k = kind(t).expect("checked");
        let center_of = |cell: i32| {
            let p = port_on_grid(t, k.cells, &format!("W{cell}"));
            let q = port_on_grid(t, k.cells, &format!("E{cell}"));
            let (a, b) = (p.at(), q.at());
            ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
        };
        match t.tile.as_str() {
            "turnout-right" | "turnout-left" => {
                let id = t.id.clone().unwrap_or_else(|| format!("W{}_{}", t.x, t.y));
                let center = center_of(0);
                let straight = &k.paths["straight"];
                let diverging = &k.paths["diverging"];
                let trunk = straight
                    .iter()
                    .find(|p| diverging.contains(p))
                    .cloned()
                    .unwrap_or_default();
                let leg = |ports: &Vec<String>| {
                    ports
                        .iter()
                        .find(|p| **p != trunk)
                        .cloned()
                        .unwrap_or_default()
                };
                for (name, port) in [
                    ("trunk", trunk.clone()),
                    ("straight", leg(straight)),
                    ("diverging", leg(diverging)),
                ] {
                    add_point(
                        &mut points,
                        &mut things,
                        format!("{id}.{name}"),
                        port_on_grid(t, k.cells, &port),
                        center,
                    );
                }
                geometry_points.insert(id.clone(), scale.mm(center));
                turnouts.push(Turnout {
                    id,
                    device: t.device.clone(),
                });
            }
            "double-slip" => {
                // The halves: a on the first cell (straight W0, diverging S0), b on the
                // second (straight E1, diverging N1), their trunks joined inside the tile.
                let id =
                    t.id.clone()
                        .unwrap_or_else(|| format!("DKW{}_{}", t.x, t.y));
                let device = device_of(&t.device);
                let mut trunks = Vec::new();
                for (half, cell, straight, diverging) in
                    [("a", 0, "W0", "S0"), ("b", 1, "E1", "N1")]
                {
                    let name = format!("{id}{half}");
                    let center = center_of(cell);
                    add_point(
                        &mut points,
                        &mut things,
                        format!("{name}.straight"),
                        port_on_grid(t, k.cells, straight),
                        center,
                    );
                    add_point(
                        &mut points,
                        &mut things,
                        format!("{name}.diverging"),
                        port_on_grid(t, k.cells, diverging),
                        center,
                    );
                    points.push(Point {
                        name: format!("{name}.trunk"),
                        edge: Edge::Vertical(-1, -1),
                        anchor: center,
                        port_at: center,
                    });
                    trunks.push((points.len() - 1, center));
                    geometry_points.insert(name.clone(), scale.mm(center));
                    // Each half switches on its own address: a on the device's address, b
                    // on its second address.
                    let half_device = device.map(|d| {
                        let address = if half == "a" {
                            d.address
                        } else {
                            d.second_address.or(d.address.map(|a| a + 1))
                        };
                        extra_devices.push(Device {
                            id: format!("{}_{half}", d.id),
                            kind: "turnout".into(),
                            bus: d.bus.clone(),
                            address: address.unwrap_or(0),
                            block: None,
                            track: None,
                            at_mm: None,
                            at: None,
                            role: None,
                            direction: None,
                            aspects: None,
                            functions: None,
                        });
                        format!("{}_{half}", d.id)
                    });
                    turnouts.push(Turnout {
                        id: name,
                        device: half_device,
                    });
                }
                let ((a, ca), (b, cb)) = (trunks[0], trunks[1]);
                inner.push((a, b, scale.length(ca, cb), vec![ca, cb]));
            }
            "buffer-stop" => {
                let port = port_on_grid(t, k.cells, &k.paths["end"][0]);
                add_point(&mut points, &mut things, "end".into(), port, center_of(0));
            }
            _ => {
                for (path_ports, _) in k.paths.values().zip(0..) {
                    for (i, port) in path_ports.iter().enumerate() {
                        let p = port_on_grid(t, k.cells, port);
                        things.entry(p.edge()).or_default().push(Thing::Plain {
                            tile: index,
                            port: i,
                        });
                    }
                }
            }
        }
    }

    // --- ends: tracks leaving the grid ---
    let side_of = |edge: Edge| match edge {
        Edge::Vertical(0, _) => Some("left"),
        Edge::Vertical(x, _) if x == w => Some("right"),
        Edge::Horizontal(_, 0) => Some("top"),
        Edge::Horizontal(_, y) if y == h => Some("bottom"),
        _ => None,
    };
    let mut end_edges: BTreeMap<&str, Vec<(Edge, String)>> = BTreeMap::new();
    let mut open: Vec<String> = Vec::new();
    let edges: Vec<Edge> = things.keys().copied().collect();
    for edge in edges {
        let here = things[&edge].clone();
        if here.len() == 1 {
            if let Some(side) = side_of(edge) {
                let track = match here[0] {
                    Thing::Plain { tile, .. } => {
                        module.tiles[tile].track.clone().unwrap_or_default()
                    }
                    Thing::Point(p) => points[p].name.clone(),
                };
                end_edges.entry(side).or_default().push((edge, track));
            } else {
                let at = match edge {
                    Edge::Vertical(x, y) | Edge::Horizontal(x, y) => (x, y),
                };
                open.push(format!("{}, {}", at.0, at.1));
            }
        }
    }
    if !open.is_empty() {
        open.sort();
        return Err(format!(
            "open track ends (track that meets no other tile) at {}",
            open.join("; ")
        ));
    }
    let mut ends = Vec::new();
    let mut end_places = BTreeMap::new();
    for (side, mut found) in end_edges {
        let end = module.ends.iter().find(|e| e.side == side).ok_or_else(|| {
            format!("track leaves the grid at the {side} edge, but the module has no end there")
        })?;
        // Ordered as seen standing on the module looking out of the end, left to right.
        found.sort_by_key(|(edge, _)| match edge {
            Edge::Vertical(_, y) => *y,
            Edge::Horizontal(x, _) => *x,
        });
        if side == "left" || side == "bottom" {
            found.reverse();
        }
        let mut sum = (0.0, 0.0);
        for (edge, track) in &found {
            let at = edge_at(*edge);
            sum = (sum.0 + at.0, sum.1 + at.1);
            points.push(Point {
                name: format!("{}.{track}", end.id),
                edge: *edge,
                anchor: at,
                port_at: at,
            });
            things
                .entry(*edge)
                .or_default()
                .push(Thing::Point(points.len() - 1));
        }
        let n = found.len() as f64;
        end_places.insert(
            end.id.clone(),
            EndPlace {
                at: scale.mm((sum.0 / n, sum.1 / n)),
                facing: match side {
                    "left" => 180.0,
                    "right" => 0.0,
                    "top" => 90.0,
                    _ => 270.0,
                },
            },
        );
        ends.push(End {
            id: end.id.clone(),
            profile: end.profile.clone(),
            tracks: found.into_iter().map(|(_, t)| t).collect(),
        });
    }

    // A module with a single end (a terminus: every other track ends at a buffer stop)
    // still has a back side, so directions keep their meaning: the opposite end, without
    // tracks, at the middle of the opposite edge ("B>A": towards the buffer stops).
    if ends.len() == 1 {
        let only = &ends[0];
        let other = if only.id == "A" { "B" } else { "A" };
        let at = end_places[&only.id].at;
        let opposite = [module.size_mm[0] - at[0], module.size_mm[1] - at[1]];
        let facing = (end_places[&only.id].facing + 180.0) % 360.0;
        end_places.insert(
            other.to_owned(),
            EndPlace {
                at: opposite,
                facing,
            },
        );
        let back = End {
            id: other.to_owned(),
            profile: crate::layout::BACK_PROFILE.to_owned(),
            tracks: Vec::new(),
        };
        if other == "A" {
            ends.insert(0, back);
        } else {
            ends.push(back);
        }
    }

    // --- walking the pieces ---
    let other_end = |tile: usize, port: usize| -> (usize, Port, Port) {
        let t = &module.tiles[tile];
        let k = kind(t).expect("checked");
        let path = k.paths.values().next().expect("a plain tile has a path");
        let (a, b) = (
            port_on_grid(t, k.cells, &path[port]),
            port_on_grid(t, k.cells, &path[1 - port]),
        );
        (1 - port, a, b)
    };
    let mut walks: Vec<Walk> = Vec::new();
    let mut done: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    for start in 0..points.len() {
        if matches!(points[start].edge, Edge::Vertical(-1, -1)) {
            continue; // a double slip's inner trunk: joined below
        }
        let mut edge = points[start].edge;
        let mut came = Thing::Point(start);
        let mut line = vec![points[start].anchor, points[start].port_at];
        let mut length = scale.length(points[start].anchor, points[start].port_at);
        let mut tiles = Vec::new();
        let end = loop {
            let next = things[&edge].iter().copied().find(|thing| *thing != came);
            match next {
                Some(Thing::Point(p)) => {
                    length += scale.length(points[p].port_at, points[p].anchor);
                    line.push(points[p].anchor);
                    break p;
                }
                Some(Thing::Plain { tile, port }) => {
                    let (out, a, b) = other_end(tile, port);
                    let step = scale.length(a.at(), b.at());
                    // Facing along the walk: the tile's path runs from its first port to
                    // its second, as drawn (a signal faces that way).
                    tiles.push((tile, length + step / 2.0, out == 1));
                    length += step;
                    line.push(b.at());
                    came = Thing::Plain { tile, port: out };
                    edge = b.edge();
                }
                None => return Err(format!("track at {:?} leads nowhere", edge)),
            }
        };
        let key = (start.min(end), start.max(end));
        if done.insert(key) {
            walks.push(Walk {
                from: start,
                to: end,
                length,
                line,
                tiles,
            });
        }
    }
    for (a, b, length, line) in inner {
        walks.push(Walk {
            from: a,
            to: b,
            length,
            line,
            tiles: Vec::new(),
        });
    }
    // A piece between a turnout and a module end or buffer stop runs from the turnout, as
    // in schema version 2 (so a train set on a platform track heading back leaves it).
    let end_ids_v3: Vec<&str> = module.ends.iter().map(|e| e.id.as_str()).collect();
    let is_turnout = |p: usize| {
        ["trunk", "straight", "diverging"]
            .iter()
            .any(|leg| points[p].name.ends_with(&format!(".{leg}")))
    };
    let is_end = |p: usize| {
        points[p].name == "end"
            || end_ids_v3
                .iter()
                .any(|id| points[p].name.starts_with(&format!("{id}.")))
    };
    for walk in &mut walks {
        if is_end(walk.from) && is_turnout(walk.to) {
            std::mem::swap(&mut walk.from, &mut walk.to);
            walk.line.reverse();
            for tile in &mut walk.tiles {
                *tile = (tile.0, walk.length - tile.1, !tile.2);
            }
            walk.tiles.reverse();
        }
    }

    // --- pieces: named after their tiles' track, else after their points ---
    let directions: HashMap<&str, &str> = module
        .tracks
        .iter()
        .filter_map(|t| Some((t.name.as_str(), t.direction.as_deref()?)))
        .collect();
    let mut piece_ids: Vec<String> = Vec::new();
    let mut track = Vec::new();
    let mut geometry_track = BTreeMap::new();
    let point_label = |p: usize| points[p].name.split('.').next().unwrap_or("").to_string();
    for walk in &walks {
        let named = walk
            .tiles
            .iter()
            .find_map(|(tile, _, _)| module.tiles[*tile].track.clone());
        let mut id =
            named.unwrap_or_else(|| format!("{}-{}", point_label(walk.from), point_label(walk.to)));
        if piece_ids.contains(&id) {
            let base = id.clone();
            id = (2..)
                .map(|n| format!("{base}_{n}"))
                .find(|c| !piece_ids.contains(c))
                .expect("a free name");
        }
        piece_ids.push(id.clone());
        track.push(TrackPiece {
            id: id.clone(),
            from: points[walk.from].name.clone(),
            to: points[walk.to].name.clone(),
            length_mm: walk.length.round().max(1.0) as u32,
            direction: directions.get(id.as_str()).map(|d| (*d).to_string()),
        });
        geometry_track.insert(
            id,
            walk.line.iter().map(|p| scale.mm(*p)).collect::<Vec<_>>(),
        );
    }

    // Where each tile lies on the track: its piece, how far along, and whether it faces
    // the piece's direction.
    let mut on_track: HashMap<usize, (String, f64, bool)> = HashMap::new();
    for (walk, id) in walks.iter().zip(&piece_ids) {
        for (tile, along, forward) in &walk.tiles {
            on_track.insert(*tile, (id.clone(), *along, *forward));
        }
    }
    // A direction along a piece as the module names it: "A>B" when the piece runs from
    // the first end towards the second.
    let end_ids: Vec<&String> = ends.iter().map(|e| &e.id).collect();
    let direction_of = |piece: &str, forward: bool| -> Option<String> {
        let (a, b) = (end_ids.first()?, end_ids.get(1)?);
        let (pa, pb) = (end_places.get(*a)?.at, end_places.get(*b)?.at);
        let line = geometry_track.get(piece)?;
        let (first, last): (&[f64; 2], &[f64; 2]) = (line.first()?, line.last()?);
        let along = (last[0] - first[0]) * (pb[0] - pa[0]) + (last[1] - first[1]) * (pb[1] - pa[1]);
        let a_to_b = (along >= 0.0) == forward;
        Some(if a_to_b {
            format!("{a}>{b}")
        } else {
            format!("{b}>{a}")
        })
    };

    // --- devices, placed by their tiles ---
    let mut devices = Vec::new();
    for d in &module.devices {
        if d.kind == "crossing" {
            continue; // as a light and an animation, with the level crossings below
        }
        let tile = module
            .tiles
            .iter()
            .enumerate()
            .find(|(_, t)| t.device.as_ref() == Some(&d.id) && t.tile != "double-slip");
        if module
            .tiles
            .iter()
            .any(|t| t.device.as_ref() == Some(&d.id) && t.tile == "double-slip")
        {
            continue; // its halves are in extra_devices
        }
        let place = tile.and_then(|(index, _)| on_track.get(&index));
        let signal = d.kind == "signal";
        devices.push(Device {
            id: d.id.clone(),
            kind: d.kind.clone(),
            bus: d.bus.clone(),
            address: d.address.unwrap_or(0),
            block: None,
            track: place
                .filter(|_| signal || d.kind == "sensor")
                .map(|(p, _, _)| p.clone()),
            at_mm: place
                .filter(|_| signal || d.kind == "sensor")
                .map(|(_, along, _)| along.round().max(0.0) as u32),
            at: None,
            role: None,
            direction: place
                .filter(|_| signal)
                .and_then(|(p, _, forward)| direction_of(p, *forward)),
            aspects: if signal {
                Some(signal_colours(d.aspects.as_ref()))
            } else {
                d.aspects.clone()
            },
            functions: None,
        });
    }
    devices.extend(extra_devices);

    // --- blocks: one per occupancy section ---
    let length_of = |piece: &str| {
        track
            .iter()
            .find(|p| p.id == piece)
            .map_or(0.0, |p| f64::from(p.length_mm))
    };
    let mut blocks = Vec::new();
    for block in &module.blocks {
        // Each section's sensor tile: its piece and place.
        let mut sections: Vec<(&SectionV3, String, f64)> = block
            .sections
            .iter()
            .filter_map(|s| {
                let (index, _) = module
                    .tiles
                    .iter()
                    .enumerate()
                    .find(|(_, t)| t.device.as_ref() == Some(&s.device))?;
                let (piece, along, _) = on_track.get(&index)?;
                Some((s, piece.clone(), *along))
            })
            .collect();
        sections.sort_by(|a, b| a.1.cmp(&b.1).then(a.2.total_cmp(&b.2)));
        let several = sections.len() > 1;
        for (i, (section, piece, along)) in sections.iter().enumerate() {
            // The block's tiles on this piece give its extent; sections split it halfway
            // between their sensors.
            let span: Vec<f64> = on_track
                .iter()
                .filter(|(tile, (p, _, _))| {
                    p == piece && module.tiles[**tile].block.as_ref() == Some(&block.id)
                })
                .map(|(_, (_, along, _))| *along)
                .collect();
            let half = scale.cell_x / 2.0;
            let (mut start, mut end) = (
                span.iter().copied().fold(*along, f64::min) - half,
                span.iter().copied().fold(*along, f64::max) + half,
            );
            let same_piece: Vec<f64> = sections
                .iter()
                .filter(|(_, p, _)| p == piece)
                .map(|(_, _, a)| *a)
                .collect();
            if let Some(prev) = same_piece
                .iter()
                .copied()
                .filter(|a| a < along)
                .fold(None, |m: Option<f64>, a| Some(m.map_or(a, |m| m.max(a))))
            {
                start = start.max((prev + along) / 2.0);
            }
            if let Some(next) = same_piece
                .iter()
                .copied()
                .filter(|a| a > along)
                .fold(None, |m: Option<f64>, a| Some(m.map_or(a, |m| m.min(a))))
            {
                end = end.min((next + along) / 2.0);
            }
            let total = length_of(piece);
            let (start, end) = (start.clamp(0.0, total), end.clamp(0.0, total));
            blocks.push(Block {
                id: if several {
                    format!("{}_{}", block.id, i + 1)
                } else {
                    block.id.clone()
                },
                track: piece.clone(),
                cuts_mm: [
                    start.round() as u32,
                    end.round().max(start.round() + 1.0) as u32,
                ],
                detected_rail: block.detected_rail.clone(),
                occupancy: Some(section.device.clone()),
                role: section
                    .role
                    .clone()
                    .filter(|r| ["edge", "transfer", "entry"].contains(&r.as_str())),
                direction: section.direction.clone(),
            });
        }
    }

    // --- stations and passing loops: their tracks by name ---
    let station = |s: &StationV3| -> Result<Station, String> {
        Ok(Station {
            name: s.name.clone(),
            code: s.code.clone(),
            tracks: s
                .tracks
                .iter()
                .map(|t| {
                    let length = length_of(&t.track);
                    if length == 0.0 {
                        return Err(format!(
                            "station {} track {}: no track '{}'",
                            s.name, t.name, t.track
                        ));
                    }
                    Ok(StationTrack {
                        name: t.name.clone(),
                        track: t.track.clone(),
                        usable_mm: t
                            .usable_mm
                            .unwrap_or((length - 2.0 * scale.cell_x).max(scale.cell_x) as u32),
                        platform: t.platform,
                        passing: t.passing,
                    })
                })
                .collect::<Result<_, _>>()?,
        })
    };
    let stations = module
        .stations
        .iter()
        .map(station)
        .collect::<Result<Vec<_>, _>>()?;
    let passing_loops = module
        .passing_loops
        .iter()
        .map(station)
        .collect::<Result<Vec<_>, _>>()?;

    // --- level crossings: lights and barriers from the crossing's device ---
    let mut crossings = Vec::new();
    for c in &module.crossings {
        let device = module.devices.iter().find(|d| d.id == c.device);
        let tracks = module
            .tiles
            .iter()
            .enumerate()
            .filter(|(_, t)| t.tile == "level-crossing" && t.device.as_ref() == Some(&c.device))
            .filter_map(|(index, _)| on_track.get(&index))
            .map(|(piece, along, _)| CrossingTrack {
                track: piece.clone(),
                at_mm: along.round() as u32,
            })
            .collect();
        let (mut lights, mut barriers) = (None, None);
        if let Some(d) = device {
            if let Some(address) = d.address {
                devices.push(device_named(
                    &format!("{}_lights", d.id),
                    "light",
                    &d.bus,
                    address,
                ));
                lights = Some(format!("{}_lights", d.id));
            }
            if let Some(address) = d.barriers {
                devices.push(device_named(
                    &format!("{}_barriers", d.id),
                    "animation",
                    &d.bus,
                    address,
                ));
                barriers = Some(format!("{}_barriers", d.id));
            }
        }
        crossings.push(Crossing {
            id: c.id.clone(),
            name: c.name.clone(),
            tracks,
            lights,
            barriers,
            close: c.close.clone(),
            open: c.open.clone(),
        });
    }

    // --- the drawing: the module's outline, ends, turnouts, track and platforms ---
    let (length, depth) = (module.size_mm[0], module.size_mm[1]);
    let mut platforms = BTreeMap::new();
    for (side, top) in [("platform-top", true), ("platform-bottom", false)] {
        let mut cells: Vec<(i32, i32)> = module
            .tiles
            .iter()
            .filter(|t| t.options.iter().any(|o| o == side) && t.turn.rem_euclid(2) == 0)
            .map(|t| (t.y, t.x))
            .collect();
        cells.sort();
        let mut runs: Vec<(i32, i32, i32)> = Vec::new();
        for (y, x) in cells {
            match runs.last_mut() {
                Some((ry, _, x1)) if *ry == y && *x1 + 1 == x => *x1 = x,
                _ => runs.push((y, x, x)),
            }
        }
        for (y, x0, x1) in runs {
            // A tile turned round has its top band below the track.
            let (v0, v1) = if top {
                (f64::from(y), f64::from(y) + 0.33)
            } else {
                (f64::from(y) + 0.67, f64::from(y) + 1.0)
            };
            let (u0, u1) = (f64::from(x0), f64::from(x1) + 1.0);
            platforms.insert(
                format!("platform {} {y} {x0}", if top { "top" } else { "bottom" }),
                vec![
                    scale.mm((u0, v1)),
                    scale.mm((u1, v1)),
                    scale.mm((u1, v0)),
                    scale.mm((u0, v0)),
                ],
            );
        }
    }
    // --- the tiles for the map, each with what it shows ---
    let mut placed = Vec::new();
    for (index, t) in module.tiles.iter().enumerate() {
        let mut pieces = Vec::new();
        let mut turnout_ids = Vec::new();
        let mut block = None;
        match t.tile.as_str() {
            "turnout-right" | "turnout-left" | "double-slip" => {
                let id = t.id.clone().unwrap_or_default();
                let halves: Vec<String> = if t.tile == "double-slip" {
                    vec![format!("{id}a"), format!("{id}b")]
                } else {
                    vec![id]
                };
                for piece in &track {
                    let touches =
                        |point: &str| halves.iter().any(|h| point.starts_with(&format!("{h}.")));
                    if touches(&piece.from) || touches(&piece.to) {
                        pieces.push(piece.id.clone());
                    }
                }
                turnout_ids = halves;
            }
            // A buffer stop shows the track it ends.
            "buffer-stop" => pieces.extend(t.track.clone()),
            _ => {
                if let Some((piece, along, _)) = on_track.get(&index) {
                    pieces.push(piece.clone());
                    block = blocks
                        .iter()
                        .find(|b| {
                            &b.track == piece
                                && f64::from(b.cuts_mm[0]) <= *along
                                && *along <= f64::from(b.cuts_mm[1])
                        })
                        .map(|b| b.id.clone());
                }
            }
        }
        let crossing = (t.tile == "level-crossing")
            .then(|| {
                module
                    .crossings
                    .iter()
                    .find(|c| Some(&c.device) == t.device.as_ref())
                    .map(|c| c.id.clone())
            })
            .flatten();
        placed.push(PlacedTile {
            tile: t.tile.clone(),
            x: t.x,
            y: t.y,
            turn: t.turn.rem_euclid(4),
            mirror: t.mirror,
            options: t.options.clone(),
            text: t.text.clone(),
            name: t.id.clone(),
            pieces,
            block,
            turnouts: turnout_ids,
            signal: t
                .tile
                .starts_with("signal-")
                .then(|| t.device.clone())
                .flatten(),
            crossing,
        });
    }
    let tile_grid = TileGrid {
        cells: [w, h],
        size_mm: module.size_mm,
        tiles: placed,
    };

    let geometry = Geometry {
        outline: vec![[0.0, 0.0], [length, 0.0], [length, depth], [0.0, depth]],
        ends: end_places,
        points: geometry_points,
        track: geometry_track,
        platforms,
        devices: BTreeMap::new(),
        buildings: BTreeMap::new(),
        roads: BTreeMap::new(),
    };

    Ok(ModuleFile {
        // Read into the form of schema version 2, which the checks and the rest know.
        schema_version: 2,
        module_id: module.module_id,
        friendly_name: module.friendly_name,
        owner: module.owner,
        scale: module.scale,
        length_mm: length.round() as u32,
        ends,
        track,
        turnouts,
        stations,
        passing_loops,
        blocks,
        devices,
        buildings: Vec::new(),
        crossings,
        geometry: Some(geometry),
        tiles: Some(tile_grid),
    })
}

/// Adds a point where pieces end, at a tile port.
fn add_point(
    points: &mut Vec<Point>,
    things: &mut HashMap<Edge, Vec<Thing>>,
    name: String,
    port: Port,
    anchor: (f64, f64),
) {
    points.push(Point {
        name,
        edge: port.edge(),
        anchor,
        port_at: port.at(),
    });
    things
        .entry(port.edge())
        .or_default()
        .push(Thing::Point(points.len() - 1));
}

/// The middle of a tile edge, in cells.
fn edge_at(edge: Edge) -> (f64, f64) {
    match edge {
        Edge::Vertical(x, y) => (f64::from(x), f64::from(y) + 0.5),
        Edge::Horizontal(x, y) => (f64::from(x) + 0.5, f64::from(y)),
    }
}

fn device_named(id: &str, kind: &str, bus: &str, address: u16) -> Device {
    Device {
        id: id.into(),
        kind: kind.into(),
        bus: bus.into(),
        address,
        block: None,
        track: None,
        at_mm: None,
        at: None,
        role: None,
        direction: None,
        aspects: None,
        functions: None,
    }
}

/// A signal's aspects as the simulator shows them: red and green, and yellow when the
/// signal can announce a stop (a distant on its mast, or Ks 2).
fn signal_colours(aspects: Option<&serde_json::Value>) -> serde_json::Value {
    let names: Vec<String> = aspects
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let announces = names
        .iter()
        .any(|a| a.starts_with("vr") || a == "ks2" || a == "yellow");
    if announces {
        serde_json::json!(["red", "yellow", "green"])
    } else {
        serde_json::json!(["red", "green"])
    }
}

/// Whether a module file is of schema version 3.
pub fn is_v3(bytes: &[u8]) -> bool {
    #[derive(Deserialize)]
    struct Version {
        schema_version: u32,
    }
    serde_json::from_slice::<Version>(bytes).is_ok_and(|v| v.schema_version == 3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn module(name: &str) -> ModuleFile {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples/layout")
            .join(format!("{name}.json"));
        read(&std::fs::read(path).unwrap()).unwrap()
    }

    /// Adorf from its tiles: the pieces of version 2 between the same points, the double
    /// slip as two halves, the platform tracks with their three sections each.
    #[test]
    fn adorf_reads_from_its_tiles() {
        let m = module("timo-adorf");
        let piece = |id: &str| {
            m.track.iter().find(|p| p.id == id).unwrap_or_else(|| {
                panic!(
                    "no piece {id}: {:?}",
                    m.track
                        .iter()
                        .map(|p| (&p.id, &p.from, &p.to))
                        .collect::<Vec<_>>()
                )
            })
        };
        let ends = |id: &str| {
            let p = piece(id);
            let mut e = [p.from.clone(), p.to.clone()];
            e.sort();
            e
        };
        // A terminus: every platform track ends at a buffer stop.
        assert_eq!(ends("1"), ["3.straight", "end"]);
        assert_eq!(ends("2"), ["4a.straight", "end"]);
        assert_eq!(ends("3"), ["4a.diverging", "end"]);
        assert_eq!(ends("in"), ["1.straight", "B.in"]);
        assert_eq!(ends("out"), ["2.trunk", "B.out"]);
        assert!(m.turnouts.iter().any(|t| t.id == "4a") && m.turnouts.iter().any(|t| t.id == "4b"));
        // Track 1 runs the module's length up to turnout 3: about 26 cells of 50 mm.
        assert!(
            (1250..=1350).contains(&piece("1").length_mm),
            "{}",
            piece("1").length_mm
        );
        // Three sections on each platform track, each its own block.
        let on_1: Vec<_> = m.blocks.iter().filter(|b| b.track == "1").collect();
        assert_eq!(
            on_1.len(),
            3,
            "{:?}",
            m.blocks
                .iter()
                .map(|b| (&b.id, &b.track))
                .collect::<Vec<_>>()
        );
        // The exit signals face east (towards B), the entry signal west.
        let signal = |id: &str| m.devices.iter().find(|d| d.id == id).unwrap();
        assert_eq!(signal("signal_n1").direction.as_deref(), Some("A>B"));
        assert_eq!(signal("signal_f").direction.as_deref(), Some("B>A"));
        assert_eq!(
            m.ends.iter().find(|e| e.id == "B").unwrap().tracks,
            ["in", "out"]
        );
    }

    /// Every converted example module reads without an open end.
    #[test]
    fn every_example_module_reads() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/layout");
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let bytes = std::fs::read(&path).unwrap();
            if is_v3(&bytes) {
                read(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            }
        }
    }
}
