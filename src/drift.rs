use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::*;

/// How much a value has drifted from the expected baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DriftKind {
    /// Value matches the expected baseline exactly.
    Exact,
    /// Minor drift (e.g. patch version difference).
    Minor,
    /// Major drift (e.g. different major version).
    Major,
    /// Concept expected but no assertion found.
    Missing,
}

/// A single expectation in the organizational baseline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineExpectation {
    /// Concept ID this expectation applies to.
    pub concept: String,
    /// The expected value.
    pub expected: String,
    /// Optional tolerance (e.g. "20.x" for any 20.x version).
    #[serde(default)]
    pub tolerance: Option<String>,
}

/// The organizational baseline definition.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OrganizationalBaseline {
    /// Human-readable version identifier for this baseline.
    #[serde(default)]
    pub version: Option<String>,
    /// When this baseline was created.
    #[serde(default)]
    pub created_at: Option<String>,
    /// The expected values for each concept.
    #[serde(default)]
    pub expectation: Vec<BaselineExpectation>,
}

/// A single entry in the drift report.
#[derive(Debug, Clone, Serialize)]
pub struct BaselineDriftEntry {
    pub concept: String,
    pub expected: String,
    pub actual: String,
    pub drift_kind: DriftKind,
    pub files: Vec<PathBuf>,
}

/// The complete drift report for a single repo.
#[derive(Debug, Clone, Serialize)]
pub struct BaselineDriftReport {
    pub entries: Vec<BaselineDriftEntry>,
    /// 0.0 (fully drifted) to 1.0 (fully conformant).
    pub conformance_score: f64,
}

/// Load an organizational baseline from a TOML file.
pub fn load_organizational_baseline(path: &Path) -> anyhow::Result<OrganizationalBaseline> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Failed to read baseline {}: {}", path.display(), e))?;
    let baseline: OrganizationalBaseline = toml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("Failed to parse baseline {}: {}", path.display(), e))?;
    Ok(baseline)
}

/// Capture the current state of all concepts as an organizational baseline.
pub fn capture_baseline(result: &ScanResult) -> OrganizationalBaseline {
    let mut expectations = Vec::new();

    for cr in &result.concept_results {
        if cr.assertions.is_empty() {
            continue;
        }

        // Pick the highest-authority assertion as the "expected" value
        let best = cr
            .assertions
            .iter()
            .max_by_key(|a| a.authority)
            .unwrap();

        expectations.push(BaselineExpectation {
            concept: cr.concept.id.clone(),
            expected: best.raw_value.clone(),
            tolerance: None,
        });
    }

    OrganizationalBaseline {
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
        created_at: None,
        expectation: expectations,
    }
}

/// Compare scan results against an organizational baseline.
pub fn compare_to_baseline(
    result: &ScanResult,
    baseline: &OrganizationalBaseline,
    _scan_root: &Path,
) -> BaselineDriftReport {
    let mut entries = Vec::new();
    let mut conformant = 0usize;
    let total = baseline.expectation.len();

    // Build a map of concept → assertions from scan results
    let mut concept_map: HashMap<String, Vec<&ConfigAssertion>> = HashMap::new();
    for cr in &result.concept_results {
        for assertion in &cr.assertions {
            concept_map
                .entry(cr.concept.id.clone())
                .or_default()
                .push(assertion);
        }
    }

    for expectation in &baseline.expectation {
        let concept_id = resolve_concept_id(&expectation.concept, &concept_map);

        let assertions = concept_map.get(&concept_id);

        let is_missing = match assertions {
            None => true,
            Some(a) if a.is_empty() => true,
            _ => false,
        };

        if is_missing {
            entries.push(BaselineDriftEntry {
                concept: expectation.concept.clone(),
                expected: expectation.expected.clone(),
                actual: "(missing)".into(),
                drift_kind: DriftKind::Missing,
                files: vec![],
            });
        } else if let Some(assertions) = assertions {
                // Pick highest-authority assertion
                let best = assertions.iter().max_by_key(|a| a.authority).unwrap();
                let drift_kind = compute_drift(
                    &expectation.expected,
                    &best.raw_value,
                    expectation.tolerance.as_deref(),
                );

                if drift_kind == DriftKind::Exact {
                    conformant += 1;
                }

                let files: Vec<PathBuf> = assertions
                    .iter()
                    .map(|a| a.source.file.clone())
                    .collect();

                entries.push(BaselineDriftEntry {
                    concept: expectation.concept.clone(),
                    expected: expectation.expected.clone(),
                    actual: best.raw_value.clone(),
                    drift_kind,
                    files,
                });
        }
    }

    let conformance_score = if total == 0 {
        1.0
    } else {
        conformant as f64 / total as f64
    };

    BaselineDriftReport {
        entries,
        conformance_score,
    }
}

