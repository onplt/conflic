pub mod patcher;

mod planner;
mod render;

use crate::model::*;
use crate::parse::FileFormat;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use planner::{all_values_mutually_equivalent, build_fix_operation, values_equivalent};

/// A safe, format-aware edit operation derived from a specific extractor target.
#[derive(Debug, Clone)]
pub enum FixOperation {
    ReplaceWholeFileValue {
        value: String,
    },
    ReplaceEnvValue {
        key: String,
        value: String,
    },
    ReplaceJsonString {
        path: Vec<String>,
        value: String,
    },
    ReplaceGoModVersion {
        value: String,
    },
    ReplaceToolVersionsValue {
        value: String,
    },
    ReplaceGemfileRubyVersion {
        value: String,
    },
    ReplaceTextRange {
        start: usize,
        end: usize,
        value: String,
    },
    ReplaceDockerFromArguments {
        arguments: String,
    },
    ReplaceDockerExposeToken {
        current: String,
        value: String,
    },
}

/// A proposed fix for a single file.
#[derive(Debug, Clone)]
pub struct FixProposal {
    pub file: PathBuf,
    pub concept: SemanticConcept,
    pub current_raw: String,
    pub proposed_raw: String,
    pub key_path: String,
    pub line: usize,
    pub authority_winner: String,
    pub winner_file: PathBuf,
    pub format: FileFormat,
    pub operation: FixOperation,
}

/// Result of analyzing contradictions for fixability.
#[derive(Debug)]
pub struct FixPlan {
    pub proposals: Vec<FixProposal>,
    pub unfixable: Vec<UnfixableItem>,
}

/// A contradiction that cannot be auto-fixed.
#[derive(Debug)]
pub struct UnfixableItem {
    pub concept: SemanticConcept,
    pub reason: String,
}

/// Analyze scan results and generate fix proposals.
/// The highest-authority assertion wins; lower-authority assertions are proposed for update.
pub fn plan_fixes(result: &ScanResult) -> FixPlan {
    let mut proposals = Vec::new();
    let mut unfixable = Vec::new();

    for concept_result in &result.concept_results {
        if concept_result.findings.is_empty() || concept_result.assertions.len() < 2 {
            continue;
        }

        let winner = match concept_result.assertions.iter().max_by(|left, right| {
            left.authority
                .cmp(&right.authority)
                .then_with(|| right.is_matrix.cmp(&left.is_matrix))
        }) {
            Some(winner) => winner,
            None => continue,
        };

        let top_authority_assertions: Vec<&ConfigAssertion> = concept_result
            .assertions
            .iter()
            .filter(|assertion| assertion.authority == winner.authority)
            .collect();

        if !all_values_mutually_equivalent(&top_authority_assertions)
            && top_authority_assertions.len() > 1
        {
            unfixable.push(UnfixableItem {
                concept: concept_result.concept.clone(),
                reason: format!(
                    "Multiple {} assertions disagree; manual resolution needed",
                    winner.authority
                ),
            });
            continue;
        }

        let ambiguous_docker_expose_lines = ambiguous_docker_expose_lines(concept_result);
        let mut reported_ambiguous_docker_expose_lines = HashSet::new();

        for assertion in &concept_result.assertions {
            if std::ptr::eq(assertion, winner) || values_equivalent(&assertion.value, &winner.value)
            {
                continue;
            }

            if assertion.extractor_id == "port-dockerfile" {
                let line_key = (assertion.source.file.clone(), assertion.source.line);
                if ambiguous_docker_expose_lines.contains(&line_key) {
                    if reported_ambiguous_docker_expose_lines.insert(line_key.clone()) {
                        unfixable.push(UnfixableItem {
                            concept: concept_result.concept.clone(),
                            reason: format!(
                                "{}:{} [{}]: Dockerfile EXPOSE line contains multiple port tokens; manual update required",
                                line_key.0.display(),
                                line_key.1,
                                assertion.extractor_id
                            ),
                        });
                    }
                    continue;
                }
            }

            let filename = assertion
                .source
                .file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            let format = crate::parse::detect_format(filename, &assertion.source.file);

            match build_fix_operation(winner, assertion, format.clone()) {
                Ok((proposed_raw, operation)) => {
                    proposals.push(FixProposal {
                        file: assertion.source.file.clone(),
                        concept: concept_result.concept.clone(),
                        current_raw: assertion.raw_value.clone(),
                        proposed_raw,
                        key_path: assertion.source.key_path.clone(),
                        line: assertion.source.line,
                        authority_winner: format!(
                            "{} ({})",
                            winner.authority,
                            winner.source.file.display()
                        ),
                        winner_file: winner.source.file.clone(),
                        format,
                        operation,
                    });
                }
                Err(reason) => {
                    unfixable.push(UnfixableItem {
                        concept: concept_result.concept.clone(),
                        reason: format!(
                            "{} [{}:{}]: {}",
                            assertion.source.file.display(),
                            assertion.source.line,
                            assertion.extractor_id,
                            reason
                        ),
                    });
                }
            }
        }
    }

    FixPlan {
        proposals,
        unfixable,
    }
}

