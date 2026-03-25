pub mod boolean;
pub mod constraint;
pub mod port;
pub mod registry;
pub mod severity;
pub mod solver_trait;
pub mod string;
pub mod version;

mod findings;
mod monorepo;

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;

use crate::config::ConflicConfig;
use crate::model::*;

pub use registry::SolverRegistry;

/// Compare all assertions grouped by concept and produce findings.
pub fn compare_assertions(
    scan_root: &Path,
    assertions: Vec<ConfigAssertion>,
    config: &ConflicConfig,
) -> Vec<ConceptResult> {
    compare_assertions_with_solvers(scan_root, assertions, config, &SolverRegistry::new())
}

/// Compare all assertions with a custom solver registry.
pub fn compare_assertions_with_solvers(
    scan_root: &Path,
    assertions: Vec<ConfigAssertion>,
    config: &ConflicConfig,
    solvers: &SolverRegistry,
) -> Vec<ConceptResult> {
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
        let unique_files: HashSet<_> = group
            .iter()
            .map(|assertion| crate::pathing::normalize_path(&assertion.source.file))
            .collect();
        if unique_files.len() < 2 {
            results.push(ConceptResult {
                concept,
                assertions: group,
                findings: vec![],
            });
            continue;
        }

        let constraint_solver = solvers.get_constraint(&concept_id);
        let concept_solver = solvers.get(&concept_id);
        let is_monorepo_scoped =
            use_monorepo && !config.monorepo.global_concepts.contains(&concept_id);

        // Monorepo per-package scoping always takes precedence.
        // N-ary constraint solver is a fast-path only for non-monorepo concepts.
        let findings = if is_monorepo_scoped {
            monorepo::find_monorepo_contradictions(scan_root, &group, config, concept_solver)
        } else if let Some(cs) = constraint_solver {
            constraint_findings(&group, config, cs, concept_solver)
        } else {
            findings::find_contradictions(&group, config, concept_solver)
        };

        results.push(ConceptResult {
            concept,
            assertions: group,
            findings,
        });
    }

    results.sort_by(|left, right| left.concept.display_name.cmp(&right.concept.display_name));
    results
}

/// Result of comparing two values.
pub enum Compatibility {
    Compatible,
    Incompatible(String),
    Unknown,
}