/// Resolve a concept alias to a real concept ID.
fn resolve_concept_id(
    alias: &str,
    concept_map: &HashMap<String, Vec<&ConfigAssertion>>,
) -> String {
    if concept_map.contains_key(alias) {
        return alias.to_string();
    }
    for key in concept_map.keys() {
        if crate::config::concept_matches_selector(key, alias) {
            return key.clone();
        }
    }
    alias.to_string()
}

/// Compute drift between expected and actual values.
fn compute_drift(expected: &str, actual: &str, tolerance: Option<&str>) -> DriftKind {
    // Check exact match
    if expected.trim() == actual.trim() {
        return DriftKind::Exact;
    }

    // Check tolerance (e.g. "20.x" means any 20.x.y is acceptable)
    if let Some(tolerance) = tolerance {
        if matches_tolerance(actual, tolerance) {
            return DriftKind::Minor;
        }
    }

    // Check if it's a minor version drift (same major version)
    let expected_major = extract_major(expected);
    let actual_major = extract_major(actual);
    if let (Some(em), Some(am)) = (expected_major, actual_major) {
        if em == am {
            return DriftKind::Minor;
        }
    }

    DriftKind::Major
}

/// Check if an actual value matches a tolerance pattern like "20.x".
fn matches_tolerance(actual: &str, tolerance: &str) -> bool {
    let tol_parts: Vec<&str> = tolerance.split('.').collect();
    let act_parts: Vec<&str> = actual.split('.').collect();

    for (tp, ap) in tol_parts.iter().zip(act_parts.iter()) {
        if *tp == "x" || *tp == "*" {
            continue;
        }
        if tp != ap {
            return false;
        }
    }
    true
}

/// Extract the major version number from a version string.
fn extract_major(version: &str) -> Option<u64> {
    version.trim().split('.').next()?.parse().ok()
}

/// Render drift report for terminal output.
pub fn render_drift_report(report: &BaselineDriftReport, no_color: bool) -> String {
    use owo_colors::OwoColorize;

    let mut out = String::new();

    out.push_str(&format!(
        "conflic v{} - baseline drift report\n\n",
        env!("CARGO_PKG_VERSION")
    ));

    out.push_str(&format!(
        "  {:<25} {:<15} {:<15} {}\n",
        "Concept", "Expected", "Actual", "Drift"
    ));

    let separator = "-".repeat(75);
    if no_color {
        out.push_str(&separator);
    } else {
        out.push_str(&separator.dimmed().to_string());
    }
    out.push('\n');

    for entry in &report.entries {
        let drift_label = match entry.drift_kind {
            DriftKind::Exact => {
                if no_color {
                    "OK".to_string()
                } else {
                    "OK".green().to_string()
                }
            }
            DriftKind::Minor => {
                if no_color {
                    "MINOR".to_string()
                } else {
                    "MINOR".yellow().to_string()
                }
            }
            DriftKind::Major => {
                if no_color {
                    "MAJOR".to_string()
                } else {
                    "MAJOR".red().to_string()
                }
            }
            DriftKind::Missing => {
                if no_color {
                    "MISSING".to_string()
                } else {
                    "MISSING".red().to_string()
                }
            }
        };

        out.push_str(&format!(
            "  {:<25} {:<15} {:<15} {}\n",
            entry.concept, entry.expected, entry.actual, drift_label
        ));
    }

    out.push('\n');

    let score_pct = (report.conformance_score * 100.0).round() as u32;
    if no_color {
        out.push_str(&format!("Conformance score: {}%\n", score_pct));
    } else {
        let score_str = format!("{}%", score_pct);
        let colored_score = if score_pct >= 80 {
            score_str.green().to_string()
        } else if score_pct >= 50 {
            score_str.yellow().to_string()
        } else {
            score_str.red().to_string()
        };
        out.push_str(&format!(
            "{}: {}\n",
            "Conformance score".bold(),
            colored_score
        ));
    }

    out
}