fn ambiguous_docker_expose_lines(concept_result: &ConceptResult) -> HashSet<(PathBuf, usize)> {
    let mut counts: HashMap<(PathBuf, usize), usize> = HashMap::new();

    for assertion in &concept_result.assertions {
        if assertion.extractor_id == "port-dockerfile" {
            *counts
                .entry((assertion.source.file.clone(), assertion.source.line))
                .or_default() += 1;
        }
    }

    counts
        .into_iter()
        .filter_map(|(line_key, count)| (count > 1).then_some(line_key))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::concept::{ConceptCategory, SemanticConcept};
    use std::path::PathBuf;

    fn concept() -> SemanticConcept {
        SemanticConcept {
            id: "node-version".into(),
            display_name: "Node.js Version".into(),
            category: ConceptCategory::RuntimeVersion,
        }
    }

    fn assertion(
        extractor_id: &str,
        file: &str,
        raw_value: &str,
        value: SemanticType,
        authority: Authority,
    ) -> ConfigAssertion {
        ConfigAssertion {
            concept: concept(),
            value,
            raw_value: raw_value.into(),
            source: SourceLocation {
                file: PathBuf::from(file),
                line: 1,
                column: 0,
                key_path: String::new(),
            },
            span: None,
            authority,
            extractor_id: extractor_id.into(),
            is_matrix: false,
        }
    }

    #[test]
    fn test_plan_fixes_refuses_range_for_exact_version_file() {
        let winner = assertion(
            "node-version-package-json",
            "package.json",
            "^20",
            SemanticType::Version(parse_version("^20")),
            Authority::Declared,
        );
        let target = assertion(
            "node-version-nvmrc",
            ".nvmrc",
            "18",
            SemanticType::Version(parse_version("18")),
            Authority::Advisory,
        );

        let result = ScanResult {
            concept_results: vec![ConceptResult {
                concept: concept(),
                assertions: vec![winner, target],
                findings: vec![Finding {
                    severity: Severity::Error,
                    left: assertion(
                        "node-version-package-json",
                        "package.json",
                        "^20",
                        SemanticType::Version(parse_version("^20")),
                        Authority::Declared,
                    ),
                    right: assertion(
                        "node-version-nvmrc",
                        ".nvmrc",
                        "18",
                        SemanticType::Version(parse_version("18")),
                        Authority::Advisory,
                    ),
                    explanation: "contradiction".into(),
                    rule_id: "TEST001".into(),
                }],
            }],
            parse_diagnostics: vec![],
        };

        let plan = plan_fixes(&result);
        assert!(plan.proposals.is_empty(), "unsafe fix should be rejected");
        assert_eq!(plan.unfixable.len(), 1);
        assert!(
            plan.unfixable[0]
                .reason
                .contains("not an exact version token"),
            "expected exact-version rejection, got {}",
            plan.unfixable[0].reason
        );
    }

    #[test]
    fn test_plan_fixes_refuses_ambiguous_top_authority_ranges() {
        let left = assertion(
            "node-version-package-json",
            "packages/a/package.json",
            ">=18 <20",
            SemanticType::Version(parse_version(">=18 <20")),
            Authority::Declared,
        );
        let middle = assertion(
            "node-version-package-json",
            "packages/b/package.json",
            ">=19 <21",
            SemanticType::Version(parse_version(">=19 <21")),
            Authority::Declared,
        );
        let right = assertion(
            "node-version-package-json",
            "packages/c/package.json",
            ">=20 <22",
            SemanticType::Version(parse_version(">=20 <22")),
            Authority::Declared,
        );

        let result = ScanResult {
            concept_results: vec![ConceptResult {
                concept: concept(),
                assertions: vec![left.clone(), middle, right.clone()],
                findings: vec![Finding {
                    severity: Severity::Warning,
                    left,
                    right,
                    explanation:
                        "ranges \">=18.0.0 <20.0.0\" and \">=20.0.0 <22.0.0\" do not overlap".into(),
                    rule_id: "VER001".into(),
                }],
            }],
            parse_diagnostics: vec![],
        };

        let plan = plan_fixes(&result);
        assert!(
            plan.proposals.is_empty(),
            "ambiguous top-authority ranges should not produce auto-fix proposals: {:?}",
            plan.proposals
        );
        assert_eq!(plan.unfixable.len(), 1);
        assert!(
            plan.unfixable[0]
                .reason
                .contains("Multiple declared assertions disagree"),
            "expected explicit ambiguity warning, got {:?}",
            plan.unfixable
        );
    }

    #[test]
    fn test_plan_fixes_refuses_overlapping_top_authority_ranges() {
        let left = assertion(
            "node-version-ci",
            ".github/workflows/ci-a.yml",
            ">=18 <20",
            SemanticType::Version(parse_version(">=18 <20")),
            Authority::Enforced,
        );
        let right = assertion(
            "node-version-ci",
            ".github/workflows/ci-b.yml",
            ">=19 <21",
            SemanticType::Version(parse_version(">=19 <21")),
            Authority::Enforced,
        );
        let target = assertion(
            "node-version-package-json",
            "package.json",
            "22",
            SemanticType::Version(parse_version("22")),
            Authority::Declared,
        );

        let result = ScanResult {
            concept_results: vec![ConceptResult {
                concept: concept(),
                assertions: vec![left.clone(), right.clone(), target.clone()],
                findings: vec![
                    Finding {
                        severity: Severity::Error,
                        left: left.clone(),
                        right: target.clone(),
                        explanation: "\"22.0.0\" does not satisfy \">=18.0.0 <20.0.0\"".into(),
                        rule_id: "VER001".into(),
                    },
                    Finding {
                        severity: Severity::Error,
                        left: right,
                        right: target,
                        explanation: "\"22.0.0\" does not satisfy \">=19.0.0 <21.0.0\"".into(),
                        rule_id: "VER001".into(),
                    },
                ],
            }],
            parse_diagnostics: vec![],
        };

        let plan = plan_fixes(&result);
        assert!(
            plan.proposals.is_empty(),
            "overlapping top-authority ranges should not produce arbitrary auto-fix proposals: {:?}",
            plan.proposals
        );
        assert_eq!(plan.unfixable.len(), 1);
        assert!(
            plan.unfixable[0]
                .reason
                .contains("Multiple enforced assertions disagree"),
            "expected overlapping winners to be treated as ambiguous, got {:?}",
            plan.unfixable
        );
    }
}