/// Use the N-ary constraint solver as a fast-path: if all assertions are
/// satisfiable simultaneously, skip pairwise comparison entirely (O(n log n)
/// instead of O(n^2)). When unsatisfiable, fall back to full pairwise to
/// preserve correct severity computation from authority levels.
fn constraint_findings(
    assertions: &[ConfigAssertion],
    config: &ConflicConfig,
    cs: &dyn constraint::ConstraintSolver,
    pairwise_solver: Option<&dyn solver_trait::Solver>,
) -> Vec<Finding> {
    let refs: Vec<&ConfigAssertion> = assertions.iter().collect();
    match cs.satisfiable(&refs) {
        constraint::ConstraintResult::Satisfiable { .. } => vec![],
        constraint::ConstraintResult::Unsatisfiable { .. } => {
            // Fall back to full pairwise to produce correctly-severed findings
            findings::find_contradictions(assertions, config, pairwise_solver)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConflicConfig;
    use crate::model::assertion::{Authority, ConfigAssertion, SourceLocation};
    use crate::model::concept::{ConceptCategory, SemanticConcept};
    use crate::model::semantic_type::SemanticType;
    use std::path::PathBuf;

    fn make_assertion(file: &str, value: &str, authority: Authority) -> ConfigAssertion {
        ConfigAssertion {
            concept: SemanticConcept {
                id: "test".into(),
                display_name: "Test".into(),
                category: ConceptCategory::RuntimeVersion,
            },
            value: SemanticType::StringValue(value.into()),
            raw_value: value.into(),
            source: SourceLocation {
                file: PathBuf::from(file),
                line: 1,
                column: 0,
                key_path: "".into(),
            },
            span: None,
            authority,
            extractor_id: "test".into(),
            is_matrix: false,
        }
    }

    #[test]
    fn test_compare_detects_string_contradiction() {
        let assertions = vec![
            make_assertion("a.json", "value-a", Authority::Declared),
            make_assertion("b.json", "value-b", Authority::Declared),
        ];
        let config = ConflicConfig::default();
        let results = compare_assertions(Path::new("."), assertions, &config);

        assert_eq!(results.len(), 1);
        assert!(
            !results[0].findings.is_empty(),
            "Should detect contradiction"
        );
    }

    #[test]
    fn test_compare_no_contradiction_when_same() {
        let assertions = vec![
            make_assertion("a.json", "same", Authority::Declared),
            make_assertion("b.json", "same", Authority::Declared),
        ];
        let config = ConflicConfig::default();
        let results = compare_assertions(Path::new("."), assertions, &config);

        assert_eq!(results.len(), 1);
        assert!(
            results[0].findings.is_empty(),
            "Same values should not contradict"
        );
    }

    #[test]
    fn test_skip_concept_removes_from_results() {
        let assertions = vec![
            make_assertion("a.json", "val-a", Authority::Declared),
            make_assertion("b.json", "val-b", Authority::Declared),
        ];
        let mut config = ConflicConfig::default();
        config.conflic.skip_concepts.push("test".into());

        let results = compare_assertions(Path::new("."), assertions, &config);
        assert!(results.is_empty(), "Skipped concepts should not appear");
    }

    #[test]
    fn test_single_assertion_no_findings() {
        let assertions = vec![make_assertion("a.json", "only-one", Authority::Declared)];
        let config = ConflicConfig::default();
        let results = compare_assertions(Path::new("."), assertions, &config);

        assert_eq!(results.len(), 1);
        assert!(results[0].findings.is_empty());
    }

    #[test]
    fn test_monorepo_scoped_comparison() {
        let root = tempfile::tempdir().unwrap();
        let mut config = ConflicConfig::default();
        config.monorepo.per_package = true;
        config.monorepo.package_roots.push("packages/*".into());

        let a1 = make_assertion(
            root.path()
                .join("packages")
                .join("a")
                .join("config.json")
                .to_string_lossy()
                .as_ref(),
            "val-a",
            Authority::Declared,
        );
        let a2 = make_assertion(
            root.path()
                .join("packages")
                .join("a")
                .join("other.json")
                .to_string_lossy()
                .as_ref(),
            "val-b",
            Authority::Declared,
        );
        let b1 = make_assertion(
            root.path()
                .join("packages")
                .join("b")
                .join("config.json")
                .to_string_lossy()
                .as_ref(),
            "val-c",
            Authority::Declared,
        );

        let assertions = vec![a1, a2, b1];
        let results = compare_assertions(root.path(), assertions, &config);

        let test_results = results
            .iter()
            .find(|result| result.concept.id == "test")
            .unwrap();
        assert_eq!(
            test_results.findings.len(),
            1,
            "Only package-local contradictions should be reported"
        );
    }

    #[test]
    fn test_matrix_duplicates_are_deduplicated() {
        let mut assertions = Vec::new();
        for line in 1..=200 {
            let mut left = make_assertion("workflow-a.yml", "18", Authority::Enforced);
            left.source.line = line;
            left.is_matrix = true;
            assertions.push(left);

            let mut right = make_assertion("workflow-b.yml", "20", Authority::Enforced);
            right.source.line = line;
            right.is_matrix = true;
            assertions.push(right);
        }

        let config = ConflicConfig::default();
        let results = compare_assertions(Path::new("."), assertions, &config);
        let test_results = results
            .iter()
            .find(|result| result.concept.id == "test")
            .unwrap();

        assert_eq!(
            test_results.findings.len(),
            1,
            "duplicate matrix values should collapse to a single file-pair finding"
        );
    }

    #[test]
    fn test_monorepo_prefers_most_specific_package_root() {
        let root = tempfile::tempdir().unwrap();
        let mut config = ConflicConfig::default();
        config.monorepo.per_package = true;
        config.monorepo.package_roots = vec!["apps/*".into(), "apps/*/packages/*".into()];

        let left = make_assertion(
            root.path()
                .join("apps")
                .join("web")
                .join("packages")
                .join("a")
                .join("package.json")
                .to_string_lossy()
                .as_ref(),
            "18",
            Authority::Declared,
        );
        let right = make_assertion(
            root.path()
                .join("apps")
                .join("web")
                .join("packages")
                .join("b")
                .join("package.json")
                .to_string_lossy()
                .as_ref(),
            "20",
            Authority::Declared,
        );

        let results = compare_assertions(root.path(), vec![left, right], &config);
        let test_results = results
            .iter()
            .find(|result| result.concept.id == "test")
            .unwrap();

        assert!(
            test_results.findings.is_empty(),
            "nested package roots should win over broader matches"
        );
    }

    #[test]
    fn test_custom_solver_overrides_default_comparison() {
        // Values "20.0.0" and ">=18" are StringValue (not parsed as versions here).
        // With default string comparison, they differ. With a semver solver, they're compatible.
        let assertions = vec![
            make_assertion("a.json", "20.0.0", Authority::Declared),
            make_assertion("b.json", ">=18", Authority::Declared),
        ];

        // Without solver: string comparison → incompatible
        let config = ConflicConfig::default();
        let results = compare_assertions(Path::new("."), assertions.clone(), &config);
        assert!(
            !results[0].findings.is_empty(),
            "String comparison should find contradiction"
        );

        // With semver solver: version comparison → compatible
        let mut solvers = SolverRegistry::new();
        solvers.register("test".into(), Box::new(registry::SemverSolver));
        let results =
            compare_assertions_with_solvers(Path::new("."), assertions, &config, &solvers);
        assert!(
            results[0].findings.is_empty(),
            "Semver solver should find these compatible"
        );
    }

    #[test]
    fn test_custom_solver_uses_solver_rule_id() {
        let assertions = vec![
            make_assertion("a.json", "8080", Authority::Declared),
            make_assertion("b.json", "3000", Authority::Declared),
        ];

        let mut solvers = SolverRegistry::new();
        solvers.register("test".into(), Box::new(registry::PortSolver));
        let config = ConflicConfig::default();
        let results =
            compare_assertions_with_solvers(Path::new("."), assertions, &config, &solvers);

        assert_eq!(results[0].findings.len(), 1);
        assert_eq!(
            results[0].findings[0].rule_id, "PORT001",
            "Should use the solver's rule_id"
        );
    }
}
