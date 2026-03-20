use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::model::{Finding, ParseDiagnostic, ScanResult};

/// A baseline file that records known findings to suppress.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Baseline {
    pub version: String,
    pub fingerprints: Vec<FindingFingerprint>,
    #[serde(default)]
    pub parse_diagnostic_fingerprints: Vec<ParseDiagnosticFingerprint>,
}

/// A stable fingerprint for a finding, independent of line numbers.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct FindingFingerprint {
    pub rule_id: String,
    pub concept: String,
    #[serde(default)]
    pub severity: String,
    pub left_file: String,
    pub left_key_path: String,
    #[serde(default)]
    pub left_value: String,
    pub right_file: String,
    pub right_key_path: String,
    #[serde(default)]
    pub right_value: String,
}

/// A stable fingerprint for a parse diagnostic.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct ParseDiagnosticFingerprint {
    pub severity: String,
    pub rule_id: String,
    pub file: String,
    pub message: String,
}

impl FindingFingerprint {
    pub fn from_finding(finding: &Finding, scan_root: &Path) -> Self {
        Self {
            rule_id: finding.rule_id.clone(),
            concept: finding.left.concept.id.clone(),
            severity: finding.severity.to_string(),
            left_file: portable_path(&finding.left.source.file, scan_root),
            left_key_path: finding.left.source.key_path.clone(),
            left_value: normalized_finding_value(&finding.left.value),
            right_file: portable_path(&finding.right.source.file, scan_root),
            right_key_path: finding.right.source.key_path.clone(),
            right_value: normalized_finding_value(&finding.right.value),
        }
    }
}

impl ParseDiagnosticFingerprint {
    pub fn from_parse_diagnostic(parse_diagnostic: &ParseDiagnostic, scan_root: &Path) -> Self {
        Self {
            severity: parse_diagnostic.severity.to_string(),
            rule_id: parse_diagnostic.rule_id.clone(),
            file: portable_path(&parse_diagnostic.file, scan_root),
            message: parse_diagnostic.message.clone(),
        }
    }
}

impl Baseline {
    pub fn entry_count(&self) -> usize {
        self.fingerprints.len() + self.parse_diagnostic_fingerprints.len()
    }
}

/// Load a baseline from a JSON file.
pub fn load_baseline(path: &Path) -> anyhow::Result<Baseline> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Failed to read baseline {}: {}", path.display(), e))?;
    let baseline: Baseline = serde_json::from_str(&content)
        .map_err(|e| anyhow::anyhow!("Failed to parse baseline {}: {}", path.display(), e))?;
    Ok(baseline)
}

/// Generate a baseline from current scan results.
pub fn generate_baseline(result: &ScanResult, scan_root: &Path) -> Baseline {
    let mut fingerprints = Vec::new();
    for cr in &result.concept_results {
        for finding in &cr.findings {
            fingerprints.push(FindingFingerprint::from_finding(finding, scan_root));
        }
    }
    let parse_diagnostic_fingerprints = result
        .parse_diagnostics
        .iter()
        .map(|diagnostic| ParseDiagnosticFingerprint::from_parse_diagnostic(diagnostic, scan_root))
        .collect();

    Baseline {
        version: env!("CARGO_PKG_VERSION").to_string(),
        fingerprints,
        parse_diagnostic_fingerprints,
    }
}

/// Save a baseline to a JSON file.
pub fn save_baseline(baseline: &Baseline, path: &Path) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(baseline)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Filter findings, removing any that match the baseline.
pub fn filter_baselined(result: &mut ScanResult, baseline: &Baseline, scan_root: &Path) {
    let known: HashSet<FindingFingerprint> = baseline.fingerprints.iter().cloned().collect();
    let known_parse_diagnostics: HashSet<ParseDiagnosticFingerprint> = baseline
        .parse_diagnostic_fingerprints
        .iter()
        .cloned()
        .collect();

    for cr in &mut result.concept_results {
        cr.findings.retain(|finding| {
            let fp = FindingFingerprint::from_finding(finding, scan_root);
            !known.contains(&fp)
        });
    }

    result.parse_diagnostics.retain(|diagnostic| {
        let fingerprint = ParseDiagnosticFingerprint::from_parse_diagnostic(diagnostic, scan_root);
        !known_parse_diagnostics.contains(&fingerprint)
    });
}

