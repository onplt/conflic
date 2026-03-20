use std::fmt;
use std::path::PathBuf;

/// The semantic type of a configuration value.
#[derive(Debug, Clone)]
pub enum SemanticType {
    Version(VersionSpec),
    Port(PortSpec),
    Boolean(bool),
    Path(PathBuf),
    StringValue(String),
    Number(f64),
}

/// How a version is specified.
#[derive(Debug, Clone)]
pub enum VersionSpec {
    /// Exact version: "20.11.0"
    Exact(semver::Version),
    /// Partial version: "20" meaning 20.x.x, or "3.12" meaning 3.12.x
    Partial { major: u64, minor: Option<u64> },
    /// Range: ">=18 <20", "^20", "~3.12"
    Range(node_semver::Range),
    /// Docker tag: "22-alpine", "3.12-slim-bookworm"
    DockerTag {
        version: String,
        variant: Option<String>,
    },
    /// Arbitrary string that looks like a version but doesn't parse cleanly
    Unparsed(String),
}

/// How a port is specified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortSpec {
    Single(u16),
    Range(u16, u16),
    /// Docker-style host:container mapping
    Mapping {
        host: u16,
        container: u16,
    },
}

impl fmt::Display for SemanticType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SemanticType::Version(v) => write!(f, "{}", v),
            SemanticType::Port(p) => write!(f, "{}", p),
            SemanticType::Boolean(b) => write!(f, "{}", b),
            SemanticType::Path(p) => write!(f, "{}", p.display()),
            SemanticType::StringValue(s) => write!(f, "{}", s),
            SemanticType::Number(n) => write!(f, "{}", n),
        }
    }
}

impl fmt::Display for VersionSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VersionSpec::Exact(v) => write!(f, "{}", v),
            VersionSpec::Partial {
                major,
                minor: Some(minor),
            } => write!(f, "{}.{}", major, minor),
            VersionSpec::Partial { major, minor: None } => write!(f, "{}", major),
            VersionSpec::Range(r) => write!(f, "{}", r),
            VersionSpec::DockerTag {
                version,
                variant: Some(var),
            } => {
                write!(f, "{}-{}", version, var)
            }
            VersionSpec::DockerTag {
                version,
                variant: None,
            } => write!(f, "{}", version),
            VersionSpec::Unparsed(s) => write!(f, "{}", s),
        }
    }
}

impl fmt::Display for PortSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PortSpec::Single(p) => write!(f, "{}", p),
            PortSpec::Range(start, end) => write!(f, "{}-{}", start, end),
            PortSpec::Mapping { host, container } => write!(f, "{}:{}", host, container),
        }
    }
}

/// Parse a version string into a VersionSpec, trying multiple strategies.
pub fn parse_version(raw: &str) -> VersionSpec {
    let trimmed = raw.trim();

    // Try exact semver first
    if let Ok(v) = semver::Version::parse(trimmed) {
        return VersionSpec::Exact(v);
    }

    // Try as npm-style range (handles ">=18 <20", "^20", "~3.12", etc.)
    if let Ok(r) = node_semver::Range::parse(trimmed) {
        // Check if it's actually a simple exact version that semver couldn't parse
        // but node-semver can (e.g., "20" or "20.11")
        return VersionSpec::Range(r);
    }

    // Try as partial version: "20" or "3.12"
    if let Some(partial) = try_parse_partial(trimmed) {
        return partial;
    }

    // Try as Docker tag: "22-alpine", "3.12-slim-bookworm"
    if let Some(docker) = try_parse_docker_tag(trimmed) {
        return docker;
    }

    VersionSpec::Unparsed(trimmed.to_string())
}

fn try_parse_partial(s: &str) -> Option<VersionSpec> {
    let parts: Vec<&str> = s.split('.').collect();
    match parts.len() {
        1 => {
            let major = parts[0].parse::<u64>().ok()?;
            Some(VersionSpec::Partial { major, minor: None })
        }
        2 => {
            let major = parts[0].parse::<u64>().ok()?;
            let minor = parts[1].parse::<u64>().ok()?;
            Some(VersionSpec::Partial {
                major,
                minor: Some(minor),
            })
        }
        _ => None,
    }
}

fn try_parse_docker_tag(s: &str) -> Option<VersionSpec> {
    // Docker tags like "22-alpine", "3.12-slim-bookworm", "20.11.0-bullseye"
    let dash_pos = s.find('-')?;
    let version_part = &s[..dash_pos];
    let variant_part = &s[dash_pos + 1..];

    // Version part must start with a digit
    if !version_part.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }

    Some(VersionSpec::DockerTag {
        version: version_part.to_string(),
        variant: Some(variant_part.to_string()),
    })
}

