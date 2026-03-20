use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

use crate::model::{Finding, ScanResult};

/// A baseline file that records known findings to suppress.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Baseline {
    pub version: String,
    pub fingerprints: Vec<FindingFingerprint>,
}

/// A stable fingerprint for a finding, independent of line numbers.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct FindingFingerprint {
    pub rule_id: String,
    pub concept: String,
    pub left_file: String,
    pub left_key_path: String,
    pub right_file: String,
    pub right_key_path: String,
}

impl FindingFingerprint {
    pub fn from_finding(finding: &Finding) -> Self {
        Self {
            rule_id: finding.rule_id.clone(),
            concept: finding.left.concept.id.clone(),
            left_file: filename_of(&finding.left.source.file),
            left_key_path: finding.left.source.key_path.clone(),
            right_file: filename_of(&finding.right.source.file),
            right_key_path: finding.right.source.key_path.clone(),
        }
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
pub fn generate_baseline(result: &ScanResult) -> Baseline {
    let mut fingerprints = Vec::new();
    for cr in &result.concept_results {
        for finding in &cr.findings {
            fingerprints.push(FindingFingerprint::from_finding(finding));
        }
    }
    Baseline {
        version: env!("CARGO_PKG_VERSION").to_string(),
        fingerprints,
    }
}

/// Save a baseline to a JSON file.
pub fn save_baseline(baseline: &Baseline, path: &Path) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(baseline)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Filter findings, removing any that match the baseline.
pub fn filter_baselined(result: &mut ScanResult, baseline: &Baseline) {
    let known: HashSet<FindingFingerprint> = baseline
        .fingerprints
        .iter()
        .cloned()
        .collect();

    for cr in &mut result.concept_results {
        cr.findings.retain(|finding| {
            let fp = FindingFingerprint::from_finding(finding);
            !known.contains(&fp)
        });
    }
}

/// Extract just the filename from a path for stable fingerprinting.
/// Using filename rather than full path ensures portability across machines/CWDs.
fn filename_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}
