use std::fmt;

/// Layout-wide power request, from RocNet group 1 or LocoNet GPON/GPOFF/IDLE.
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

/// Actual power state of one node's district. `Unknown` (no contact with the
/// command station) is added with the real DCC-EX driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    Off,
    On,
    EmergencyStop,
    Short,
}

impl PowerState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::On => "ON",
            Self::EmergencyStop => "STOP",
            Self::Short => "SHORT",
        }
    }

    /// RocNet command station status byte: bit 0 power, bit 1 idle.
    pub fn rocnet_status(self) -> u8 {
        match self {
            Self::On => 0x01,
            Self::EmergencyStop => 0x03,
            Self::Off | Self::Short => 0x00,
        }
    }
}

/// Stand-in for DCC-EX until the serial driver exists.
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

    fn set(&mut self, next: PowerState) -> bool {
        let changed = next != self.state;
        self.state = next;
        changed
    }
}

#[cfg(test)]
mod tests {
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
