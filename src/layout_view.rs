//! What the web view draws: the modules of a layout placed in the world by joining their
//! ends, with their drawings (outline, track, blocks, points, platforms, buildings, roads)
//! in world coordinates, and the live state on top (occupied blocks, turnout positions,
//! closed level crossings, trains).
//!
//! World coordinates are millimetres with y pointing up (towards the back of the first
//! module); the page flips y for the screen. A module is placed so that each connected end
//! meets its neighbour's end, facing it: a module turned round, joined at its other end,
//! comes out turned.

use crate::layout::{Layout, ModuleFile};
use crate::layout_sim::LayoutSim;
use crate::topology::{Heading, PieceId, Topology};
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};

pub type Point = [f64; 2];

/// Where a module lies in the world: rotated by `angle` degrees, then moved by `offset`.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Placement {
    pub angle: f64,
    pub offset: Point,
}

impl Placement {
    fn apply(&self, [x, y]: Point) -> Point {
        let (sin, cos) = self.angle.to_radians().sin_cos();
        [
            x * cos - y * sin + self.offset[0],
            x * sin + y * cos + self.offset[1],
        ]
    }
}

#[derive(Debug, Serialize)]
pub struct LayoutView {
    pub meeting: String,
    pub districts: Vec<String>,
    pub modules: Vec<ModuleView>,
    pub pieces: Vec<PieceView>,
    pub blocks: Vec<BlockView>,
    pub turnouts: Vec<TurnoutView>,
    pub crossings: Vec<CrossingView>,
    pub buildings: Vec<BuildingView>,
    pub platforms: Vec<Vec<Point>>,
    pub stations: Vec<LabelView>,
    /// The drawing's extent: min x, min y, max x, max y.
    pub bounds: [f64; 4],
}

#[derive(Debug, Serialize)]
pub struct ModuleView {
    pub id: String,
    pub name: String,
    pub owner: Option<String>,
    pub district: String,
    pub placement: Placement,
    pub outline: Vec<Point>,
    /// Where the module's name is written.
    pub label: Point,
}

#[derive(Debug, Serialize)]
pub struct PieceView {
    pub id: String,
    pub district: String,
    pub points: Vec<Point>,
}

#[derive(Debug, Serialize)]
pub struct BlockView {
    pub id: String,
    pub district: String,
    pub role: Option<String>,
    pub points: Vec<Point>,
}

#[derive(Debug, Serialize)]
pub struct TurnoutView {
    pub id: String,
    pub name: String,
    pub at: Point,
}

#[derive(Debug, Serialize)]
pub struct CrossingView {
    pub id: String,
    pub name: String,
    pub road: Vec<Point>,
    /// Where the road meets each track.
    pub at: Vec<Point>,
}

#[derive(Debug, Serialize)]
pub struct BuildingView {
    pub id: String,
    pub name: String,
    pub outline: Vec<Point>,
    pub lights: usize,
}

#[derive(Debug, Serialize)]
pub struct LabelView {
    pub text: String,
    pub at: Point,
    /// The module it belongs to.
    pub module: String,
}

/// The live state, sent to the page several times a second.
#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub time_s: f64,
    pub occupied: Vec<String>,
    /// Turnout positions by `module/turnout`: true is straight.
    pub turnouts: BTreeMap<String, bool>,
    pub crossings_closed: Vec<String>,
    pub trains: Vec<TrainView>,
}

#[derive(Debug, Serialize)]
pub struct TrainView {
    pub name: String,
    pub address: u16,
    /// The train along the track, head first.
    pub points: Vec<Point>,
    pub district: String,
    pub state: String,
}

