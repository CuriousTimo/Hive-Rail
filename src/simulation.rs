use std::fmt;

#[derive(Debug)]
struct RailSegment {
    id: &'static str,
    power_sources: Vec<&'static str>,
}

#[derive(Debug, PartialEq, Eq)]
struct InvariantViolation {
    segment_id: &'static str,
    power_sources: Vec<&'static str>,
}

impl fmt::Display for InvariantViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "rail segment {} has multiple power sources: {}",
            self.segment_id,
            self.power_sources.join(", ")
        )
    }
}

impl std::error::Error for InvariantViolation {}

fn validate_power_exclusivity(segments: &[RailSegment]) -> Result<(), InvariantViolation> {
    for segment in segments {
        if segment.power_sources.len() > 1 {
            return Err(InvariantViolation {
                segment_id: segment.id,
                power_sources: segment.power_sources.clone(),
            });
        }
    }
    Ok(())
}

pub fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let segments = [
        RailSegment {
            id: "district-a",
            power_sources: vec!["booster-a"],
        },
        RailSegment {
            id: "transfer-block",
            power_sources: vec!["booster-a"],
        },
        RailSegment {
            id: "district-b",
            power_sources: vec!["booster-b"],
        },
    ];

    validate_power_exclusivity(&segments)?;
    println!("simulation scaffold: booster exclusivity invariant passed");
    println!("no hardware drivers are connected; no track commands were sent");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate_power_exclusivity, RailSegment};

    #[test]
    fn accepts_segments_with_at_most_one_power_source() {
        let segments = [RailSegment {
            id: "transfer-block",
            power_sources: vec!["booster-a"],
        }];
        assert!(validate_power_exclusivity(&segments).is_ok());
    }

    #[test]
    fn rejects_a_segment_powered_by_two_boosters() {
        let segments = [RailSegment {
            id: "transfer-block",
            power_sources: vec!["booster-a", "booster-b"],
        }];
        let error = validate_power_exclusivity(&segments).unwrap_err();
        assert_eq!(error.segment_id, "transfer-block");
    }
}
