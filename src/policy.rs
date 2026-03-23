use crate::config::{ConflicConfig, PolicyConfig};
use crate::model::*;

/// Evaluate all policies against extracted assertions and produce policy violation findings.
///
/// For each policy, every assertion matching the policy's concept is checked against the
/// policy's rule constraint. When an assertion violates a policy, a `Finding` is generated
/// with the assertion on the left side and a synthetic policy assertion on the right side.
pub fn evaluate_policies(concept_results: &mut [ConceptResult], config: &ConflicConfig) {
    if config.policy.is_empty() {
        return;
    }

    let config_path = config
        .resolved_config_path(std::path::Path::new("."))
        .to_path_buf();

    for policy in &config.policy {
        let severity = parse_policy_severity(&policy.severity);
        let checker = match PolicyChecker::from_policy(policy) {
            Some(c) => c,
            None => continue,
        };

        // Find or create the concept result for this policy's concept
        let concept_result = concept_results
            .iter_mut()
            .find(|cr| concept_matches_policy(&cr.concept.id, &policy.concept));

        if let Some(result) = concept_result {
            let violations = evaluate_policy_against_assertions(
                &result.assertions,
                policy,
                &checker,
                severity,
                &config_path,
            );
            result.findings.extend(violations);
        }
    }
}

fn concept_matches_policy(concept_id: &str, policy_concept: &str) -> bool {
    if concept_id == policy_concept {
        return true;
    }
    // Support the same shorthand aliases as skip_concepts
    crate::config::concept_matches_selector(concept_id, policy_concept)
}

fn evaluate_policy_against_assertions(
    assertions: &[ConfigAssertion],
    policy: &PolicyConfig,
    checker: &PolicyChecker,
    severity: Severity,
    config_path: &std::path::Path,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    // Deduplicate by (file, raw_value) to avoid redundant policy findings
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();

    for assertion in assertions {
        let file_key = crate::pathing::normalize_path(&assertion.source.file)
            .to_string_lossy()
            .into_owned();
        let dedup_key = (file_key, assertion.raw_value.clone());
        if !seen.insert(dedup_key) {
            continue;
        }

        if let Some(explanation) = checker.check_violation(&assertion.raw_value, &assertion.value) {
            let message = if let Some(ref msg) = policy.message {
                format!("{}: {}", explanation, msg)
            } else {
                explanation
            };

            let policy_assertion = ConfigAssertion {
                concept: assertion.concept.clone(),
                value: SemanticType::StringValue(policy.rule.clone()),
                raw_value: policy.rule.clone(),
                source: SourceLocation {
                    file: config_path.to_path_buf(),
                    line: 0,
                    column: 0,
                    key_path: format!("policy.{}", policy.id),
                },
                span: None,
                authority: Authority::Enforced,
                extractor_id: "policy".into(),
                is_matrix: false,
            };

            findings.push(Finding {
                severity,
                left: assertion.clone(),
                right: policy_assertion,
                explanation: message,
                rule_id: policy.id.clone(),
            });
        }
    }

    findings
}

fn parse_policy_severity(severity: &str) -> Severity {
    match severity.trim().to_ascii_lowercase().as_str() {
        "error" => Severity::Error,
        "warning" => Severity::Warning,
        "info" => Severity::Info,
        _ => Severity::Error,
    }
}

/// Internal checker that encapsulates the parsed policy rule.
enum PolicyChecker {
    /// Version constraint: assertion must satisfy this range.
    VersionRange(node_semver::Range),
    /// Port blacklist: assertion port must NOT be any of these.
    PortBlacklist(Vec<u16>),
    /// Port range requirement: assertion port must be within this range.
    PortRange { min: u16, max: u16 },
    /// Boolean requirement: assertion must match this value.
    BooleanEquals(bool),
    /// String blacklist: assertion value must NOT be any of these.
    StringBlacklist(Vec<String>),
    /// String whitelist: assertion value must be one of these.
    StringWhitelist(Vec<String>),
}

