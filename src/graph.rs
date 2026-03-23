use crate::config::{ConceptRuleConfig, ConflicConfig};
use crate::model::*;

/// Evaluate cross-concept dependency rules against scan results.
///
/// For each `[[concept_rule]]`, if any assertion in the "when" concept matches
/// the trigger condition, all assertions in the "then" concept are checked
/// against the required constraint. Violations produce findings attached to
/// the "then" concept's result.
pub fn evaluate_concept_rules(concept_results: &mut [ConceptResult], config: &ConflicConfig) {
    if config.concept_rule.is_empty() {
        return;
    }

    let config_path = config
        .resolved_config_path(std::path::Path::new("."))
        .to_path_buf();

    for rule in &config.concept_rule {
        let severity = parse_severity(&rule.severity);
        let trigger = match parse_trigger(&rule.when.matches) {
            Some(t) => t,
            None => continue,
        };
        let constraint = match parse_constraint(&rule.then.requires) {
            Some(c) => c,
            None => continue,
        };

        // Check if any assertion in the "when" concept matches the trigger
        let when_concept_id = &rule.when.concept;
        let triggered = concept_results.iter().any(|cr| {
            concept_matches(cr, when_concept_id)
                && cr
                    .assertions
                    .iter()
                    .any(|a| trigger_matches(&trigger, &a.raw_value, &a.value))
        });

        if !triggered {
            continue;
        }

        // Find the "then" concept and check its assertions
        let then_concept_id = &rule.then.concept;
        let violations = collect_violations(
            concept_results,
            then_concept_id,
            &constraint,
            rule,
            severity,
            &config_path,
        );

        if violations.is_empty() {
            continue;
        }

        // Append findings to the "then" concept result, or create one
        if let Some(cr) = concept_results
            .iter_mut()
            .find(|cr| concept_matches(cr, then_concept_id))
        {
            cr.findings.extend(violations);
        }
    }
}

fn concept_matches(cr: &ConceptResult, selector: &str) -> bool {
    cr.concept.id == selector || crate::config::concept_matches_selector(&cr.concept.id, selector)
}

/// Trigger types parsed from the "when.matches" field.
enum Trigger {
    /// Semver range trigger (e.g., ">=3.12")
    VersionRange(node_semver::Range),
    /// Exact string match
    ExactMatch(String),
}

/// Constraint types parsed from the "then.requires" field.
enum Constraint {
    /// Semver range constraint (e.g., ">=22.3")
    VersionRange(node_semver::Range),
    /// Exact string constraint
    ExactMatch(String),
}

fn parse_trigger(matches_str: &str) -> Option<Trigger> {
    let trimmed = matches_str.trim();
    // Try as semver range first
    if let Ok(range) = node_semver::Range::parse(trimmed) {
        return Some(Trigger::VersionRange(range));
    }
    // Fall back to exact string
    Some(Trigger::ExactMatch(trimmed.to_string()))
}

fn parse_constraint(requires_str: &str) -> Option<Constraint> {
    let trimmed = requires_str.trim();
    if let Ok(range) = node_semver::Range::parse(trimmed) {
        return Some(Constraint::VersionRange(range));
    }
    Some(Constraint::ExactMatch(trimmed.to_string()))
}

fn trigger_matches(trigger: &Trigger, raw_value: &str, typed_value: &SemanticType) -> bool {
    match trigger {
        Trigger::VersionRange(range) => {
            if let Some(version) = extract_node_version(raw_value, typed_value) {
                range.satisfies(&version)
            } else {
                false
            }
        }
        Trigger::ExactMatch(expected) => raw_value.trim() == expected,
    }
}

fn constraint_satisfied(
    constraint: &Constraint,
    raw_value: &str,
    typed_value: &SemanticType,
) -> bool {
    match constraint {
        Constraint::VersionRange(range) => {
            if let Some(version) = extract_node_version(raw_value, typed_value) {
                range.satisfies(&version)
            } else {
                // If we can't parse the version, we can't verify — treat as satisfied
                true
            }
        }
        Constraint::ExactMatch(expected) => raw_value.trim() == expected,
    }
}

