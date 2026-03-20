pub mod patcher;

use crate::model::*;
use crate::parse::FileFormat;
use std::path::PathBuf;

/// A proposed fix for a single file.
#[derive(Debug)]
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

    for cr in &result.concept_results {
        if cr.findings.is_empty() || cr.assertions.len() < 2 {
            continue;
        }

        // Find the winner: highest authority, and among ties, prefer non-matrix assertions
        let winner = cr
            .assertions
            .iter()
            .max_by(|a, b| {
                a.authority
                    .cmp(&b.authority)
                    .then_with(|| {
                        // Prefer non-matrix values as the canonical source
                        b.is_matrix.cmp(&a.is_matrix)
                    })
            });

        let winner = match winner {
            Some(w) => w,
            None => continue,
        };

        // Check if we have multiple assertions at the same highest authority with different values
        let top_authority_assertions: Vec<&ConfigAssertion> = cr
            .assertions
            .iter()
            .filter(|a| a.authority == winner.authority)
            .collect();

        let all_same_value = top_authority_assertions
            .windows(2)
            .all(|w| values_equivalent(&w[0].value, &w[1].value));

        if !all_same_value && top_authority_assertions.len() > 1 {
            unfixable.push(UnfixableItem {
                concept: cr.concept.clone(),
                reason: format!(
                    "Multiple {} assertions disagree — manual resolution needed",
                    winner.authority
                ),
            });
            continue;
        }

        // Propose fixes for lower-authority assertions that differ
        for assertion in &cr.assertions {
            if std::ptr::eq(assertion, winner) {
                continue;
            }

            if values_equivalent(&assertion.value, &winner.value) {
                continue;
            }

            // Determine the proposed replacement value
            let proposed_raw = compute_proposed_value(winner, assertion);

            let filename = assertion
                .source
                .file
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            let format =
                crate::parse::detect_format(filename, &assertion.source.file);

            proposals.push(FixProposal {
                file: assertion.source.file.clone(),
                concept: cr.concept.clone(),
                current_raw: assertion.raw_value.clone(),
                proposed_raw,
                key_path: assertion.source.key_path.clone(),
                line: assertion.source.line,
                authority_winner: format!("{} ({})", winner.authority, winner.source.file.display()),
                winner_file: winner.source.file.clone(),
                format,
            });
        }
    }

    FixPlan {
        proposals,
        unfixable,
    }
}

/// Determine if two semantic values are equivalent (no fix needed).
fn values_equivalent(a: &SemanticType, b: &SemanticType) -> bool {
    use crate::solve::Compatibility;

    match (a, b) {
        (SemanticType::Version(va), SemanticType::Version(vb)) => {
            matches!(
                crate::solve::version::versions_compatible(va, vb),
                Compatibility::Compatible
            )
        }
        (SemanticType::Port(pa), SemanticType::Port(pb)) => {
            matches!(
                crate::solve::port::ports_compatible(pa, pb),
                Compatibility::Compatible
            )
        }
        (SemanticType::Boolean(a), SemanticType::Boolean(b)) => a == b,
        (SemanticType::StringValue(a), SemanticType::StringValue(b)) => a == b,
        (SemanticType::Number(a), SemanticType::Number(b)) => (a - b).abs() < f64::EPSILON,
        _ => false,
    }
}

/// Compute what the proposed raw value should be for an assertion to match the winner.
fn compute_proposed_value(winner: &ConfigAssertion, target: &ConfigAssertion) -> String {
    match (&winner.value, &target.value) {
        (SemanticType::Version(winner_ver), SemanticType::Version(_)) => {
            // For plain text files (.nvmrc, .python-version, .ruby-version),
            // use the winner's exact version string
            match winner_ver {
                VersionSpec::Exact(v) => v.to_string(),
                VersionSpec::Partial { major, minor, .. } => {
                    if let Some(m) = minor {
                        format!("{}.{}", major, m)
                    } else {
                        major.to_string()
                    }
                }
                VersionSpec::DockerTag { version, .. } => {
                    // Use the version part from the docker tag
                    version.clone()
                }
                _ => winner.raw_value.clone(),
            }
        }
        (SemanticType::Boolean(b), _) => b.to_string(),
        (SemanticType::Port(p), _) => match p {
            PortSpec::Single(port) => port.to_string(),
            PortSpec::Range(start, end) => format!("{}-{}", start, end),
            PortSpec::Mapping { container, .. } => container.to_string(),
        },
        _ => winner.raw_value.clone(),
    }
}

