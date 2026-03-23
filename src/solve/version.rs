use super::Compatibility;
use crate::model::VersionSpec;

/// Convert a semver::Version to a node_semver::Version for range checks.
pub(crate) fn to_node_version_pub(v: &semver::Version) -> node_semver::Version {
    to_node_version(v)
}

fn to_node_version(v: &semver::Version) -> node_semver::Version {
    node_semver::Version {
        major: v.major,
        minor: v.minor,
        patch: v.patch,
        build: to_node_identifiers(v.build.as_str()),
        pre_release: to_node_identifiers(v.pre.as_str()),
    }
}

fn to_node_identifiers(raw: &str) -> Vec<node_semver::Identifier> {
    if raw.is_empty() {
        return Vec::new();
    }

    raw.split('.')
        .map(|segment| {
            segment
                .parse::<u64>()
                .map(node_semver::Identifier::Numeric)
                .unwrap_or_else(|_| node_semver::Identifier::AlphaNumeric(segment.to_string()))
        })
        .collect()
}

/// Check if two version specs are compatible.
pub fn versions_compatible(a: &VersionSpec, b: &VersionSpec) -> Compatibility {
    match (a, b) {
        (VersionSpec::Exact(va), VersionSpec::Exact(vb)) => {
            if va == vb {
                Compatibility::Compatible
            } else {
                Compatibility::Incompatible(format!("\"{}\" differs from \"{}\"", va, vb))
            }
        }

        (VersionSpec::Exact(v), VersionSpec::Range(r))
        | (VersionSpec::Range(r), VersionSpec::Exact(v)) => {
            let nv = to_node_version(v);
            if r.satisfies(&nv) {
                Compatibility::Compatible
            } else {
                Compatibility::Incompatible(format!("\"{}\" does not satisfy \"{}\"", v, r))
            }
        }

        (VersionSpec::Range(ra), VersionSpec::Range(rb)) => {
            if ra.intersect(rb).is_some() {
                Compatibility::Compatible
            } else {
                Compatibility::Incompatible(format!(
                    "ranges \"{}\" and \"{}\" do not overlap",
                    ra, rb
                ))
            }
        }

        (
            VersionSpec::Partial {
                major: ma,
                minor: mia,
            },
            VersionSpec::Partial {
                major: mb,
                minor: mib,
            },
        ) => {
            if ma != mb {
                Compatibility::Incompatible(format!("major version {} differs from {}", ma, mb))
            } else if mia.is_some() && mib.is_some() && mia != mib {
                Compatibility::Incompatible(format!(
                    "version {}.{} differs from {}.{}",
                    ma,
                    mia.unwrap(),
                    mb,
                    mib.unwrap()
                ))
            } else {
                Compatibility::Compatible
            }
        }

        // Partial vs Exact: expand partial and compare
        (VersionSpec::Partial { major, minor }, VersionSpec::Exact(v))
        | (VersionSpec::Exact(v), VersionSpec::Partial { major, minor }) => {
            if v.major != *major {
                return Compatibility::Incompatible(format!(
                    "version {} has major version {}, expected {}",
                    v, v.major, major
                ));
            }
            if let Some(m) = minor
                && v.minor != *m
            {
                return Compatibility::Incompatible(format!(
                    "version {} has minor version {}, expected {}",
                    v, v.minor, m
                ));
            }
            Compatibility::Compatible
        }

        // Partial vs Range: expand partial to range
        (VersionSpec::Partial { major, minor }, VersionSpec::Range(r))
        | (VersionSpec::Range(r), VersionSpec::Partial { major, minor }) => {
            let test_version = match minor {
                Some(m) => node_semver::Version::from((*major, *m, 0_u64)),
                None => node_semver::Version::from((*major, 0_u64, 0_u64)),
            };
            if r.satisfies(&test_version) {
                Compatibility::Compatible
            } else {
                Compatibility::Incompatible(format!(
                    "version {}.{} does not satisfy \"{}\"",
                    major,
                    minor.map_or("x".to_string(), |m| m.to_string()),
                    r
                ))
            }
        }

        // DockerTag: extract version part and recurse
        (VersionSpec::DockerTag { version: v, .. }, other)
        | (other, VersionSpec::DockerTag { version: v, .. }) => {
            let parsed = crate::model::parse_version(v);
            // Avoid infinite recursion if it parses back to DockerTag
            if matches!(&parsed, VersionSpec::DockerTag { .. }) {
                Compatibility::Unknown
            } else {
                versions_compatible(&parsed, other)
            }
        }

        // Unparsed: string equality
        (VersionSpec::Unparsed(a), VersionSpec::Unparsed(b)) => {
            if a == b {
                Compatibility::Compatible
            } else {
                Compatibility::Incompatible(format!("\"{}\" differs from \"{}\"", a, b))
            }
        }

        // Unparsed vs anything else: unknown
        (VersionSpec::Unparsed(_), _) | (_, VersionSpec::Unparsed(_)) => Compatibility::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::parse_version;

    fn is_compatible(a: &str, b: &str) -> bool {
        let va = parse_version(a);
        let vb = parse_version(b);
        matches!(versions_compatible(&va, &vb), Compatibility::Compatible)
    }

    fn is_incompatible(a: &str, b: &str) -> bool {
        let va = parse_version(a);
        let vb = parse_version(b);
        matches!(
            versions_compatible(&va, &vb),
            Compatibility::Incompatible(_)
        )
    }

    #[test]
    fn test_exact_match() {
        assert!(is_compatible("20.11.0", "20.11.0"));
    }

    #[test]
    fn test_exact_mismatch() {
        assert!(is_incompatible("20.11.0", "22.0.0"));
    }

    #[test]
    fn test_exact_satisfies_range() {
        assert!(is_compatible("20.0.0", ">=18.0.0 <21.0.0"));
    }

    #[test]
    fn test_exact_outside_range() {
        assert!(is_incompatible("22.0.0", ">=18.0.0 <20.0.0"));
    }

    #[test]
    fn test_prerelease_exact_does_not_satisfy_stable_range() {
        assert!(is_incompatible("20.0.0-rc.1", ">=20.0.0"));
    }

    #[test]
    fn test_ranges_overlap() {
        assert!(is_compatible(">=18.0.0 <22.0.0", ">=20.0.0 <24.0.0"));
    }

    #[test]
    fn test_ranges_no_overlap() {
        assert!(is_incompatible(">=18.0.0 <20.0.0", ">=22.0.0 <24.0.0"));
    }

    #[test]
    fn test_narrow_ranges_overlap() {
        assert!(is_compatible(">=20.2.0 <20.4.0", ">=20.3.0 <20.5.0"));
    }

    #[test]
    fn test_narrow_ranges_touching_bounds_do_not_overlap() {
        assert!(is_incompatible(">=20.2.0 <20.3.0", ">=20.3.0 <20.5.0"));
    }

    #[test]
    fn test_partial_vs_exact_compatible() {
        assert!(is_compatible("20.11.0", "20"));
    }

    #[test]
    fn test_partial_vs_exact_incompatible() {
        assert!(is_incompatible("22.0.0", "20"));
    }

    #[test]
    fn test_docker_tag_vs_exact() {
        assert!(is_compatible("20.11.0", "20-alpine"));
    }

    #[test]
    fn test_docker_tag_vs_incompatible() {
        assert!(is_incompatible("22.0.0", "20-alpine"));
    }
}
