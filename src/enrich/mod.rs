pub mod cache;
pub mod eol;

use crate::config::ConflicConfig;
use crate::model::*;

/// Metadata enrichment for a config assertion.
#[derive(Debug, Clone, Default)]
pub struct AssertionMetadata {
    /// End-of-life date as ISO string (YYYY-MM-DD), if known.
    pub eol_date: Option<String>,
    /// Whether this version is deprecated.
    pub deprecated: bool,
    /// Latest stable version in this cycle, if known.
    pub latest_stable: Option<String>,
    /// Whether the version is LTS.
    pub lts: bool,
}

/// Lifecycle data for a single version cycle.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VersionLifecycle {
    /// The version cycle identifier (e.g. "18", "20", "3.12").
    pub cycle: String,
    /// Release date (ISO 8601).
    #[serde(default)]
    pub release_date: Option<String>,
    /// End-of-life date (ISO 8601).
    #[serde(default)]
    pub eol_date: Option<String>,
    /// Whether this is an LTS release.
    #[serde(default)]
    pub lts: bool,
    /// Latest patch version in this cycle.
    #[serde(default)]
    pub latest_patch: Option<String>,
}

/// The registry database loaded from cache.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct RegistryDb {
    /// Maps concept family (e.g. "node", "python") → lifecycle entries.
    pub families: std::collections::HashMap<String, Vec<VersionLifecycle>>,
    /// ISO timestamp when the cache was last updated.
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// Annotate assertions with enrichment metadata from the registry cache.
pub fn annotate_assertions(
    concept_results: &mut [ConceptResult],
    registry: &RegistryDb,
    config: &ConflicConfig,
    today: &str,
) {
    for cr in concept_results.iter_mut() {
        let family = concept_to_family(&cr.concept.id);
        let lifecycles = match registry.families.get(&family) {
            Some(lc) => lc,
            None => continue,
        };

        for assertion in &cr.assertions {
            let cycle = extract_cycle_from_assertion(assertion);
            if let Some(lifecycle) = lifecycles.iter().find(|lc| lc.cycle == cycle) {
                // Check EOL policy violations
                if let Some(ref eol_date) = lifecycle.eol_date {
                    let days_until = days_between(today, eol_date);
                    check_eol_policies(
                        &mut cr.findings,
                        assertion,
                        config,
                        eol_date,
                        days_until,
                        lifecycle,
                    );
                }
            }
        }
    }
}

/// Look up enrichment metadata for a single assertion.
pub fn lookup_metadata(
    assertion: &ConfigAssertion,
    registry: &RegistryDb,
    today: &str,
) -> Option<AssertionMetadata> {
    let family = concept_to_family(&assertion.concept.id);
    let lifecycles = registry.families.get(&family)?;
    let cycle = extract_cycle_from_assertion(assertion);

    let lifecycle = lifecycles.iter().find(|lc| lc.cycle == cycle)?;

    let deprecated = lifecycle
        .eol_date
        .as_ref()
        .map(|eol| eol.as_str() <= today)
        .unwrap_or(false);

    Some(AssertionMetadata {
        eol_date: lifecycle.eol_date.clone(),
        deprecated,
        latest_stable: lifecycle.latest_patch.clone(),
        lts: lifecycle.lts,
    })
}

/// Map concept ID to a registry family name.
fn concept_to_family(concept_id: &str) -> String {
    match concept_id {
        "node-version" => "node".into(),
        "python-version" => "python".into(),
        "go-version" => "go".into(),
        "java-version" => "java".into(),
        "ruby-version" => "ruby".into(),
        "dotnet-version" => "dotnet".into(),
        other => other.trim_end_matches("-version").to_string(),
    }
}

/// Extract the version cycle identifier from an assertion's value.
fn extract_cycle_from_assertion(assertion: &ConfigAssertion) -> String {
    match &assertion.value {
        SemanticType::Version(VersionSpec::Exact(v)) => {
            format!("{}", v.major)
        }
        SemanticType::Version(VersionSpec::Partial { major, .. }) => {
            format!("{}", major)
        }
        SemanticType::Version(VersionSpec::DockerTag { version, .. }) => {
            version.split('.').next().unwrap_or(version).to_string()
        }
        _ => {
            // Try to extract major version from raw value
            let raw = assertion.raw_value.trim();
            raw.split('.').next().unwrap_or(raw).to_string()
        }
    }
}

