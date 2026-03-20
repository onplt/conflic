use crate::model::PortSpec;
use super::Compatibility;

pub fn ports_compatible(a: &PortSpec, b: &PortSpec) -> Compatibility {
    let port_a = effective_app_port(a);
    let port_b = effective_app_port(b);

    if port_a == port_b {
        Compatibility::Compatible
    } else {
        Compatibility::Incompatible(format!(
            "port {} differs from {}",
            port_a, port_b
        ))
    }
}

/// Get the effective application port.
/// For a Docker mapping host:container, the container port is the app port.
fn effective_app_port(spec: &PortSpec) -> u16 {
    match spec {
        PortSpec::Single(p) => *p,
        PortSpec::Range(start, _) => *start, // Use start of range
        PortSpec::Mapping { container, .. } => *container,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_same_port() {
        let a = PortSpec::Single(8080);
        let b = PortSpec::Single(8080);
        assert!(matches!(ports_compatible(&a, &b), Compatibility::Compatible));
    }

    #[test]
    fn test_different_port() {
        let a = PortSpec::Single(8080);
        let b = PortSpec::Single(3000);
        assert!(matches!(ports_compatible(&a, &b), Compatibility::Incompatible(_)));
    }

    #[test]
    fn test_mapping_container_matches() {
        // .env PORT=8080, docker-compose 3000:8080 — container port matches
        let a = PortSpec::Single(8080);
        let b = PortSpec::Mapping { host: 3000, container: 8080 };
        assert!(matches!(ports_compatible(&a, &b), Compatibility::Compatible));
    }

    #[test]
    fn test_mapping_container_mismatch() {
        // .env PORT=8080, Dockerfile EXPOSE 3000 — mismatch
        let a = PortSpec::Single(8080);
        let b = PortSpec::Single(3000);
        assert!(matches!(ports_compatible(&a, &b), Compatibility::Incompatible(_)));
    }
}