/// Parse a port specification string.
pub fn parse_port(raw: &str) -> Option<PortSpec> {
    let trimmed = raw.trim();
    let trimmed = match trimmed.rsplit_once('/') {
        Some((value, suffix))
            if suffix.eq_ignore_ascii_case("tcp") || suffix.eq_ignore_ascii_case("udp") =>
        {
            value
        }
        _ => trimmed,
    };

    // Docker mapping: "3000:8080", "8000-9000:80", "9090-9091:8080-8081"
    let mut mapping_parts: Vec<&str> = trimmed.split(':').map(str::trim).collect();
    if mapping_parts.len() >= 2 {
        let container = parse_port_fragment(mapping_parts.pop()?)?;
        let host = parse_port_fragment(mapping_parts.pop()?)?;
        return match (host, container) {
            (PortSpec::Single(host), PortSpec::Single(container)) => {
                Some(PortSpec::Mapping { host, container })
            }
            (_, container) => Some(container),
        };
    }

    parse_port_fragment(trimmed)
}

fn parse_port_fragment(raw: &str) -> Option<PortSpec> {
    let trimmed = raw.trim();

    if let Some((start, end)) = trimmed.split_once('-') {
        let start = start.trim().parse::<u16>().ok()?;
        let end = end.trim().parse::<u16>().ok()?;
        return Some(PortSpec::Range(start, end));
    }

    let port = trimmed.parse::<u16>().ok()?;
    Some(PortSpec::Single(port))
}

/// Normalize a boolean-like string.
pub fn normalize_boolean(raw: &str) -> Option<bool> {
    match raw.to_lowercase().trim() {
        "true" | "yes" | "on" | "1" | "enabled" | "enable" => Some(true),
        "false" | "no" | "off" | "0" | "disabled" | "disable" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_exact_version() {
        match parse_version("20.11.0") {
            VersionSpec::Exact(v) => {
                assert_eq!(v.major, 20);
                assert_eq!(v.minor, 11);
                assert_eq!(v.patch, 0);
            }
            other => panic!("Expected Exact, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_range_version() {
        match parse_version(">=18 <20") {
            VersionSpec::Range(_) => {}
            other => panic!("Expected Range, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_caret_version() {
        match parse_version("^20") {
            VersionSpec::Range(_) => {}
            other => panic!("Expected Range, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_docker_tag() {
        match parse_version("22-alpine") {
            VersionSpec::DockerTag { version, variant } => {
                assert_eq!(version, "22");
                assert_eq!(variant, Some("alpine".to_string()));
            }
            other => panic!("Expected DockerTag, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_port_single() {
        assert_eq!(parse_port("8080"), Some(PortSpec::Single(8080)));
    }

    #[test]
    fn test_parse_port_mapping() {
        assert_eq!(
            parse_port("3000:8080"),
            Some(PortSpec::Mapping {
                host: 3000,
                container: 8080
            })
        );
    }

    #[test]
    fn test_parse_port_mapping_with_host_ip() {
        assert_eq!(
            parse_port("127.0.0.1:3000:8080"),
            Some(PortSpec::Mapping {
                host: 3000,
                container: 8080
            })
        );
    }

    #[test]
    fn test_parse_port_mapping_with_host_range() {
        assert_eq!(parse_port("8000-9000:80"), Some(PortSpec::Single(80)));
    }

    #[test]
    fn test_parse_port_mapping_with_container_range() {
        assert_eq!(
            parse_port("9090-9091:8080-8081"),
            Some(PortSpec::Range(8080, 8081))
        );
    }

    #[test]
    fn test_parse_port_host_ip_without_host_port_is_rejected() {
        assert_eq!(parse_port("127.0.0.1:8080"), None);
    }

    #[test]
    fn test_normalize_boolean() {
        assert_eq!(normalize_boolean("true"), Some(true));
        assert_eq!(normalize_boolean("yes"), Some(true));
        assert_eq!(normalize_boolean("on"), Some(true));
        assert_eq!(normalize_boolean("1"), Some(true));
        assert_eq!(normalize_boolean("false"), Some(false));
        assert_eq!(normalize_boolean("no"), Some(false));
        assert_eq!(normalize_boolean("off"), Some(false));
        assert_eq!(normalize_boolean("0"), Some(false));
        assert_eq!(normalize_boolean("maybe"), None);
    }
}