/// Check EOL-related policies and produce findings.
fn check_eol_policies(
    findings: &mut Vec<Finding>,
    assertion: &ConfigAssertion,
    config: &ConflicConfig,
    eol_date: &str,
    days_until: Option<i64>,
    lifecycle: &VersionLifecycle,
) {
    for policy in &config.policy {
        if !crate::config::concept_matches_selector(&assertion.concept.id, &policy.concept) {
            continue;
        }

        // Check if policy rule is "eol-window >= N"
        if let Some(window_days) = parse_eol_window_rule(&policy.rule) {
            if let Some(days) = days_until {
                if days <= window_days as i64 {
                    let severity = match policy.severity.to_lowercase().as_str() {
                        "error" => Severity::Error,
                        "info" => Severity::Info,
                        _ => Severity::Warning,
                    };

                    let message = if days <= 0 {
                        format!(
                            "\"{}\" has reached end-of-life (EOL: {})",
                            assertion.raw_value, eol_date,
                        )
                    } else {
                        format!(
                            "\"{}\" reaches end-of-life in {} days (EOL: {})",
                            assertion.raw_value, days, eol_date,
                        )
                    };

                    let enriched_message = if let Some(ref msg) = policy.message {
                        format!("{}: {}", message, msg)
                    } else {
                        message
                    };

                    let latest_info = lifecycle
                        .latest_patch
                        .as_deref()
                        .unwrap_or("unknown");

                    let policy_assertion = ConfigAssertion {
                        concept: assertion.concept.clone(),
                        value: SemanticType::StringValue(format!(
                            "EOL: {}, latest: {}",
                            eol_date, latest_info
                        )),
                        raw_value: policy.rule.clone(),
                        source: assertion::SourceLocation {
                            file: std::path::PathBuf::from(".conflic-registry-cache.json"),
                            line: 0,
                            column: 0,
                            key_path: format!("eol.{}", assertion.concept.id),
                        },
                        span: None,
                        authority: Authority::Enforced,
                        extractor_id: "registry-enrichment".into(),
                        is_matrix: false,
                    };

                    findings.push(Finding {
                        severity,
                        left: assertion.clone(),
                        right: policy_assertion,
                        explanation: enriched_message,
                        rule_id: policy.id.clone(),
                    });
                }
            }
        }
    }
}

/// Parse "eol-window >= N" rule syntax. Returns the number of days.
fn parse_eol_window_rule(rule: &str) -> Option<u32> {
    let trimmed = rule.trim();
    let rest = trimmed.strip_prefix("eol-window")?;
    let rest = rest.trim();
    let rest = rest.strip_prefix(">=")?;
    let rest = rest.trim();
    rest.parse::<u32>().ok()
}

/// Compute the number of days between two ISO dates (YYYY-MM-DD).
/// Returns None if either date cannot be parsed.
/// Positive means `to` is in the future, negative means in the past.
fn days_between(from: &str, to: &str) -> Option<i64> {
    let from_days = parse_date_to_days(from)?;
    let to_days = parse_date_to_days(to)?;
    Some(to_days - from_days)
}

