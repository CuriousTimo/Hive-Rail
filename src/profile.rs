#![allow(dead_code)]

use serde::Deserialize;
use std::{collections::HashSet, fs, io, path::Path};
use thiserror::Error;

#[derive(Debug, Deserialize)]
pub struct ClusterConfig {
    pub schema_version: u32,
    pub cluster_id: String,
    pub friendly_name: String,
    pub hardware_setup: HardwareSetup,
    pub declared_ports: Vec<DeclaredPort>,
    #[serde(default)]
    pub modules: Vec<ModuleConfig>,
}

#[derive(Debug, Deserialize)]
pub struct HardwareSetup {
    pub command_station_type: String,
    pub connection_type: String,
    pub connection_address: String,
}

#[derive(Debug, Deserialize)]
pub struct DeclaredPort {
    pub port_id: String,
    pub label: String,
    pub boundary: Option<Boundary>,
}

#[derive(Debug, Deserialize)]
pub struct Boundary {
    pub sync: BoosterSync,
    #[serde(default)]
    pub transfer_block: bool,
    pub edge_sensor: u16,
    pub seam_sensor: Option<u16>,
    pub seam_relay: Option<u16>,
    pub signal: u16,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BoosterSync {
    Independent,
    SharedDcc,
}

#[derive(Debug, Deserialize)]
pub struct ModuleConfig {
    pub module_id: String,
    pub friendly_name: String,
    pub length_mm: u32,
    pub devices: Vec<DeviceConfig>,
}

#[derive(Debug, Deserialize)]
pub struct DeviceConfig {
    #[serde(rename = "type")]
    pub device_type: String,
    pub local_address: u16,
    pub purpose: String,
}

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("could not read profile: {0}")]
    Io(#[from] io::Error),
    #[error("invalid profile JSON: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("invalid profile: {0}")]
    Invalid(String),
}

pub fn load(path: &Path) -> Result<ClusterConfig, ProfileError> {
    let text = fs::read_to_string(path)?;
    let profile: ClusterConfig = serde_json::from_str(&text)?;
    profile.validate()?;
    Ok(profile)
}

impl ClusterConfig {
    pub fn validate(&self) -> Result<(), ProfileError> {
        if self.schema_version != 1 {
            return Err(ProfileError::Invalid(format!(
                "unsupported schema_version {}; expected 1",
                self.schema_version
            )));
        }
        if self.cluster_id.trim().is_empty() {
            return Err(ProfileError::Invalid("cluster_id must not be empty".into()));
        }
        if self.friendly_name.trim().is_empty() {
            return Err(ProfileError::Invalid(
                "friendly_name must not be empty".into(),
            ));
        }

        let mut port_ids = HashSet::new();
        for port in &self.declared_ports {
            if !port_ids.insert(&port.port_id) {
                return Err(ProfileError::Invalid(format!(
                    "duplicate port_id {}",
                    port.port_id
                )));
            }

            if let Some(boundary) = &port.boundary {
                let incomplete = boundary.seam_sensor.is_none() || boundary.seam_relay.is_none();
                if boundary.sync == BoosterSync::Independent
                    && boundary.transfer_block
                    && incomplete
                {
                    return Err(ProfileError::Invalid(format!(
                        "port {}: transfer_block requires seam_sensor and seam_relay",
                        port.port_id
                    )));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{load, BoosterSync, ClusterConfig, ProfileError};
    use std::path::Path;

    #[test]
    fn example_profile_is_valid() {
        let profile = load(Path::new("examples/module_profile.json")).unwrap();
        assert_eq!(profile.schema_version, 1);
        assert_eq!(profile.declared_ports.len(), 2);
    }

    #[test]
    fn independent_transfer_block_requires_sensor_and_relay() {
        let mut profile: ClusterConfig = serde_json::from_str(
            r#"{
                "schema_version": 1,
                "cluster_id": "02.01.0d.00.00.42",
                "friendly_name": "Test node",
                "hardware_setup": {
                    "command_station_type": "DCC_EX_Serial",
                    "connection_type": "Serial",
                    "connection_address": "/dev/ttyUSB0"
                },
                "declared_ports": [{
                    "port_id": "A",
                    "label": "West Main",
                    "boundary": {
                        "sync": "independent",
                        "transfer_block": true,
                        "edge_sensor": 104,
                        "signal": 21
                    }
                }]
            }"#,
        )
        .unwrap();

        assert!(matches!(
            profile.validate(),
            Err(ProfileError::Invalid(message)) if message.contains("seam_sensor and seam_relay")
        ));

        profile.declared_ports[0].boundary.as_mut().unwrap().sync = BoosterSync::SharedDcc;
        assert!(profile.validate().is_ok());
    }
}