/// Places every module: the first one at the origin, the others by joining ends.
pub fn place_modules(layout: &Layout) -> BTreeMap<String, Placement> {
    let mut placed: BTreeMap<String, Placement> = BTreeMap::new();
    let Some(first) = layout.file.modules.first() else {
        return placed;
    };
    placed.insert(
        first.module_id.clone(),
        Placement {
            angle: 0.0,
            offset: [0.0, 0.0],
        },
    );
    let mut queue = VecDeque::from([first.module_id.clone()]);
    while let Some(current) = queue.pop_front() {
        for connection in &layout.file.connections {
            for (here, there) in [
                (&connection.a, &connection.b),
                (&connection.b, &connection.a),
            ] {
                let (Some((m_here, e_here)), Some((m_there, e_there))) =
                    (here.rsplit_once('.'), there.rsplit_once('.'))
                else {
                    continue;
                };
                if m_here != current || placed.contains_key(m_there) {
                    continue;
                }
                let (Some(end_here), Some(end_there)) = (
                    end_place(layout, m_here, e_here),
                    end_place(layout, m_there, e_there),
                ) else {
                    continue;
                };
                let placement = placed[m_here];
                let world_at = placement.apply(end_here.0);
                let world_facing = end_here.1 + placement.angle;
                // The neighbour's end faces back at this one.
                let angle = (world_facing + 180.0 - end_there.1).rem_euclid(360.0);
                let rotated = Placement {
                    angle,
                    offset: [0.0, 0.0],
                }
                .apply(end_there.0);
                let offset = [world_at[0] - rotated[0], world_at[1] - rotated[1]];
                placed.insert(m_there.to_owned(), Placement { angle, offset });
                queue.push_back(m_there.to_owned());
            }
        }
    }
    // Modules joined to nothing placed so far: in a row below.
    let mut spare_x = 0.0;
    for module in &layout.file.modules {
        if !placed.contains_key(&module.module_id) {
            placed.insert(
                module.module_id.clone(),
                Placement {
                    angle: 0.0,
                    offset: [spare_x, -1500.0],
                },
            );
            spare_x += layout
                .modules
                .get(&module.module_id)
                .map_or(1000.0, |(m, _)| f64::from(m.length_mm) + 200.0);
        }
    }
    placed
}

/// A module end's place and facing from its geometry.
fn end_place(layout: &Layout, module: &str, end: &str) -> Option<(Point, f64)> {
    let (module, _) = layout.modules.get(module)?;
    let place = module.geometry.as_ref()?.ends.get(end)?;
    Some((place.at, place.facing))
}

/// A piece's drawing in module coordinates, from `from` to `to`: its polyline in the
/// geometry, or a straight line between its points when it has none.
fn piece_polyline(module: &ModuleFile, piece: &str) -> Option<Vec<Point>> {
    let geometry = module.geometry.as_ref()?;
    if let Some(line) = geometry.track.get(piece) {
        return Some(line.clone());
    }
    let track = module.track.iter().find(|t| t.id == piece)?;
    let at = |point: &str| -> Option<Point> {
        let (left, _) = point.split_once('.')?;
        geometry
            .ends
            .get(left)
            .map(|end| end.at)
            .or_else(|| geometry.points.get(left).copied())
    };
    Some(vec![at(&track.from)?, at(&track.to)?])
}

fn polyline_length(points: &[Point]) -> f64 {
    points
        .windows(2)
        .map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt())
        .sum()
}

/// The part of a polyline between two fractions of its length (0 to 1).
fn sub_polyline(points: &[Point], from: f64, to: f64) -> Vec<Point> {
    let total = polyline_length(points);
    if total == 0.0 || points.len() < 2 {
        return points.to_vec();
    }
    let (start, stop) = (from.clamp(0.0, 1.0) * total, to.clamp(0.0, 1.0) * total);
    let mut out = Vec::new();
    let mut walked = 0.0;
    for w in points.windows(2) {
        let length = ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt();
        let lerp = |d: f64| {
            let t = if length == 0.0 {
                0.0
            } else {
                (d - walked) / length
            };
            [
                w[0][0] + (w[1][0] - w[0][0]) * t,
                w[0][1] + (w[1][1] - w[0][1]) * t,
            ]
        };
        if walked + length >= start && walked <= stop {
            if out.is_empty() {
                out.push(lerp(start.max(walked)));
            }
            out.push(lerp(stop.min(walked + length)));
        }
        walked += length;
    }
    out
}

fn centroid(points: &[Point]) -> Point {
    let n = points.len().max(1) as f64;
    let sum = points
        .iter()
        .fold([0.0, 0.0], |s, p| [s[0] + p[0], s[1] + p[1]]);
    [sum[0] / n, sum[1] / n]
}

pub struct ViewBuilder {
    placements: BTreeMap<String, Placement>,
    /// Each piece's world polyline and its length in mm.
    pieces: BTreeMap<PieceId, (Vec<Point>, f64)>,
}