/// Parse an ISO date string to days since epoch (approximate, for comparison).
fn parse_date_to_days(date: &str) -> Option<i64> {
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let year: i64 = parts[0].parse().ok()?;
    let month: i64 = parts[1].parse().ok()?;
    let day: i64 = parts[2].parse().ok()?;

    // Approximate days since epoch (good enough for window comparison)
    Some(year * 365 + month * 30 + day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConflicConfig, PolicyConfig};
    use crate::model::assertion::{Authority, ConfigAssertion, SourceLocation};
    use crate::model::concept::{ConceptCategory, SemanticConcept};
    use crate::model::semantic_type::{SemanticType, parse_version};
    use std::path::PathBuf;

    fn make_assertion(raw: &str) -> ConfigAssertion {
        let parsed = parse_version(raw);
        ConfigAssertion {
            concept: SemanticConcept {
                id: "node-version".into(),
                display_name: "Node.js Version".into(),
                category: ConceptCategory::RuntimeVersion,
            },
            value: SemanticType::Version(parsed),
            raw_value: raw.into(),
            source: SourceLocation {
                file: PathBuf::from(".nvmrc"),
                line: 1,
                column: 0,
                key_path: "".into(),
            },
            span: None,
            authority: Authority::Declared,
            extractor_id: "test".into(),
            is_matrix: false,
        }
    }

    fn make_registry() -> RegistryDb {
        let mut db = RegistryDb::default();
        db.families.insert(
            "node".into(),
            vec![
                VersionLifecycle {
                    cycle: "18".into(),
                    release_date: Some("2022-04-19".into()),
                    eol_date: Some("2025-04-30".into()),
                    lts: true,
                    latest_patch: Some("18.20.4".into()),
                },
                VersionLifecycle {
                    cycle: "20".into(),
                    release_date: Some("2023-04-18".into()),
                    eol_date: Some("2026-04-30".into()),
                    lts: true,
                    latest_patch: Some("20.18.0".into()),
                },
            ],
        );
        db
    }

    #[test]
    fn test_lookup_metadata_found() {
        let a = make_assertion("18.20.0");
        let db = make_registry();

        let meta = lookup_metadata(&a, &db, "2025-03-01").unwrap();
        assert_eq!(meta.eol_date.as_deref(), Some("2025-04-30"));
        assert!(!meta.deprecated);
        assert!(meta.lts);
    }

    #[test]
    fn test_lookup_metadata_deprecated() {
        let a = make_assertion("18.20.0");
        let db = make_registry();

        let meta = lookup_metadata(&a, &db, "2025-05-01").unwrap();
        assert!(meta.deprecated);
    }

    #[test]
    fn test_eol_window_policy() {
        let a = make_assertion("18");
        let db = make_registry();

        let mut results = vec![ConceptResult {
            concept: a.concept.clone(),
            assertions: vec![a],
            findings: vec![],
        }];

        let mut config = ConflicConfig::default();
        config.policy.push(PolicyConfig {
            id: "EOL001".into(),
            concept: "node-version".into(),
            rule: "eol-window >= 90".into(),
            severity: "warning".into(),
            message: Some("Upgrade to a supported LTS version".into()),
        });

        annotate_assertions(&mut results, &db, &config, "2025-03-01");

        assert!(
            !results[0].findings.is_empty(),
            "Should produce EOL warning when within window"
        );
        assert_eq!(results[0].findings[0].rule_id, "EOL001");
    }

    #[test]
    fn test_eol_window_policy_not_triggered_when_far() {
        let a = make_assertion("20");
        let db = make_registry();

        let mut results = vec![ConceptResult {
            concept: a.concept.clone(),
            assertions: vec![a],
            findings: vec![],
        }];

        let mut config = ConflicConfig::default();
        config.policy.push(PolicyConfig {
            id: "EOL001".into(),
            concept: "node-version".into(),
            rule: "eol-window >= 90".into(),
            severity: "warning".into(),
            message: None,
        });

        annotate_assertions(&mut results, &db, &config, "2025-03-01");

        assert!(
            results[0].findings.is_empty(),
            "Node 20 EOL is far away, should not trigger"
        );
    }

    #[test]
    fn test_parse_eol_window_rule() {
        assert_eq!(parse_eol_window_rule("eol-window >= 90"), Some(90));
        assert_eq!(parse_eol_window_rule("eol-window >= 30"), Some(30));
        assert_eq!(parse_eol_window_rule(">= 20"), None);
        assert_eq!(parse_eol_window_rule("eol-window"), None);
    }

    #[test]
    fn test_days_between() {
        assert!(days_between("2025-01-01", "2025-01-31").unwrap() > 0);
        assert!(days_between("2025-06-01", "2025-01-01").unwrap() < 0);
    }

    #[test]
    fn test_concept_to_family() {
        assert_eq!(concept_to_family("node-version"), "node");
        assert_eq!(concept_to_family("python-version"), "python");
        assert_eq!(concept_to_family("custom-version"), "custom");
    }
}