fn extract_node_version(
    raw_value: &str,
    typed_value: &SemanticType,
) -> Option<node_semver::Version> {
    match typed_value {
        SemanticType::Version(VersionSpec::Exact(v)) => {
            Some(crate::solve::version::to_node_version_pub(v))
        }
        SemanticType::Version(VersionSpec::Partial { major, minor }) => {
            let minor = minor.unwrap_or(0);
            Some(node_semver::Version::from((*major, minor, 0_u64)))
        }
        SemanticType::Version(VersionSpec::DockerTag { version, .. }) => {
            extract_version_from_raw(version)
        }
        SemanticType::Version(VersionSpec::Range(_)) => {
            // For ranges parsed from simple values like "20" or "3.12",
            // extract the testable version from the raw string.
            extract_version_from_raw(raw_value)
        }
        _ => extract_version_from_raw(raw_value),
    }
}

/// Try to extract a testable version from a raw string like "18", "3.12", or "20.11.0".
fn extract_version_from_raw(raw: &str) -> Option<node_semver::Version> {
    let trimmed = raw.trim();
    // Try exact semver first
    if let Ok(v) = semver::Version::parse(trimmed) {
        return Some(crate::solve::version::to_node_version_pub(&v));
    }
    // Try as partial (major or major.minor)
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

fn collect_violations(
    concept_results: &[ConceptResult],
    then_concept_id: &str,
    constraint: &Constraint,
    rule: &ConceptRuleConfig,
    severity: Severity,
    config_path: &std::path::Path,
) -> Vec<Finding> {
    let then_result = concept_results
        .iter()
        .find(|cr| concept_matches(cr, then_concept_id));

    let Some(then_result) = then_result else {
        return Vec::new();
    };

    let mut findings = Vec::new();
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();

    for assertion in &then_result.assertions {
        if constraint_satisfied(constraint, &assertion.raw_value, &assertion.value) {
            continue;
        }

        let file_key = crate::pathing::normalize_path(&assertion.source.file)
            .to_string_lossy()
            .into_owned();
        let dedup_key = (file_key, assertion.raw_value.clone());
        if !seen.insert(dedup_key) {
            continue;
        }

        let explanation = if let Some(ref msg) = rule.message {
            format!(
                "\"{}\" violates cross-concept constraint \"{}\": {}",
                assertion.raw_value, rule.then.requires, msg
            )
        } else {
            format!(
                "\"{}\" violates cross-concept constraint \"{}\"",
                assertion.raw_value, rule.then.requires
            )
        };

        let rule_assertion = ConfigAssertion {
            concept: assertion.concept.clone(),
            value: SemanticType::StringValue(rule.then.requires.clone()),
            raw_value: rule.then.requires.clone(),
            source: SourceLocation {
                file: config_path.to_path_buf(),
                line: 0,
                column: 0,
                key_path: format!("concept_rule.{}", rule.id),
            },
            span: None,
            authority: Authority::Enforced,
            extractor_id: "concept-rule".into(),
            is_matrix: false,
        };

        findings.push(Finding {
            severity,
            left: assertion.clone(),
            right: rule_assertion,
            explanation,
            rule_id: rule.id.clone(),
        });
    }

    findings
}

fn parse_severity(severity: &str) -> Severity {
    match severity.trim().to_ascii_lowercase().as_str() {
        "error" => Severity::Error,
        "warning" => Severity::Warning,
        "info" => Severity::Info,
        _ => Severity::Error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConceptRuleConfig, ConceptRuleThen, ConceptRuleWhen, ConflicConfig};
    use crate::model::assertion::{Authority, ConfigAssertion, SourceLocation};
    use crate::model::concept::{ConceptCategory, SemanticConcept};
    use std::path::PathBuf;

    fn make_assertion(
        concept_id: &str,
        display_name: &str,
        file: &str,
        raw: &str,
    ) -> ConfigAssertion {
        let parsed = parse_version(raw);
        ConfigAssertion {
            concept: SemanticConcept {
                id: concept_id.into(),
                display_name: display_name.into(),
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

    fn make_rule(
        id: &str,
        when_concept: &str,
        when_matches: &str,
        then_concept: &str,
        then_requires: &str,
        severity: &str,
        message: Option<&str>,
    ) -> ConceptRuleConfig {
        ConceptRuleConfig {
            id: id.into(),
            when: ConceptRuleWhen {
                concept: when_concept.into(),
                matches: when_matches.into(),
            },
            then: ConceptRuleThen {
                concept: then_concept.into(),
                requires: then_requires.into(),
            },
            severity: severity.into(),
            message: message.map(String::from),
        }
    }

    #[test]
    fn test_cross_concept_rule_triggers_violation() {
        // Python 3.12 requires pip >= 22.3
        let python = make_assertion(
            "python-version",
            "Python Version",
            ".python-version",
            "3.12",
        );
        let pip = make_assertion("pip-version", "pip Version", "requirements.txt", "21.0.0");

        let mut results = vec![
            ConceptResult {
                concept: python.concept.clone(),
                assertions: vec![python],
                findings: vec![],
            },
            ConceptResult {
                concept: pip.concept.clone(),
                assertions: vec![pip],
                findings: vec![],
            },
        ];

        let mut config = ConflicConfig::default();
        config.concept_rule.push(make_rule(
            "XCON001",
            "python-version",
            ">=3.12",
            "pip-version",
            ">=22.3",
            "warning",
            Some("Python 3.12 requires pip 22.3+"),
        ));

        evaluate_concept_rules(&mut results, &config);

        let pip_result = results
            .iter()
            .find(|cr| cr.concept.id == "pip-version")
            .unwrap();
        assert_eq!(pip_result.findings.len(), 1);
        assert_eq!(pip_result.findings[0].rule_id, "XCON001");
        assert_eq!(pip_result.findings[0].severity, Severity::Warning);
        assert!(
            pip_result.findings[0]
                .explanation
                .contains("Python 3.12 requires pip 22.3+")
        );
    }

    #[test]
    fn test_cross_concept_rule_no_violation_when_satisfied() {
        let python = make_assertion(
            "python-version",
            "Python Version",
            ".python-version",
            "3.12",
        );
        let pip = make_assertion("pip-version", "pip Version", "requirements.txt", "23.0.0");

        let mut results = vec![
            ConceptResult {
                concept: python.concept.clone(),
                assertions: vec![python],
                findings: vec![],
            },
            ConceptResult {
                concept: pip.concept.clone(),
                assertions: vec![pip],
                findings: vec![],
            },
        ];

        let mut config = ConflicConfig::default();
        config.concept_rule.push(make_rule(
            "XCON001",
            "python-version",
            ">=3.12",
            "pip-version",
            ">=22.3",
            "warning",
            None,
        ));

        evaluate_concept_rules(&mut results, &config);

        let pip_result = results
            .iter()
            .find(|cr| cr.concept.id == "pip-version")
            .unwrap();
        assert!(pip_result.findings.is_empty());
    }

    #[test]
    fn test_cross_concept_rule_not_triggered_when_condition_not_met() {
        // Python 3.10 should NOT trigger the rule for >=3.12
        let python = make_assertion(
            "python-version",
            "Python Version",
            ".python-version",
            "3.10",
        );
        let pip = make_assertion("pip-version", "pip Version", "requirements.txt", "21.0.0");

        let mut results = vec![
            ConceptResult {
                concept: python.concept.clone(),
                assertions: vec![python],
                findings: vec![],
            },
            ConceptResult {
                concept: pip.concept.clone(),
                assertions: vec![pip],
                findings: vec![],
            },
        ];

        let mut config = ConflicConfig::default();
        config.concept_rule.push(make_rule(
            "XCON001",
            "python-version",
            ">=3.12",
            "pip-version",
            ">=22.3",
            "warning",
            None,
        ));

        evaluate_concept_rules(&mut results, &config);

        let pip_result = results
            .iter()
            .find(|cr| cr.concept.id == "pip-version")
            .unwrap();
        assert!(
            pip_result.findings.is_empty(),
            "Rule should not trigger for Python 3.10"
        );
    }

    #[test]
    fn test_cross_concept_rule_with_alias() {
        let python = make_assertion(
            "python-version",
            "Python Version",
            ".python-version",
            "3.12",
        );
        let pip = make_assertion("pip-version", "pip Version", "requirements.txt", "21.0.0");

        let mut results = vec![
            ConceptResult {
                concept: python.concept.clone(),
                assertions: vec![python],
                findings: vec![],
            },
            ConceptResult {
                concept: pip.concept.clone(),
                assertions: vec![pip],
                findings: vec![],
            },
        ];

        let mut config = ConflicConfig::default();
        // Use alias "python" instead of "python-version"
        config.concept_rule.push(make_rule(
            "XCON001",
            "python",
            ">=3.12",
            "pip-version",
            ">=22.3",
            "error",
            None,
        ));

        evaluate_concept_rules(&mut results, &config);

        let pip_result = results
            .iter()
            .find(|cr| cr.concept.id == "pip-version")
            .unwrap();
        assert_eq!(pip_result.findings.len(), 1, "Should trigger via alias");
    }

    #[test]
    fn test_cross_concept_rule_deduplicates_same_file_value() {
        let python = make_assertion(
            "python-version",
            "Python Version",
            ".python-version",
            "3.12",
        );
        let mut pip1 = make_assertion("pip-version", "pip Version", "req.txt", "21.0.0");
        let mut pip2 = make_assertion("pip-version", "pip Version", "req.txt", "21.0.0");
        pip2.source.line = 5;
        // Make sure pip1 and pip2 have same normalized file key
        pip1.source.file = PathBuf::from("req.txt");
        pip2.source.file = PathBuf::from("req.txt");

        let mut results = vec![
            ConceptResult {
                concept: python.concept.clone(),
                assertions: vec![python],
                findings: vec![],
            },
            ConceptResult {
                concept: pip1.concept.clone(),
                assertions: vec![pip1, pip2],
                findings: vec![],
            },
        ];

        let mut config = ConflicConfig::default();
        config.concept_rule.push(make_rule(
            "XCON001",
            "python-version",
            ">=3.12",
            "pip-version",
            ">=22.3",
            "warning",
            None,
        ));

        evaluate_concept_rules(&mut results, &config);

        let pip_result = results
            .iter()
            .find(|cr| cr.concept.id == "pip-version")
            .unwrap();
        assert_eq!(
            pip_result.findings.len(),
            1,
            "Should deduplicate same file+value"
        );
    }

    #[test]
    fn test_cross_concept_rule_missing_then_concept_no_panic() {
        let python = make_assertion(
            "python-version",
            "Python Version",
            ".python-version",
            "3.12",
        );

        let mut results = vec![ConceptResult {
            concept: python.concept.clone(),
            assertions: vec![python],
            findings: vec![],
        }];

        let mut config = ConflicConfig::default();
        config.concept_rule.push(make_rule(
            "XCON001",
            "python-version",
            ">=3.12",
            "nonexistent-concept",
            ">=1.0",
            "warning",
            None,
        ));

        // Should not panic
        evaluate_concept_rules(&mut results, &config);
    }

    #[test]
    fn test_cross_concept_exact_match_trigger() {
        let node = make_assertion("node-version", "Node.js Version", ".nvmrc", "20");
        let npm = make_assertion("npm-version", "npm Version", "package.json", "6.0.0");

        let mut results = vec![
            ConceptResult {
                concept: node.concept.clone(),
                assertions: vec![node],
                findings: vec![],
            },
            ConceptResult {
                concept: npm.concept.clone(),
                assertions: vec![npm],
                findings: vec![],
            },
        ];

        let mut config = ConflicConfig::default();
        // Exact match: when node-version is "20", npm must be >= 8
        config.concept_rule.push(make_rule(
            "XCON002",
            "node-version",
            ">=20",
            "npm-version",
            ">=8",
            "error",
            Some("Node 20 requires npm 8+"),
        ));

        evaluate_concept_rules(&mut results, &config);

        let npm_result = results
            .iter()
            .find(|cr| cr.concept.id == "npm-version")
            .unwrap();
        assert_eq!(npm_result.findings.len(), 1);
        assert_eq!(npm_result.findings[0].severity, Severity::Error);
    }
}
