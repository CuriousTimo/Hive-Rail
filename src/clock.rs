//! Layout fast clock. Hive-Rail commands it: every node keeps the same state and counts
//! locally, the clock master sends a RocNet sync on each fast minute, and every node
//! writes LocoNet slot 123 so Rocrail, centrals and throttles follow.

use tokio::time::Instant;

const MINUTES_PER_DAY: u32 = 24 * 60;
const MILLIS_PER_MINUTE: u64 = 60_000;
pub const MAX_RATE: u8 = 60;

#[derive(Debug, Clone)]
pub struct FastClock {
    /// Fast milliseconds since midnight at `reference`.
    base_millis: u64,
    reference: Instant,
    rate: u8,
    running: bool,
    /// False until this node has set the clock or received it from the mesh.
    known: bool,
}

impl FastClock {
    /// FREMO sessions usually start in the morning; the clock stays paused until started.
    pub fn new(now: Instant) -> Self {
        Self {
            base_millis: 6 * 60 * MILLIS_PER_MINUTE,
            reference: now,
            rate: 4,
            running: false,
            known: false,
        }
    }

    fn millis(&self, now: Instant) -> u64 {
        let elapsed = if self.running {
            now.saturating_duration_since(self.reference).as_millis() as u64 * u64::from(self.rate)
        } else {
            0
        };
        (self.base_millis + elapsed) % (u64::from(MINUTES_PER_DAY) * MILLIS_PER_MINUTE)
    }

    pub fn minute_of_day(&self, now: Instant) -> u16 {
        (self.millis(now) / MILLIS_PER_MINUTE) as u16
    }

    pub fn rate(&self) -> u8 {
        self.rate
    }

    pub fn running(&self) -> bool {
        self.running
    }

    pub fn known(&self) -> bool {
        self.known
    }

    /// Applies a set or sync: the time is the start of the given minute.
    pub fn set(&mut self, minute_of_day: u16, rate: u8, running: bool, now: Instant) {
        self.base_millis =
            u64::from(u32::from(minute_of_day) % MINUTES_PER_DAY) * MILLIS_PER_MINUTE;
        self.reference = now;
        self.rate = rate.clamp(1, MAX_RATE);
        self.running = running;
        self.known = true;
    }

    pub fn toggle_running(&mut self, now: Instant) {
        self.restart(now);
        self.running = !self.running;
    }

    pub fn change_rate(&mut self, delta: i8, now: Instant) {
        self.restart(now);
        self.rate = self.rate.saturating_add_signed(delta).clamp(1, MAX_RATE);
    }

    pub fn shift_hours(&mut self, delta: i16, now: Instant) {
        let minute = i32::from(self.minute_of_day(now)) + i32::from(delta) * 60;
        let minute = minute.rem_euclid(MINUTES_PER_DAY as i32) as u16;
        let (rate, running) = (self.rate, self.running);
        self.set(minute, rate, running, now);
    }

    /// Keeps the current fast time but measures from `now`, before a rate or run change.
    fn restart(&mut self, now: Instant) {
        self.base_millis = self.millis(now);
        self.reference = now;
        self.known = true;
    }

    pub fn label(&self, now: Instant) -> String {
        if !self.known {
            return "--:--  waiting for clock".to_owned();
        }
        let minute = self.minute_of_day(now);
        let state = if self.running {
            format!("x{}", self.rate)
        } else {
            format!("paused (x{})", self.rate)
        };
        format!("{:02}:{:02}  {state}", minute / 60, minute % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::FastClock;
    use std::time::Duration;
    use tokio::time::Instant;

    #[test]
    fn counts_at_the_rate_only_while_running() {
        let start = Instant::now();
        let mut clock = FastClock::new(start);
        assert_eq!(clock.label(start), "--:--  waiting for clock");

        clock.set(6 * 60, 4, true, start);
        assert_eq!(clock.label(start), "06:00  x4");
        // One real minute is four fast minutes.
        assert_eq!(
            clock.minute_of_day(start + Duration::from_secs(60)),
            6 * 60 + 4
        );

        clock.toggle_running(start + Duration::from_secs(60));
        assert_eq!(
            clock.label(start + Duration::from_secs(600)),
            "06:04  paused (x4)"
        );
    }

    #[test]
    fn rate_and_hour_changes_keep_the_current_time() {
        let start = Instant::now();
        let mut clock = FastClock::new(start);
        clock.set(23 * 60 + 30, 1, true, start);

        let later = start + Duration::from_secs(120);
        clock.change_rate(2, later);
        assert_eq!(clock.label(later), "23:32  x3");
        assert_eq!(
            clock.minute_of_day(later + Duration::from_secs(20)),
            23 * 60 + 33
        );

        clock.shift_hours(1, later);
        assert_eq!(clock.label(later), "00:32  x3");
        clock.change_rate(-10, later);
        assert_eq!(clock.rate(), 1);
    }
}
