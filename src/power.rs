use std::fmt;

/// Power request for one district, from RocNet group 1 or LocoNet GPON/GPOFF/IDLE.
/// There is no layout-wide power: every district keeps running independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerRequest {
    On,
    Off,
    Halt,
}

impl fmt::Display for PowerRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::On => "power on",
            Self::Off => "power off",
            Self::Halt => "emergency stop",
        })
    }
}

/// A global resume ends a global emergency stop: a district in emergency stop goes back on;
/// one that is off stays off, and one with a short stays in short until it is cleared.
pub fn resumes(state: PowerState) -> bool {
    state == PowerState::EmergencyStop
}

/// Actual power state of one node's district.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    Off,
    On,
    EmergencyStop,
    Short,
    /// No contact with the command station.
    Unknown,
}

impl PowerState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::On => "ON",
            Self::EmergencyStop => "STOP",
            Self::Short => "SHORT",
            Self::Unknown => "UNKNOWN",
        }
    }

    /// RocNet command station status byte: bit 0 power, bit 1 idle.
    pub fn rocnet_status(self) -> u8 {
        match self {
            Self::On => 0x01,
            Self::EmergencyStop => 0x03,
            Self::Off | Self::Short | Self::Unknown => 0x00,
        }
    }
}

/// The district's power state: simulated, or following what a real command station
/// reports through [`report`](Self::report) and [`report_power`](Self::report_power).
#[derive(Debug)]
pub struct SimulatedCommandStation {
    state: PowerState,
}

impl SimulatedCommandStation {
    pub fn new() -> Self {
        Self {
            state: PowerState::Off,
        }
    }

    pub fn state(&self) -> PowerState {
        self.state
    }

    /// Applies a requested power change and reports whether the state changed.
    pub fn apply(&mut self, request: PowerRequest) -> bool {
        let next = match (request, self.state) {
            (PowerRequest::On, _) => PowerState::On,
            (PowerRequest::Off, _) => PowerState::Off,
            (PowerRequest::Halt, PowerState::On) => PowerState::EmergencyStop,
            (PowerRequest::Halt, current) => current,
        };
        self.set(next)
    }

    /// Power lost without a request, as a booster trip would cause.
    pub fn simulate_short(&mut self) -> bool {
        match self.state {
            PowerState::On | PowerState::EmergencyStop => self.set(PowerState::Short),
            _ => false,
        }
    }

    /// A state reported by the command station, such as a short or lost contact.
    pub fn report(&mut self, state: PowerState) -> bool {
        self.set(state)
    }

    /// DCC-EX's `<p1>`/`<p0>`. Power stays on during an emergency stop, so `<p1>` does
    /// not end it.
    pub fn report_power(&mut self, on: bool) -> bool {
        match (on, self.state) {
            (true, PowerState::On | PowerState::EmergencyStop) => false,
            (true, _) => self.set(PowerState::On),
            (false, _) => self.set(PowerState::Off),
        }
    }

    fn set(&mut self, next: PowerState) -> bool {
        let changed = next != self.state;
        self.state = next;
        changed
    }
}

/// The layout-wide state, apart from each district's own power (which every list shows):
/// whether a global emergency stop is active. It is active from the global stop until the
/// global go, or until no district is in emergency stop any more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalState {
    pub label: String,
    pub stopped: bool,
}

/// `districts`: each online district's power label. `global_stop`: who sent the last global
/// emergency stop and when (cleared by a global go).
pub fn global_state(districts: &[&str], global_stop: Option<&(String, String)>) -> GlobalState {
    match global_stop {
        Some((origin, time)) if districts.contains(&"STOP") => GlobalState {
            label: format!("STOP by {origin} at {time}"),
            stopped: true,
        },
        _ => GlobalState {
            label: "normal".to_owned(),
            stopped: false,
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_global_state_is_a_global_stop_or_normal() {
        use super::global_state;
        let stop = ("M5StickC".to_owned(), "12:31:34".to_owned());
        let stopped = global_state(&["OFF", "STOP", "SHORT"], Some(&stop));
        assert_eq!(stopped.label, "STOP by M5StickC at 12:31:34");
        assert!(stopped.stopped);
        // Once no district is in emergency stop, the global stop is over.
        assert!(!global_state(&["ON", "OFF", "SHORT"], Some(&stop)).stopped);
        // A district's own stop or short is not a global stop.
        assert_eq!(global_state(&["STOP", "SHORT"], None).label, "normal");
    }

    use super::{PowerRequest, PowerState, SimulatedCommandStation};

    #[test]
    fn starts_off_and_follows_on_off_requests() {
        let mut station = SimulatedCommandStation::new();
        assert_eq!(station.state(), PowerState::Off);

        assert!(station.apply(PowerRequest::On));
        assert_eq!(station.state(), PowerState::On);
        assert!(!station.apply(PowerRequest::On));

        assert!(station.apply(PowerRequest::Off));
        assert_eq!(station.state(), PowerState::Off);
    }

    #[test]
    fn halt_only_stops_a_powered_district() {
        let mut station = SimulatedCommandStation::new();
        assert!(!station.apply(PowerRequest::Halt));
        assert_eq!(station.state(), PowerState::Off);

        station.apply(PowerRequest::On);
        assert!(station.apply(PowerRequest::Halt));
        assert_eq!(station.state(), PowerState::EmergencyStop);
    }

    #[test]
    fn reported_power_keeps_an_emergency_stop() {
        let mut station = SimulatedCommandStation::new();
        assert!(station.report(PowerState::Unknown));
        assert!(station.report_power(true));
        station.apply(PowerRequest::Halt);
        assert!(!station.report_power(true));
        assert_eq!(station.state(), PowerState::EmergencyStop);
        assert!(station.report_power(false));
        assert_eq!(station.state(), PowerState::Off);
    }

    #[test]
    fn short_needs_power_and_is_cleared_by_power_on() {
        let mut station = SimulatedCommandStation::new();
        assert!(!station.simulate_short());

        station.apply(PowerRequest::On);
        assert!(station.simulate_short());
        assert_eq!(station.state(), PowerState::Short);
        assert_eq!(station.state().rocnet_status(), 0);

        assert!(station.apply(PowerRequest::On));
        assert_eq!(station.state(), PowerState::On);
    }
}
