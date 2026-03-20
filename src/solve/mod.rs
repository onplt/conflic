pub mod boolean;
pub mod port;
pub mod severity;
pub mod string;
pub mod version;

use std::collections::HashMap;

use crate::config::ConflicConfig;
use crate::model::*;

/// Compare all assertions grouped by concept and produce findings.
pub fn compare_assertions(
    assertions: Vec<ConfigAssertion>,
    config: &ConflicConfig,
) -> Vec<ConceptResult> {
    // Group by concept ID
    let mut groups: HashMap<String, Vec<ConfigAssertion>> = HashMap::new();
    for assertion in assertions {
        groups
            .entry(assertion.concept.id.clone())
            .or_default()
            .push(assertion);
    }

    let mut results = Vec::new();

    let use_monorepo = config.monorepo.per_package && !config.monorepo.package_roots.is_empty();

    for (concept_id, group) in groups {
        if config.should_skip_concept(&concept_id) {
            continue;
        }

        if group.len() < 2 {
            results.push(ConceptResult {
                concept: group[0].concept.clone(),
                assertions: group,
                findings: vec![],
            });
            continue;
        }

        let concept = group[0].concept.clone();

        let findings = if use_monorepo
            && !config.monorepo.global_concepts.contains(&concept_id)
        {
            // Monorepo scoped comparison: only compare within same package root
            find_monorepo_contradictions(&group, config)
        } else {
            find_contradictions(&group, config)
        };

        results.push(ConceptResult {
            concept,
            assertions: group,
            findings,
        });
    }

    // Sort by concept display name
    results.sort_by(|a, b| a.concept.display_name.cmp(&b.concept.display_name));
    results
}

/// In monorepo mode, only compare assertions within the same package root.
fn find_monorepo_contradictions(
    assertions: &[ConfigAssertion],
    config: &ConflicConfig,
) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Build glob matchers for package roots
    let matchers: Vec<globset::GlobMatcher> = config
        .monorepo
        .package_roots
        .iter()
        .filter_map(|pattern| {
            globset::Glob::new(pattern).ok().map(|g| g.compile_matcher())
        })
        .collect();

    // Determine which package root each assertion belongs to
    let package_of = |assertion: &ConfigAssertion| -> Option<String> {
        let path_str = assertion.source.file.to_string_lossy();
        for matcher in &matchers {
            // Find the package root by checking path components
            let path = std::path::Path::new(path_str.as_ref());
            let mut accumulated = std::path::PathBuf::new();
            for component in path.components() {
                accumulated.push(component);
                if matcher.is_match(&accumulated) {
                    return Some(accumulated.to_string_lossy().to_string());
                }
            }
        }
        None // Not in any package root
    };

    // Group by package root
    let mut by_package: HashMap<String, Vec<&ConfigAssertion>> = HashMap::new();
    let mut root_level: Vec<&ConfigAssertion> = Vec::new();

    for assertion in assertions {
        if let Some(pkg) = package_of(assertion) {
            by_package.entry(pkg).or_default().push(assertion);
        } else {
            root_level.push(assertion);
        }
    }

    // Compare within each package (package assertions + root-level assertions)
    for (_pkg, pkg_assertions) in &by_package {
        let combined: Vec<&ConfigAssertion> = pkg_assertions
            .iter()
            .copied()
            .chain(root_level.iter().copied())
            .collect();

        for i in 0..combined.len() {
            for j in (i + 1)..combined.len() {
                if let Some(finding) = compare_pair(combined[i], combined[j]) {
                    if !config.should_ignore_finding(
                        &finding.rule_id,
                        &finding.left.source.file,
                        &finding.right.source.file,
                    ) {
                        findings.push(finding);
                    }
                }
            }
        }
    }

    // Also compare root-level assertions among themselves
    for i in 0..root_level.len() {
        for j in (i + 1)..root_level.len() {
            if let Some(finding) = compare_pair(root_level[i], root_level[j]) {
                if !config.should_ignore_finding(
                    &finding.rule_id,
                    &finding.left.source.file,
                    &finding.right.source.file,
                ) {
                    findings.push(finding);
                }
            }
        }
    }

    findings.sort_by(|a, b| b.severity.cmp(&a.severity));
    findings
}

fn find_contradictions(
    assertions: &[ConfigAssertion],
    config: &ConflicConfig,
) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Pairwise comparison
    for i in 0..assertions.len() {
        for j in (i + 1)..assertions.len() {
            let left = &assertions[i];
            let right = &assertions[j];

            if let Some(finding) = compare_pair(left, right) {
                // Check ignore rules
                if !config.should_ignore_finding(
                    &finding.rule_id,
                    &finding.left.source.file,
                    &finding.right.source.file,
                ) {
                    findings.push(finding);
                }
            }
        }
    }

    // Deduplicate: keep highest severity per file pair
    findings.sort_by(|a, b| b.severity.cmp(&a.severity));
    findings
}

fn compare_pair(left: &ConfigAssertion, right: &ConfigAssertion) -> Option<Finding> {
    let compatibility = match (&left.value, &right.value) {
        (SemanticType::Version(a), SemanticType::Version(b)) => {
            version::versions_compatible(a, b)
        }
        (SemanticType::Port(a), SemanticType::Port(b)) => {
            port::ports_compatible(a, b)
        }
        (SemanticType::Boolean(a), SemanticType::Boolean(b)) => {
            boolean::booleans_compatible(*a, *b)
        }
        (SemanticType::StringValue(a), SemanticType::StringValue(b)) => {
            string::strings_compatible(a, b)
        }
        _ => Compatibility::Unknown,
    };

    match compatibility {
        Compatibility::Compatible => None,
        Compatibility::Incompatible(explanation) => {
            let sev = severity::compute_severity(left.authority, right.authority);
            let rule_id = rule_id_for_type(&left.value);
            Some(Finding {
                severity: sev,
                left: left.clone(),
                right: right.clone(),
                explanation,
                rule_id,
            })
        }
        Compatibility::Unknown => None, // Don't report unknowns
    }
}

fn rule_id_for_type(value: &SemanticType) -> String {
    match value {
        SemanticType::Version(_) => "VER001".into(),
        SemanticType::Port(_) => "PORT001".into(),
        SemanticType::Boolean(_) => "BOOL001".into(),
        SemanticType::StringValue(_) => "STR001".into(),
        _ => "MISC001".into(),
    }
}

/// Result of comparing two values.
pub enum Compatibility {
    Compatible,
    Incompatible(String),
    Unknown,
}