/// Extract a portable path for stable fingerprinting.
/// Uses forward slashes for cross-platform consistency.
/// Tries relative path from the scan root first, falls back to the normalized path.
fn portable_path(path: &Path, scan_root: &Path) -> String {
    let normalized = normalize_path(path);
    if normalized.is_relative() {
        return normalized.to_string_lossy().replace('\\', "/");
    }

    let normalized_scan_root = normalize_path(scan_root);
    if let Ok(rel) = normalized.strip_prefix(&normalized_scan_root) {
        return rel.to_string_lossy().replace('\\', "/");
    }

    normalized.to_string_lossy().replace('\\', "/")
}

fn normalized_finding_value(value: &crate::model::SemanticType) -> String {
    value.to_string()
}

fn normalize_path(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();

    if let Some(stripped) = raw.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{}", stripped));
    }

    if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        return PathBuf::from(stripped);
    }

    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::assertion::{Authority, ConfigAssertion, SourceLocation};
    use crate::model::concept::{ConceptCategory, SemanticConcept};
    use crate::model::finding::{ConceptResult, Finding, Severity};
    use crate::model::semantic_type::SemanticType;
    use std::path::PathBuf;

    fn make_finding(left_file: &str, left_key: &str, right_file: &str, right_key: &str) -> Finding {
        let concept = SemanticConcept {
            id: "test-concept".into(),
            display_name: "Test".into(),
            category: ConceptCategory::RuntimeVersion,
        };
        let make_assertion = |file: &str, key: &str| ConfigAssertion {
            concept: concept.clone(),
            value: SemanticType::StringValue("v1".into()),
            raw_value: "v1".into(),
            source: SourceLocation {
                file: PathBuf::from(file),
                line: 1,
                column: 0,
                key_path: key.into(),
            },
            span: None,
            authority: Authority::Declared,
            extractor_id: "test".into(),
            is_matrix: false,
        };
        Finding {
            severity: Severity::Warning,
            left: make_assertion(left_file, left_key),
            right: make_assertion(right_file, right_key),
            explanation: "test".into(),
            rule_id: "TEST001".into(),
        }
    }

    fn make_parse_diagnostic(file: &str, message: &str) -> ParseDiagnostic {
        ParseDiagnostic {
            severity: Severity::Error,
            file: PathBuf::from(file),
            message: message.into(),
            rule_id: "PARSE001".into(),
        }
    }

    #[test]
    fn test_different_dirs_same_filename_no_collision() {
        // Two Dockerfiles in different directories should produce different fingerprints
        let f1 = make_finding("packages/a/Dockerfile", "FROM", "packages/a/.nvmrc", "");
        let f2 = make_finding("packages/b/Dockerfile", "FROM", "packages/b/.nvmrc", "");
        let scan_root = Path::new("workspace");

        let fp1 = FindingFingerprint::from_finding(&f1, scan_root);
        let fp2 = FindingFingerprint::from_finding(&f2, scan_root);

        assert_ne!(
            fp1, fp2,
            "Fingerprints for different directories should differ"
        );
    }

    #[test]
    fn test_filter_baselined_removes_known_findings() {
        let finding = make_finding("a.json", "key", "b.json", "key");
        let scan_root = Path::new("workspace");
        let fp = FindingFingerprint::from_finding(&finding, scan_root);

        let concept = finding.left.concept.clone();
        let mut result = ScanResult {
            concept_results: vec![ConceptResult {
                concept,
                assertions: vec![],
                findings: vec![finding],
            }],
            parse_diagnostics: vec![],
        };

        let baseline = Baseline {
            version: "1.0.0".into(),
            fingerprints: vec![fp],
            parse_diagnostic_fingerprints: vec![],
        };

        filter_baselined(&mut result, &baseline, scan_root);
        assert!(
            result.concept_results[0].findings.is_empty(),
            "Baselined finding should be removed"
        );
    }

    #[test]
    fn test_filter_baselined_keeps_new_findings() {
        let finding = make_finding("a.json", "key", "b.json", "key");
        let concept = finding.left.concept.clone();
        let scan_root = Path::new("workspace");

        let mut result = ScanResult {
            concept_results: vec![ConceptResult {
                concept,
                assertions: vec![],
                findings: vec![finding],
            }],
            parse_diagnostics: vec![],
        };

        let baseline = Baseline {
            version: "1.0.0".into(),
            fingerprints: vec![],
            parse_diagnostic_fingerprints: vec![],
        };

        filter_baselined(&mut result, &baseline, scan_root);
        assert_eq!(
            result.concept_results[0].findings.len(),
            1,
            "New finding should remain"
        );
    }

    #[test]
    fn test_filter_baselined_removes_known_parse_diagnostics() {
        let diagnostic = make_parse_diagnostic("broken.json", "Failed to parse JSON");
        let scan_root = Path::new("workspace");
        let mut result = ScanResult {
            concept_results: vec![],
            parse_diagnostics: vec![diagnostic.clone()],
        };

        let baseline = Baseline {
            version: "1.0.0".into(),
            fingerprints: vec![],
            parse_diagnostic_fingerprints: vec![ParseDiagnosticFingerprint::from_parse_diagnostic(
                &diagnostic,
                scan_root,
            )],
        };

        filter_baselined(&mut result, &baseline, scan_root);
        assert!(
            result.parse_diagnostics.is_empty(),
            "Baselined parse diagnostic should be removed"
        );
    }

    #[test]
    fn test_generate_baseline_includes_parse_diagnostics() {
        let diagnostic = make_parse_diagnostic("broken.json", "Failed to parse JSON");
        let result = ScanResult {
            concept_results: vec![],
            parse_diagnostics: vec![diagnostic],
        };
        let scan_root = Path::new("workspace");

        let baseline = generate_baseline(&result, scan_root);

        assert_eq!(baseline.parse_diagnostic_fingerprints.len(), 1);
        assert_eq!(baseline.entry_count(), 1);
    }

    #[test]
    fn test_generate_baseline_uses_scan_root_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let left = root.join("packages").join("a").join("Dockerfile");
        let right = root.join("packages").join("a").join(".nvmrc");
        let finding = make_finding(
            &left.to_string_lossy(),
            "FROM",
            &right.to_string_lossy(),
            "",
        );
        let result = ScanResult {
            concept_results: vec![ConceptResult {
                concept: finding.left.concept.clone(),
                assertions: vec![],
                findings: vec![finding],
            }],
            parse_diagnostics: vec![],
        };

        let baseline = generate_baseline(&result, root);

        assert_eq!(baseline.fingerprints.len(), 1);
        assert_eq!(baseline.fingerprints[0].left_file, "packages/a/Dockerfile");
        assert_eq!(baseline.fingerprints[0].right_file, "packages/a/.nvmrc");
    }

    #[test]
    fn test_save_and_load_baseline_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("baseline.json");

        let baseline = Baseline {
            version: "2.0.0".into(),
            fingerprints: vec![FindingFingerprint {
                rule_id: "VER001".into(),
                concept: "node-version".into(),
                severity: "ERROR".into(),
                left_file: ".nvmrc".into(),
                left_key_path: "".into(),
                left_value: "18".into(),
                right_file: "Dockerfile".into(),
                right_key_path: "FROM".into(),
                right_value: "20".into(),
            }],
            parse_diagnostic_fingerprints: vec![ParseDiagnosticFingerprint {
                severity: "ERROR".into(),
                rule_id: "PARSE001".into(),
                file: "package.json".into(),
                message: "Failed to parse JSON".into(),
            }],
        };

        save_baseline(&baseline, &path).unwrap();
        let loaded = load_baseline(&path).unwrap();

        assert_eq!(loaded.version, "2.0.0");
        assert_eq!(loaded.fingerprints.len(), 1);
        assert_eq!(loaded.fingerprints[0].rule_id, "VER001");
        assert_eq!(loaded.parse_diagnostic_fingerprints.len(), 1);
        assert_eq!(loaded.parse_diagnostic_fingerprints[0].rule_id, "PARSE001");
    }
}