impl ViewBuilder {
    pub fn new(layout: &Layout, topology: &Topology) -> Self {
        let placements = place_modules(layout);
        let mut pieces = BTreeMap::new();
        for piece in topology.pieces() {
            let Some((module, _)) = layout.modules.get(&piece.id.module) else {
                continue;
            };
            let (Some(line), Some(placement)) = (
                piece_polyline(module, &piece.id.piece),
                placements.get(&piece.id.module),
            ) else {
                continue;
            };
            let world: Vec<Point> = line.iter().map(|p| placement.apply(*p)).collect();
            pieces.insert(piece.id.clone(), (world, f64::from(piece.length_mm)));
        }
        Self { placements, pieces }
    }

    /// A stretch of a piece, in mm from its `from` point, in world coordinates.
    fn span(&self, piece: &PieceId, a_mm: f64, b_mm: f64) -> Vec<Point> {
        match self.pieces.get(piece) {
            Some((line, length)) if *length > 0.0 => {
                sub_polyline(line, a_mm / length, b_mm / length)
            }
            _ => Vec::new(),
        }
    }

    pub fn layout(&self, layout: &Layout, topology: &Topology) -> LayoutView {
        let mut view = LayoutView {
            meeting: layout.file.meeting.clone(),
            districts: layout
                .file
                .districts
                .iter()
                .map(|d| d.node.clone())
                .collect(),
            modules: Vec::new(),
            pieces: Vec::new(),
            blocks: Vec::new(),
            turnouts: Vec::new(),
            crossings: Vec::new(),
            buildings: Vec::new(),
            platforms: Vec::new(),
            stations: Vec::new(),
            bounds: [f64::MAX, f64::MAX, f64::MIN, f64::MIN],
        };
        for listed in &layout.file.modules {
            let id = &listed.module_id;
            let (Some((module, _)), Some(placement)) =
                (layout.modules.get(id), self.placements.get(id))
            else {
                continue;
            };
            let place = |points: &[Point]| {
                points
                    .iter()
                    .map(|p| placement.apply(*p))
                    .collect::<Vec<_>>()
            };
            let geometry = module.geometry.clone().unwrap_or_default();
            let outline = if geometry.outline.is_empty() {
                let l = f64::from(module.length_mm);
                place(&[[0.0, 0.0], [l, 0.0], [l, 500.0], [0.0, 500.0]])
            } else {
                place(&geometry.outline)
            };
            for p in &outline {
                view.bounds = [
                    view.bounds[0].min(p[0]),
                    view.bounds[1].min(p[1]),
                    view.bounds[2].max(p[0]),
                    view.bounds[3].max(p[1]),
                ];
            }
            let local_outline = if geometry.outline.is_empty() {
                vec![[0.0, 0.0]]
            } else {
                geometry.outline.clone()
            };
            let [cx, _] = centroid(&local_outline);
            view.modules.push(ModuleView {
                id: id.clone(),
                name: module.friendly_name.clone(),
                owner: module.owner.clone(),
                district: layout.district_of(id).unwrap_or("").to_owned(),
                placement: *placement,
                outline,
                // Below the front edge, outside the module, so it covers nothing.
                label: placement.apply([cx, -60.0]),
            });
            for (name, points) in &geometry.platforms {
                if let [a, b] = points[..] {
                    view.platforms
                        .push(place(&[a, [b[0], a[1]], b, [a[0], b[1]]]));
                } else {
                    view.platforms.push(place(points));
                }
                let _ = name;
            }
            for station in &module.stations {
                let at = geometry
                    .platforms
                    .values()
                    .next()
                    .map(|p| centroid(p))
                    .unwrap_or([f64::from(module.length_mm) / 2.0, 120.0]);
                view.stations.push(LabelView {
                    text: station.name.clone(),
                    at: placement.apply(at),
                    module: id.clone(),
                });
            }
            for turnout in &module.turnouts {
                if let Some(at) = geometry.points.get(&turnout.id) {
                    view.turnouts.push(TurnoutView {
                        id: format!("{id}/{}", turnout.id),
                        name: turnout.id.clone(),
                        at: placement.apply(*at),
                    });
                }
            }
            for building in &module.buildings {
                if let Some(outline) = geometry.buildings.get(&building.id) {
                    view.buildings.push(BuildingView {
                        id: format!("{id}/{}", building.id),
                        name: building.name.clone(),
                        outline: place(outline),
                        lights: building.lights.len(),
                    });
                }
            }
            for crossing in &module.crossings {
                let road = geometry
                    .roads
                    .get(&crossing.name)
                    .map(|r| place(r))
                    .unwrap_or_default();
                let at = crossing
                    .tracks
                    .iter()
                    .filter_map(|t| {
                        let piece = PieceId::new(id, &t.track);
                        let at = f64::from(t.at_mm);
                        self.span(&piece, at, at).first().copied()
                    })
                    .collect();
                view.crossings.push(CrossingView {
                    id: format!("{id}/{}", crossing.id),
                    name: crossing.name.clone(),
                    road,
                    at,
                });
            }
        }
        for piece in topology.pieces() {
            if let Some((line, _)) = self.pieces.get(&piece.id) {
                view.pieces.push(PieceView {
                    id: piece.id.to_string(),
                    district: piece.district.clone(),
                    points: line.clone(),
                });
            }
        }
        for (key, block) in &topology.blocks {
            let [a, b] = block.cuts_mm;
            view.blocks.push(BlockView {
                id: key.clone(),
                district: block.district.clone(),
                role: block.role.clone(),
                points: self.span(&block.piece, f64::from(a), f64::from(b)),
            });
        }
        view
    }

