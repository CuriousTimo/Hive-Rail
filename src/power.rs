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

/// The layout's power at a glance, from every online district's power: what the
/// dispatcher's header and a node's screen show. Each district still runs on its own; this
/// is only the summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutPower {
    pub label: String,
    pub level: LayoutLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutLevel {
    /// Every district on.
    Running,
    /// Some on, some off or unknown.
    Partly,
    /// Every district off.
    Off,
    /// A district in emergency stop or short.
    Stopped,
}

/// `districts`: each online district's name and power label (ON, OFF, STOP, SHORT,
/// UNKNOWN). `global_stop`: who sent the last global emergency stop and when; it is shown
/// while any district is still in emergency stop.
pub fn layout_power(
    districts: &[(&str, &str)],
    global_stop: Option<&(String, String)>,
) -> LayoutPower {
    let named = |power: &str| {
        districts
            .iter()
            .filter(|(_, p)| *p == power)
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
    };
    let (stopped, shorted, off) = (named("STOP"), named("SHORT"), named("OFF"));
    let on = districts.iter().filter(|(_, p)| *p == "ON").count();
    if !stopped.is_empty() || !shorted.is_empty() {
        let mut parts = Vec::new();
        if !shorted.is_empty() {
            parts.push(format!("SHORT in {}", shorted.join(", ")));
        }
        if !stopped.is_empty() {
            parts.push(format!("STOP in {}", stopped.join(", ")));
        }
        // The list of districts shows which ones are stopped; a global stop needs no list.
        let label = match global_stop {
            Some((origin, time)) if !stopped.is_empty() => {
                format!("GLOBAL STOP by {origin} at {time}")
            }
            _ => parts.join("; "),
        };
        return LayoutPower {
            label,
            level: LayoutLevel::Stopped,
        };
    }
    if districts.is_empty() {
        return LayoutPower {
            label: "no districts".to_owned(),
            level: LayoutLevel::Off,
        };
    }
    if on == districts.len() {
        return LayoutPower {
            label: format!("ALL ON ({on})"),
            level: LayoutLevel::Running,
        };
    }
    if off.len() == districts.len() {
        return LayoutPower {
            label: format!("ALL OFF ({})", off.len()),
            level: LayoutLevel::Off,
        };
    }
    let mut parts = vec![format!("{on} ON")];
    if !off.is_empty() {
        parts.push(format!("OFF: {}", off.join(", ")));
    }
    let unknown = named("UNKNOWN");
    if !unknown.is_empty() {
        parts.push(format!("UNKNOWN: {}", unknown.join(", ")));
    }
    LayoutPower {
        label: parts.join(", "),
        level: LayoutLevel::Partly,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn layout_power_sums_up_the_districts() {
        use super::{layout_power, LayoutLevel};
        let stop = ("M5StickC".to_owned(), "12:31:34".to_owned());
        let all_on = layout_power(&[("MacBook", "ON"), ("Windows", "ON")], None);
        assert_eq!(
            (all_on.label.as_str(), all_on.level),
            ("ALL ON (2)", LayoutLevel::Running)
        );
        let partly = layout_power(&[("MacBook", "OFF"), ("Windows", "ON")], Some(&stop));
        assert_eq!(partly.label, "1 ON, OFF: MacBook");
        assert_eq!(partly.level, LayoutLevel::Partly);
        let stopped = layout_power(
            &[
                ("MacBook", "OFF"),
                ("Windows", "STOP"),
                ("M5StickC", "SHORT"),
            ],
            Some(&stop),
        );
        assert_eq!(stopped.label, "GLOBAL STOP by M5StickC at 12:31:34");
        assert_eq!(stopped.level, LayoutLevel::Stopped);
        // A short alone is not a global stop.
        let short = layout_power(&[("Windows", "SHORT")], Some(&stop));
        assert_eq!(short.label, "SHORT in Windows");
        assert_eq!(layout_power(&[("A", "OFF")], None).label, "ALL OFF (1)");
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