impl PolicyChecker {
    /// Parse a policy config's rule string into a checker.
    fn from_policy(policy: &PolicyConfig) -> Option<Self> {
        let rule = policy.rule.trim();

        // Try to detect the rule type from the concept name and rule syntax
        if is_version_concept(&policy.concept) {
            return Self::parse_version_rule(rule);
        }
        if is_port_concept(&policy.concept) {
            return Self::parse_port_rule(rule);
        }
        if is_boolean_concept(&policy.concept) {
            return Self::parse_boolean_rule(rule);
        }

        // Fallback: try version range first, then string rules
        if let Some(checker) = Self::parse_version_rule(rule) {
            return Some(checker);
        }
        Self::parse_string_rule(rule)
    }

    fn parse_version_rule(rule: &str) -> Option<Self> {
        // Handle blacklist syntax: "!= 3.8, != 3.9"
        if rule.contains("!=") {
            let values: Vec<String> = rule
                .split(',')
                .map(|s| s.trim().trim_start_matches("!=").trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !values.is_empty() {
                return Some(PolicyChecker::StringBlacklist(values));
            }
        }

        // Try as semver range
        node_semver::Range::parse(rule)
            .ok()
            .map(PolicyChecker::VersionRange)
    }

    fn parse_port_rule(rule: &str) -> Option<Self> {
        // Handle "!= 80" or "!= 80, != 443"
        if rule.contains("!=") {
            let ports: Vec<u16> = rule
                .split(',')
                .filter_map(|s| s.trim().trim_start_matches("!=").trim().parse::<u16>().ok())
                .collect();
            if !ports.is_empty() {
                return Some(PolicyChecker::PortBlacklist(ports));
            }
        }

        // Handle ">= 1024" as a range requirement
        let trimmed = rule.trim();
        if let Some(rest) = trimmed.strip_prefix(">=")
            && let Ok(min) = rest.trim().parse::<u16>()
        {
            return Some(PolicyChecker::PortRange { min, max: u16::MAX });
        }
        if let Some(rest) = trimmed.strip_prefix(">")
            && let Ok(min) = rest.trim().parse::<u16>()
        {
            return Some(PolicyChecker::PortRange {
                min: min.saturating_add(1),
                max: u16::MAX,
            });
        }

        None
    }

    fn parse_boolean_rule(rule: &str) -> Option<Self> {
        normalize_boolean(rule).map(PolicyChecker::BooleanEquals)
    }

    fn parse_string_rule(rule: &str) -> Option<Self> {
        if rule.contains("!=") {
            let values: Vec<String> = rule
                .split(',')
                .map(|s| s.trim().trim_start_matches("!=").trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !values.is_empty() {
                return Some(PolicyChecker::StringBlacklist(values));
            }
        }

        // Whitelist: comma-separated values
        let values: Vec<String> = rule
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !values.is_empty() {
            return Some(PolicyChecker::StringWhitelist(values));
        }

        None
    }

    /// Check if a value violates this policy. Returns an explanation string if violated.
    fn check_violation(&self, raw_value: &str, typed_value: &SemanticType) -> Option<String> {
        match self {
            PolicyChecker::VersionRange(range) => {
                check_version_against_range(raw_value, typed_value, range)
            }
            PolicyChecker::PortBlacklist(blocked_ports) => {
                check_port_blacklist(raw_value, typed_value, blocked_ports)
            }
            PolicyChecker::PortRange { min, max } => {
                check_port_range(raw_value, typed_value, *min, *max)
            }
            PolicyChecker::BooleanEquals(expected) => {
                check_boolean_equals(raw_value, typed_value, *expected)
            }
            PolicyChecker::StringBlacklist(blocked) => check_string_blacklist(raw_value, blocked),
            PolicyChecker::StringWhitelist(allowed) => check_string_whitelist(raw_value, allowed),
        }
    }
}

fn check_version_against_range(
    raw_value: &str,
    typed_value: &SemanticType,
    range: &node_semver::Range,
) -> Option<String> {
    // Extract a version to test against the range
    let version_to_check = match typed_value {
        SemanticType::Version(VersionSpec::Exact(v)) => {
            Some(crate::solve::version::to_node_version_pub(v))
        }
        SemanticType::Version(VersionSpec::Partial { major, minor }) => {
            let minor = minor.unwrap_or(0);
            Some(node_semver::Version::from((*major, minor, 0_u64)))
        }
        SemanticType::Version(VersionSpec::DockerTag { version, .. }) => {
            // Re-parse the version part
            let reparsed = parse_version(version);
            match reparsed {
                VersionSpec::Exact(v) => Some(crate::solve::version::to_node_version_pub(&v)),
                VersionSpec::Partial { major, minor } => {
                    let minor = minor.unwrap_or(0);
                    Some(node_semver::Version::from((major, minor, 0_u64)))
                }
                _ => None,
            }
        }
        SemanticType::Version(VersionSpec::Range(_)) => {
            // For range assertions (e.g., "18" parsed as >=18.0.0 <19.0.0),
            // fall through to raw_value parsing to extract a testable version.
            let parsed = parse_version(raw_value);
            match parsed {
                VersionSpec::Exact(v) => Some(crate::solve::version::to_node_version_pub(&v)),
                _ => {
                    // Try to extract a version from the raw string directly
                    // (e.g., "18" → 18.0.0)
                    extract_version_from_raw(raw_value)
                }
            }
        }
        _ => {
            // Try to parse the raw value as a version
            let parsed = parse_version(raw_value);
            match parsed {
                VersionSpec::Exact(v) => Some(crate::solve::version::to_node_version_pub(&v)),
                VersionSpec::Partial { major, minor } => {
                    let minor = minor.unwrap_or(0);
                    Some(node_semver::Version::from((major, minor, 0_u64)))
                }
                _ => extract_version_from_raw(raw_value),
            }
        }
    };

    let version = version_to_check?;
    if range.satisfies(&version) {
        None // Compliant
    } else {
        Some(format!(
            "\"{}\" violates policy constraint \"{}\"",
            raw_value, range
        ))
    }
}

/// Try to extract a testable version from a raw string like "18" or "3.12".
fn extract_version_from_raw(raw: &str) -> Option<node_semver::Version> {
    let trimmed = raw.trim();
    let parts: Vec<&str> = trimmed.split('.').collect();
    match parts.len() {
        1 => {
            let major = parts[0].parse::<u64>().ok()?;
            Some(node_semver::Version::from((major, 0_u64, 0_u64)))
        }
        2 => {
            let major = parts[0].parse::<u64>().ok()?;
            let minor = parts[1].parse::<u64>().ok()?;
            Some(node_semver::Version::from((major, minor, 0_u64)))
        }
        _ => None,
    }
}

fn check_port_blacklist(
    raw_value: &str,
    typed_value: &SemanticType,
    blocked_ports: &[u16],
) -> Option<String> {
    let port = match typed_value {
        SemanticType::Port(PortSpec::Single(p)) => Some(*p),
        SemanticType::Port(PortSpec::Mapping { container, .. }) => Some(*container),
        _ => raw_value.trim().parse::<u16>().ok(),
    };

    let port = port?;
    if blocked_ports.contains(&port) {
        Some(format!("port {} is prohibited by policy", port))
    } else {
        None
    }
}

fn check_port_range(
    raw_value: &str,
    typed_value: &SemanticType,
    min: u16,
    max: u16,
) -> Option<String> {
    let port = match typed_value {
        SemanticType::Port(PortSpec::Single(p)) => Some(*p),
        SemanticType::Port(PortSpec::Mapping { container, .. }) => Some(*container),
        _ => raw_value.trim().parse::<u16>().ok(),
    };

    let port = port?;
    if port >= min && port <= max {
        None // Compliant
    } else {
        Some(format!(
            "port {} is outside allowed range {}-{}",
            port, min, max
        ))
    }
}

fn check_boolean_equals(
    raw_value: &str,
    typed_value: &SemanticType,
    expected: bool,
) -> Option<String> {
    let actual = match typed_value {
        SemanticType::Boolean(b) => Some(*b),
        _ => normalize_boolean(raw_value),
    };

    let actual = actual?;
    if actual == expected {
        None
    } else {
        Some(format!(
            "\"{}\" violates policy (expected {})",
            raw_value, expected
        ))
    }
}

fn check_string_blacklist(raw_value: &str, blocked: &[String]) -> Option<String> {
    let trimmed = raw_value.trim();
    for blocked_val in blocked {
        if trimmed == blocked_val {
            return Some(format!("\"{}\" is prohibited by policy", trimmed));
        }
        if version_matches_blacklist_entry(trimmed, blocked_val) {
            return Some(format!(
                "\"{}\" matches prohibited version \"{}\"",
                trimmed, blocked_val
            ));
        }
    }
    None
}

/// Check if an actual version string matches a blacklisted version pattern.
/// Handles cases like "3.8.1" matching blacklisted "3.8" by comparing
/// the major.minor prefix.
fn version_matches_blacklist_entry(actual: &str, blocked: &str) -> bool {
    let blocked_parts: Vec<&str> = blocked.split('.').collect();
    let actual_parts: Vec<&str> = actual.split('.').collect();

    // Both must start with digits
    if blocked_parts.is_empty()
        || actual_parts.is_empty()
        || blocked_parts[0].parse::<u64>().is_err()
        || actual_parts[0].parse::<u64>().is_err()
    {
        return false;
    }

    // The actual version must have at least as many parts as the blocked pattern
    if actual_parts.len() < blocked_parts.len() {
        return false;
    }

    // Compare each component of the blocked pattern against the actual value
    for (blocked_part, actual_part) in blocked_parts.iter().zip(actual_parts.iter()) {
        // Strip any pre-release suffix from actual part for comparison
        let actual_numeric = actual_part.split('-').next().unwrap_or(actual_part);
        match (blocked_part.parse::<u64>(), actual_numeric.parse::<u64>()) {
            (Ok(b), Ok(a)) => {
                if a != b {
                    return false;
                }
            }
            _ => {
                if *blocked_part != *actual_part {
                    return false;
                }
            }
        }
    }

    true
}

fn check_string_whitelist(raw_value: &str, allowed: &[String]) -> Option<String> {
    let trimmed = raw_value.trim();
    if allowed.iter().any(|a| trimmed == a) {
        None
    } else {
        Some(format!(
            "\"{}\" is not in the allowed values: {}",
            trimmed,
            allowed.join(", ")
        ))
    }
}

fn is_version_concept(concept: &str) -> bool {
    concept.ends_with("-version")
        || concept == "node"
        || concept == "python"
        || concept == "ruby"
        || concept == "java"
        || concept == "go"
        || concept == "dotnet"
}

fn is_port_concept(concept: &str) -> bool {
    concept.contains("port")
}

fn is_boolean_concept(concept: &str) -> bool {
    concept.contains("strict") || concept.contains("boolean") || concept.contains("bool")
}

// Re-export for use by version checker
use crate::model::{
    Authority, PortSpec, SemanticType, SourceLocation, VersionSpec, normalize_boolean,
    parse_version,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PolicyConfig;
    use crate::model::concept::{ConceptCategory, SemanticConcept};
    use std::path::PathBuf;

    fn make_version_assertion(file: &str, raw: &str) -> ConfigAssertion {
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
                file: PathBuf::from(file),
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

    fn make_port_assertion(file: &str, port: u16) -> ConfigAssertion {
        ConfigAssertion {
            concept: SemanticConcept {
                id: "app-port".into(),
                display_name: "Application Port".into(),
                category: ConceptCategory::Port,
            },
            value: SemanticType::Port(PortSpec::Single(port)),
            raw_value: port.to_string(),
            source: SourceLocation {
                file: PathBuf::from(file),
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

    fn make_bool_assertion(file: &str, val: bool) -> ConfigAssertion {
        ConfigAssertion {
            concept: SemanticConcept {
                id: "ts-strict-mode".into(),
                display_name: "TypeScript Strict Mode".into(),
                category: ConceptCategory::StrictMode,
            },
            value: SemanticType::Boolean(val),
            raw_value: val.to_string(),
            source: SourceLocation {
                file: PathBuf::from(file),
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

    fn make_policy(id: &str, concept: &str, rule: &str, severity: &str) -> PolicyConfig {
        PolicyConfig {
            id: id.into(),
            concept: concept.into(),
            rule: rule.into(),
            severity: severity.into(),
            message: None,
        }
    }

    #[test]
    fn test_version_policy_compliant() {
        let assertions = vec![make_version_assertion(".nvmrc", "20.11.0")];
        let policy = make_policy("POL001", "node-version", ">= 20", "error");
        let checker = PolicyChecker::from_policy(&policy).unwrap();
        let findings = evaluate_policy_against_assertions(
            &assertions,
            &policy,
            &checker,
            Severity::Error,
            &PathBuf::from(".conflic.toml"),
        );
        assert!(findings.is_empty(), "20.11.0 satisfies >= 20");
    }

    #[test]
    fn test_version_policy_violated() {
        let assertions = vec![make_version_assertion(".nvmrc", "18.0.0")];
        let policy = make_policy("POL001", "node-version", ">= 20", "error");
        let checker = PolicyChecker::from_policy(&policy).unwrap();
        let findings = evaluate_policy_against_assertions(
            &assertions,
            &policy,
            &checker,
            Severity::Error,
            &PathBuf::from(".conflic.toml"),
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "POL001");
        assert_eq!(findings[0].severity, Severity::Error);
    }

    #[test]
    fn test_version_blacklist_policy() {
        let assertions = vec![
            make_version_assertion("a.json", "3.8.1"),
            make_version_assertion("b.json", "3.10.0"),
        ];
        let policy = make_policy("POL002", "python-version", "!= 3.8, != 3.9", "error");

        // Manually set concept to python-version
        let mut assertions = assertions;
        for a in &mut assertions {
            a.concept = SemanticConcept {
                id: "python-version".into(),
                display_name: "Python Version".into(),
                category: ConceptCategory::RuntimeVersion,
            };
        }

        let checker = PolicyChecker::from_policy(&policy).unwrap();
        let findings = evaluate_policy_against_assertions(
            &assertions,
            &policy,
            &checker,
            Severity::Error,
            &PathBuf::from(".conflic.toml"),
        );
        assert_eq!(
            findings.len(),
            1,
            "3.8.1 should match blacklisted 3.8, but 3.10.0 should pass"
        );
    }

    #[test]
    fn test_port_blacklist_policy() {
        let assertions = vec![
            make_port_assertion("a.env", 80),
            make_port_assertion("b.env", 8080),
        ];
        let policy = make_policy("POL003", "app-port", "!= 80, != 443", "warning");
        let checker = PolicyChecker::from_policy(&policy).unwrap();
        let findings = evaluate_policy_against_assertions(
            &assertions,
            &policy,
            &checker,
            Severity::Warning,
            &PathBuf::from(".conflic.toml"),
        );
        assert_eq!(
            findings.len(),
            1,
            "port 80 is blocked, port 8080 is allowed"
        );
        assert_eq!(findings[0].severity, Severity::Warning);
    }

    #[test]
    fn test_port_range_policy() {
        let assertions = vec![
            make_port_assertion("a.env", 80),
            make_port_assertion("b.env", 8080),
        ];
        let policy = make_policy("POL004", "app-port", ">= 1024", "warning");
        let checker = PolicyChecker::from_policy(&policy).unwrap();
        let findings = evaluate_policy_against_assertions(
            &assertions,
            &policy,
            &checker,
            Severity::Warning,
            &PathBuf::from(".conflic.toml"),
        );
        assert_eq!(findings.len(), 1, "port 80 is below 1024");
    }

    #[test]
    fn test_boolean_policy() {
        let assertions = vec![make_bool_assertion("tsconfig.json", false)];
        let policy = make_policy("POL005", "ts-strict-mode", "true", "error");
        let checker = PolicyChecker::from_policy(&policy).unwrap();
        let findings = evaluate_policy_against_assertions(
            &assertions,
            &policy,
            &checker,
            Severity::Error,
            &PathBuf::from(".conflic.toml"),
        );
        assert_eq!(
            findings.len(),
            1,
            "strict mode false violates policy requiring true"
        );
    }

    #[test]
    fn test_boolean_policy_compliant() {
        let assertions = vec![make_bool_assertion("tsconfig.json", true)];
        let policy = make_policy("POL005", "ts-strict-mode", "true", "error");
        let checker = PolicyChecker::from_policy(&policy).unwrap();
        let findings = evaluate_policy_against_assertions(
            &assertions,
            &policy,
            &checker,
            Severity::Error,
            &PathBuf::from(".conflic.toml"),
        );
        assert!(findings.is_empty(), "strict mode true satisfies policy");
    }

    #[test]
    fn test_policy_message_is_included() {
        let assertions = vec![make_version_assertion(".nvmrc", "18.0.0")];
        let mut policy = make_policy("POL001", "node-version", ">= 20", "error");
        policy.message = Some("Node 18 reaches EOL April 2025".into());
        let checker = PolicyChecker::from_policy(&policy).unwrap();
        let findings = evaluate_policy_against_assertions(
            &assertions,
            &policy,
            &checker,
            Severity::Error,
            &PathBuf::from(".conflic.toml"),
        );
        assert!(
            findings[0]
                .explanation
                .contains("Node 18 reaches EOL April 2025"),
            "Policy message should be in explanation: {}",
            findings[0].explanation
        );
    }

    #[test]
    fn test_policy_synthetic_assertion_points_to_config() {
        let assertions = vec![make_version_assertion(".nvmrc", "18.0.0")];
        let policy = make_policy("POL001", "node-version", ">= 20", "error");
        let checker = PolicyChecker::from_policy(&policy).unwrap();
        let config_path = PathBuf::from("/project/.conflic.toml");
        let findings = evaluate_policy_against_assertions(
            &assertions,
            &policy,
            &checker,
            Severity::Error,
            &config_path,
        );
        assert_eq!(findings[0].right.source.file, config_path);
        assert_eq!(findings[0].right.extractor_id, "policy");
        assert_eq!(findings[0].right.authority, Authority::Enforced);
        assert_eq!(findings[0].right.raw_value, ">= 20");
    }

    #[test]
    fn test_concept_alias_matching() {
        // "node" should match "node-version"
        assert!(concept_matches_policy("node-version", "node"));
        assert!(concept_matches_policy("node-version", "node-version"));
        assert!(concept_matches_policy("app-port", "port"));
        assert!(!concept_matches_policy("python-version", "node"));
    }

    #[test]
    fn test_evaluate_policies_integration() {
        let assertion = make_version_assertion(".nvmrc", "18.0.0");
        let mut concept_results = vec![ConceptResult {
            concept: assertion.concept.clone(),
            assertions: vec![assertion],
            findings: vec![],
        }];

        let mut config = ConflicConfig::default();
        config.policy.push(PolicyConfig {
            id: "POL001".into(),
            concept: "node-version".into(),
            rule: ">= 20".into(),
            severity: "error".into(),
            message: Some("Upgrade to Node 20+".into()),
        });

        evaluate_policies(&mut concept_results, &config);

        assert_eq!(concept_results[0].findings.len(), 1);
        assert_eq!(concept_results[0].findings[0].rule_id, "POL001");
    }

    #[test]
    fn test_no_duplicate_findings_for_same_file_value() {
        // Two assertions with same file and value should produce only one policy finding
        let mut assertions = vec![
            make_version_assertion("workflow.yml", "18"),
            make_version_assertion("workflow.yml", "18"),
        ];
        assertions[1].source.line = 5;

        let policy = make_policy("POL001", "node-version", ">= 20", "error");
        let checker = PolicyChecker::from_policy(&policy).unwrap();
        let findings = evaluate_policy_against_assertions(
            &assertions,
            &policy,
            &checker,
            Severity::Error,
            &PathBuf::from(".conflic.toml"),
        );
        assert_eq!(
            findings.len(),
            1,
            "duplicate file+value should be deduplicated"
        );
    }
}