    pub fn snapshot(&self, sim: &LayoutSim<'_>, topology: &Topology) -> Snapshot {
        let trains = sim
            .trains
            .iter()
            .map(|train| {
                let mut points = Vec::new();
                for (piece, heading, a, b) in sim.covered_with_heading(train) {
                    let mut span = self.span(&piece, a, b);
                    // Head first: a span runs from the piece's `from`; running forward
                    // along the piece, the train's front end is at the far end.
                    if heading == Heading::Forward {
                        span.reverse();
                    }
                    points.extend(span);
                }
                TrainView {
                    name: train.name.clone(),
                    address: train.address,
                    points,
                    district: topology
                        .piece(&train.head.piece)
                        .map(|p| p.district.clone())
                        .unwrap_or_default(),
                    state: format!("{:?}", train.state)
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_owned(),
                }
            })
            .collect();
        Snapshot {
            time_s: sim.time_s,
            occupied: sim.occupied().iter().cloned().collect(),
            turnouts: sim.turnouts.clone(),
            crossings_closed: sim.closed_crossings().cloned().collect(),
            trains,
        }
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

    /// The example's modules lie in a row: each starts where the one before ends.
    #[test]
    fn modules_are_placed_end_to_end() {
        let layout = layout();
        let placed = place_modules(&layout);
        let x = |id: &str| placed[id].offset[0];
        assert_eq!(x("timo-adorf"), 0.0);
        assert!((x("timo-2e96-west") - 1800.0).abs() < 1e-6);
        assert!((x("timo-2e96-border") - 2700.0).abs() < 1e-6);
        assert!((x("anna-bdorf") - 5400.0).abs() < 1e-6);
        assert!(placed
            .values()
            .all(|p| p.angle.abs() < 1e-6 && p.offset[1].abs() < 1e-6));
    }

    /// A module turned round and joined at its other end comes out turned by 180°.
    #[test]
    fn a_turned_module_is_drawn_turned() {
        let mut layout = layout();
        // Join the east line module the other way round: its B end to the border.
        layout.file.connections[3].b = "anna-2e96-east.B".into();
        layout.file.connections[4].a = "anna-2e96-east.A".into();
        let placed = place_modules(&layout);
        let east = placed["anna-2e96-east"];
        assert!((east.angle - 180.0).abs() < 1e-6);
        // Its end B meets the border module's end B at x 4500.
        let b = east.apply([900.0, 273.0]);
        assert!(
            (b[0] - 4500.0).abs() < 1e-6 && (b[1] - 273.0).abs() < 1e-6,
            "{b:?}"
        );
    }

    #[test]
    fn sub_polylines_follow_the_track() {
        let line = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]];
        assert_eq!(
            sub_polyline(&line, 0.25, 0.75),
            [[50.0, 0.0], [100.0, 0.0], [100.0, 50.0]]
        );
    }
}
