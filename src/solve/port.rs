use super::Compatibility;
use crate::model::PortSpec;

pub fn ports_compatible(a: &PortSpec, b: &PortSpec) -> Compatibility {
    let interval_a = port_interval(a);
    let interval_b = port_interval(b);

    if intervals_overlap(interval_a, interval_b) {
        Compatibility::Compatible
    } else {
        Compatibility::Incompatible(incompatible_port_message(a, b, interval_a, interval_b))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PortInterval {
    start: u16,
    end: u16,
}

/// Convert a port specification into the effective application-port interval.
/// For a Docker mapping host:container, the container port is the app port.
fn port_interval(spec: &PortSpec) -> PortInterval {
    match spec {
        PortSpec::Single(port) => PortInterval {
            start: *port,
            end: *port,
        },
        PortSpec::Range(start, end) => PortInterval {
            start: (*start).min(*end),
            end: (*start).max(*end),
        },
        PortSpec::Mapping { container, .. } => PortInterval {
            start: *container,
            end: *container,
        },
    }
}

fn intervals_overlap(left: PortInterval, right: PortInterval) -> bool {
    left.start <= right.end && right.start <= left.end
}

fn incompatible_port_message(
    left: &PortSpec,
    right: &PortSpec,
    left_interval: PortInterval,
    right_interval: PortInterval,
) -> String {
    match (
        left_interval.start == left_interval.end,
        right_interval.start == right_interval.end,
    ) {
        (true, true) => format!(
            "port {} differs from {}",
            left_interval.start, right_interval.start
        ),
        (true, false) => format!("port {} is outside {}", left_interval.start, right),
        (false, true) => format!("port {} does not include {}", left, right_interval.start),
        (false, false) => format!("port range {} does not overlap {}", left, right),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_same_port() {
        let a = PortSpec::Single(8080);
        let b = PortSpec::Single(8080);
        assert!(matches!(
            ports_compatible(&a, &b),
            Compatibility::Compatible
        ));
    }

    #[test]
    fn test_different_port() {
        let a = PortSpec::Single(8080);
        let b = PortSpec::Single(3000);
        assert!(matches!(
            ports_compatible(&a, &b),
            Compatibility::Incompatible(_)
        ));
    }

    #[test]
    fn test_mapping_container_matches() {
        let a = PortSpec::Single(8080);
        let b = PortSpec::Mapping {
            host: 3000,
            container: 8080,
        };
        assert!(matches!(
            ports_compatible(&a, &b),
            Compatibility::Compatible
        ));
    }

    #[test]
    fn test_mapping_container_mismatch() {
        let a = PortSpec::Single(8080);
        let b = PortSpec::Single(3000);
        assert!(matches!(
            ports_compatible(&a, &b),
            Compatibility::Incompatible(_)
        ));
    }

    #[test]
    fn test_single_port_inside_range_is_compatible() {
        let a = PortSpec::Single(3001);
        let b = PortSpec::Range(3000, 3005);
        assert!(matches!(
            ports_compatible(&a, &b),
            Compatibility::Compatible
        ));
    }

    #[test]
    fn test_single_port_on_range_boundary_is_compatible() {
        let a = PortSpec::Single(3000);
        let b = PortSpec::Range(3000, 3005);
        assert!(matches!(
            ports_compatible(&a, &b),
            Compatibility::Compatible
        ));
    }

    #[test]
    fn test_single_port_outside_range_is_incompatible() {
        let a = PortSpec::Single(3006);
        let b = PortSpec::Range(3000, 3005);
        assert!(matches!(
            ports_compatible(&a, &b),
            Compatibility::Incompatible(_)
        ));
    }

    #[test]
    fn test_overlapping_ranges_are_compatible() {
        let a = PortSpec::Range(3000, 3005);
        let b = PortSpec::Range(3005, 3010);
        assert!(matches!(
            ports_compatible(&a, &b),
            Compatibility::Compatible
        ));
    }
}