/// Render drift report as JSON.
pub fn render_drift_json(report: &BaselineDriftReport) -> String {
    serde_json::to_string_pretty(report).unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::assertion::{Authority, ConfigAssertion, SourceLocation};
    use crate::model::concept::{ConceptCategory, SemanticConcept};
    use crate::model::finding::ConceptResult;
    use crate::model::semantic_type::SemanticType;

    fn make_assertion(concept_id: &str, file: &str, raw: &str) -> ConfigAssertion {
        ConfigAssertion {
            concept: SemanticConcept {
                id: concept_id.into(),
                display_name: concept_id.into(),
                category: ConceptCategory::RuntimeVersion,
            },
            value: SemanticType::StringValue(raw.into()),
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

    #[test]
    fn test_exact_conformance() {
        let a = make_assertion("node-version", ".nvmrc", "20");
        let result = ScanResult {
            concept_results: vec![ConceptResult {
                concept: a.concept.clone(),
                assertions: vec![a],
                findings: vec![],
            }],
            parse_diagnostics: vec![],
        };

        let baseline = OrganizationalBaseline {
            version: None,
            created_at: None,
            expectation: vec![BaselineExpectation {
                concept: "node-version".into(),
                expected: "20".into(),
                tolerance: None,
            }],
        };

        let report = compare_to_baseline(&result, &baseline, Path::new("."));
        assert_eq!(report.conformance_score, 1.0);
        assert_eq!(report.entries[0].drift_kind, DriftKind::Exact);
    }

    #[test]
    fn test_major_drift() {
        let a = make_assertion("node-version", ".nvmrc", "18");
        let result = ScanResult {
            concept_results: vec![ConceptResult {
                concept: a.concept.clone(),
                assertions: vec![a],
                findings: vec![],
            }],
            parse_diagnostics: vec![],
        };

        let baseline = OrganizationalBaseline {
            version: None,
            created_at: None,
            expectation: vec![BaselineExpectation {
                concept: "node-version".into(),
                expected: "20".into(),
                tolerance: None,
            }],
        };

        let report = compare_to_baseline(&result, &baseline, Path::new("."));
        assert_eq!(report.conformance_score, 0.0);
        assert_eq!(report.entries[0].drift_kind, DriftKind::Major);
    }

    #[test]
    fn test_minor_drift_with_tolerance() {
        let a = make_assertion("node-version", ".nvmrc", "20.11.0");
        let result = ScanResult {
            concept_results: vec![ConceptResult {
                concept: a.concept.clone(),
                assertions: vec![a],
                findings: vec![],
            }],
            parse_diagnostics: vec![],
        };

        let baseline = OrganizationalBaseline {
            version: None,
            created_at: None,
            expectation: vec![BaselineExpectation {
                concept: "node-version".into(),
                expected: "20.18.0".into(),
                tolerance: Some("20.x".into()),
            }],
        };

        let report = compare_to_baseline(&result, &baseline, Path::new("."));
        assert_eq!(report.entries[0].drift_kind, DriftKind::Minor);
    }

    #[test]
    fn test_missing_concept() {
        let result = ScanResult {
            concept_results: vec![],
            parse_diagnostics: vec![],
        };

        let baseline = OrganizationalBaseline {
            version: None,
            created_at: None,
            expectation: vec![BaselineExpectation {
                concept: "node-version".into(),
                expected: "20".into(),
                tolerance: None,
            }],
        };

        let report = compare_to_baseline(&result, &baseline, Path::new("."));
        assert_eq!(report.entries[0].drift_kind, DriftKind::Missing);
        assert_eq!(report.conformance_score, 0.0);
    }

    #[test]
    fn test_capture_baseline() {
        let a = make_assertion("node-version", ".nvmrc", "20");
        let result = ScanResult {
            concept_results: vec![ConceptResult {
                concept: a.concept.clone(),
                assertions: vec![a],
                findings: vec![],
            }],
            parse_diagnostics: vec![],
        };

        let baseline = capture_baseline(&result);
        assert_eq!(baseline.expectation.len(), 1);
        assert_eq!(baseline.expectation[0].concept, "node-version");
        assert_eq!(baseline.expectation[0].expected, "20");
    }

    #[test]
    fn test_matches_tolerance() {
        assert!(matches_tolerance("20.11.0", "20.x"));
        assert!(matches_tolerance("20.18.0", "20.x"));
        assert!(!matches_tolerance("18.0.0", "20.x"));
        assert!(matches_tolerance("3.12.1", "3.12.*"));
    }
}
