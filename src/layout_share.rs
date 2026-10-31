//! Sharing the layout over the mesh: a full node started with `--layout` answers each
//! district's request for its part of the layout, so a lite node (a microcontroller with
//! no files) learns what its district contains. The answer is a small JSON "district
//! package": the modules, blocks with their occupancy addresses, sensors, signals,
//! turnouts, level crossings and borders, and the layout's checksum. No drawing geometry:
//! the web view stays on the full nodes.
//!
//! Key: `hive/<session>/layout/district/<district>`; every full node with the layout
//! answers, so a lite node gets it from whichever is reachable.

use crate::layout::{self, Layout};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct DistrictPackage {
    pub district: String,
    pub meeting: String,
    /// Changes whenever the layout or any of its module files changes.
    pub layout_checksum: String,
    pub modules: Vec<String>,
    pub blocks: Vec<BlockEntry>,
    pub sensors: Vec<DeviceEntry>,
    pub signals: Vec<SignalEntry>,
    pub turnouts: Vec<DeviceEntry>,
    pub crossings: Vec<String>,
    pub borders: Vec<BorderEntry>,
}

#[derive(Debug, Serialize)]
pub struct BlockEntry {
    /// `module/block`.
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// The occupancy detector's LocoNet sensor address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<u16>,
}

#[derive(Debug, Serialize)]
pub struct DeviceEntry {
    /// `module/device`.
    pub id: String,
    pub bus: String,
    pub address: u16,
}

#[derive(Debug, Serialize)]
pub struct SignalEntry {
    /// `module/signal`.
    pub id: String,
    pub address: u16,
    pub aspects: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct BorderEntry {
    /// The neighbouring district.
    pub to: String,
    /// This district's end at the border, `module.end`.
    pub here: String,
}

/// The layout's checksum: over the layout file's districts and modules and each module
/// file's checksum, so it changes when any of them does.
pub fn layout_checksum(layout: &Layout) -> String {
    let mut text = String::new();
    for district in &layout.file.districts {
        text.push_str(&district.node);
        text.push(':');
        text.push_str(&district.modules.join(","));
        text.push(';');
    }
    for (id, (_, checksum)) in &layout.modules {
        text.push_str(id);
        text.push('=');
        text.push_str(checksum);
        text.push(';');
    }
    layout::checksum(text.as_bytes())
}

/// Every district's package, by district (node) name.
pub fn district_packages(layout: &Layout) -> BTreeMap<String, DistrictPackage> {
    let checksum = layout_checksum(layout);
    let borders = layout.borders();
    let mut packages = BTreeMap::new();
    for district in &layout.file.districts {
        let mut package = DistrictPackage {
            district: district.node.clone(),
            meeting: layout.file.meeting.clone(),
            layout_checksum: checksum.clone(),
            modules: district.modules.clone(),
            blocks: Vec::new(),
            sensors: Vec::new(),
            signals: Vec::new(),
            turnouts: Vec::new(),
            crossings: Vec::new(),
            borders: Vec::new(),
        };
        for module_id in &district.modules {
            let Some((module, _)) = layout.modules.get(module_id) else {
                continue;
            };
            let device = |id: &str| module.devices.iter().find(|d| d.id == id);
            for block in &module.blocks {
                package.blocks.push(BlockEntry {
                    id: format!("{module_id}/{}", block.id),
                    role: block.role.clone(),
                    address: block
                        .occupancy
                        .as_deref()
                        .and_then(device)
                        .map(|d| d.address),
                });
            }
            for d in &module.devices {
                let id = format!("{module_id}/{}", d.id);
                match d.kind.as_str() {
                    "sensor" => package.sensors.push(DeviceEntry {
                        id,
                        bus: d.bus.clone(),
                        address: d.address,
                    }),
                    "turnout" => package.turnouts.push(DeviceEntry {
                        id,
                        bus: d.bus.clone(),
                        address: d.address,
                    }),
                    "signal" => package.signals.push(SignalEntry {
                        id,
                        address: d.address,
                        aspects: d
                            .aspects
                            .as_ref()
                            .and_then(|a| a.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(str::to_owned))
                                    .collect()
                            })
                            .unwrap_or_else(|| vec!["red".to_owned(), "green".to_owned()]),
                    }),
                    _ => {}
                }
            }
            for crossing in &module.crossings {
                package.crossings.push(crossing.name.clone());
            }
        }
        for border in &borders {
            let (a_district, b_district) = &border.districts;
            if *a_district == district.node {
                package.borders.push(BorderEntry {
                    to: b_district.clone(),
                    here: border.a.clone(),
                });
            } else if *b_district == district.node {
                package.borders.push(BorderEntry {
                    to: a_district.clone(),
                    here: border.b.clone(),
                });
            }
        }
        packages.insert(district.node.clone(), package);
    }
    packages
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_m5stickc_gets_its_two_modules_and_both_borders() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/layout");
        let layout = layout::load(&dir.join("bench-m5.json"), &dir).unwrap();
        let packages = district_packages(&layout);
        let m5 = &packages["M5StickC"];
        assert_eq!(m5.modules, ["m5-2e96-in", "m5-2e96-out"]);
        assert_eq!(m5.blocks.len(), 4);
        assert!(m5
            .blocks
            .iter()
            .any(|b| b.role.as_deref() == Some("transfer") && b.address.is_some()));
        let mut neighbours: Vec<&str> = m5.borders.iter().map(|b| b.to.as_str()).collect();
        neighbours.sort();
        assert_eq!(neighbours, ["MacBook", "Windows"]);
        assert_eq!(m5.signals.len(), 2);
        // Small enough for a microcontroller.
        let json = serde_json::to_string(m5).unwrap();
        assert!(json.len() < 4096, "{} bytes", json.len());
        // Every district carries the same layout checksum.
        assert_eq!(packages["MacBook"].layout_checksum, m5.layout_checksum);
    }
}
