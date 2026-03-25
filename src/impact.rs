use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::config::ConflicConfig;
use crate::model::{ScanResult, Severity};

/// A file affected by a configuration change.
#[derive(Debug, Clone)]
pub struct ImpactedFile {
    pub path: PathBuf,
    pub concept_ids: Vec<String>,
    pub reason: String,
}

/// Blast radius summary for a set of changes.
#[derive(Debug, Clone)]
pub struct BlastRadius {
    pub files_affected: usize,
    pub concepts_affected: usize,
    pub worst_severity: Option<Severity>,
}

/// The complete impact analysis report.
#[derive(Debug, Clone)]
pub struct ImpactReport {
    /// Files that were directly changed.
    pub root_changes: Vec<ImpactedFile>,
    /// Files that share concepts with root changes (peer files).
    pub direct_impacts: Vec<ImpactedFile>,
    /// Files affected transitively via concept_rule dependencies.
    pub transitive_impacts: Vec<ImpactedFile>,
    /// All concepts affected (direct + transitive).
    pub affected_concepts: Vec<String>,
    /// Summary of the blast radius.
    pub blast_radius: BlastRadius,
}

/// Build an impact report by analyzing which concepts are affected by
/// changed files and propagating through cross-concept rules.
pub fn analyze_impact(
    scan_result: &ScanResult,
    changed_files: &[PathBuf],
    scan_root: &Path,
    config: &ConflicConfig,
) -> ImpactReport {
    let changed_normalized: HashSet<PathBuf> = changed_files
        .iter()
        .map(|p| crate::pathing::normalize_for_workspace(scan_root, p))
        .collect();

    // Step 1: Find directly impacted concepts from changed files
    let mut direct_concepts: HashSet<String> = HashSet::new();
    let mut root_changes: Vec<ImpactedFile> = Vec::new();

    // Build assertion-to-file and concept-to-files maps
    let mut concept_files: HashMap<String, Vec<(PathBuf, String)>> = HashMap::new();

    for cr in &scan_result.concept_results {
        for assertion in &cr.assertions {
            let norm_path =
                crate::pathing::normalize_for_workspace(scan_root, &assertion.source.file);
            concept_files
                .entry(cr.concept.id.clone())
                .or_default()
                .push((
                    norm_path.clone(),
                    assertion.source.file.display().to_string(),
                ));

            if changed_normalized.contains(&norm_path) {
                direct_concepts.insert(cr.concept.id.clone());
            }
        }
    }

    // Build root changes list
    for path in &changed_normalized {
        let mut concepts_for_file = Vec::new();
        for cr in &scan_result.concept_results {
            for assertion in &cr.assertions {
                let norm =
                    crate::pathing::normalize_for_workspace(scan_root, &assertion.source.file);
                if norm == *path && !concepts_for_file.contains(&cr.concept.id) {
                    concepts_for_file.push(cr.concept.id.clone());
                }
            }
        }
        if !concepts_for_file.is_empty() {
            root_changes.push(ImpactedFile {
                path: path.clone(),
                concept_ids: concepts_for_file,
                reason: "directly changed".into(),
            });
        }
    }

    // Step 2: Find peer files (same concepts, different files)
    let mut direct_impact_set: HashSet<PathBuf> = HashSet::new();
    let mut direct_impacts: Vec<ImpactedFile> = Vec::new();

    for concept_id in &direct_concepts {
        if let Some(files) = concept_files.get(concept_id) {
            for (norm_path, display_path) in files {
                if !changed_normalized.contains(norm_path)
                    && direct_impact_set.insert(norm_path.clone())
                {
                    direct_impacts.push(ImpactedFile {
                        path: PathBuf::from(display_path),
                        concept_ids: vec![concept_id.clone()],
                        reason: format!("shares concept '{}'", concept_id),
                    });
                }
            }
        }
    }

    // Step 3: Propagate through concept_rule dependencies (BFS)
    let mut transitive_concepts: HashSet<String> = HashSet::new();
    let mut transitive_impacts: Vec<ImpactedFile> = Vec::new();

    if !config.concept_rule.is_empty() {
        // Build adjacency list from concept rules
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        for rule in &config.concept_rule {
            adj.entry(rule.when.concept.clone())
                .or_default()
                .push(rule.then.concept.clone());
        }

        // BFS from directly impacted concepts
        let mut queue: VecDeque<String> = direct_concepts.iter().cloned().collect();
        let mut visited: HashSet<String> = direct_concepts.clone();

        while let Some(concept) = queue.pop_front() {
            if let Some(neighbors) = adj.get(&concept) {
                for neighbor in neighbors {
                    // Also check alias resolution
                    let resolved = resolve_concept_id(neighbor, &concept_files);
                    if visited.insert(resolved.clone()) {
                        transitive_concepts.insert(resolved.clone());
                        queue.push_back(resolved.clone());

                        // Add files from transitive concept
                        if let Some(files) = concept_files.get(&resolved) {
                            for (norm_path, display_path) in files {
                                if !changed_normalized.contains(norm_path)
                                    && !direct_impact_set.contains(norm_path)
                                {
                                    transitive_impacts.push(ImpactedFile {
                                        path: PathBuf::from(display_path),
                                        concept_ids: vec![resolved.clone()],
                                        reason: format!(
                                            "transitively affected via concept rule '{}'",
                                            neighbor
                                        ),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Step 4: Build summary
    let mut all_concepts: Vec<String> = direct_concepts.into_iter().collect();
    all_concepts.extend(transitive_concepts);
    all_concepts.sort();
    all_concepts.dedup();

    let worst_severity = scan_result
        .concept_results
        .iter()
        .filter(|cr| all_concepts.contains(&cr.concept.id))
        .flat_map(|cr| &cr.findings)
        .map(|f| f.severity)
        .max();

    let blast_radius = BlastRadius {
        files_affected: root_changes.len() + direct_impacts.len() + transitive_impacts.len(),
        concepts_affected: all_concepts.len(),
        worst_severity,
    };

    ImpactReport {
        root_changes,
        direct_impacts,
        transitive_impacts,
        affected_concepts: all_concepts,
        blast_radius,
    }
}

/// Resolve a concept alias to a real concept ID if present in the map.
fn resolve_concept_id(
    alias: &str,
    concept_files: &HashMap<String, Vec<(PathBuf, String)>>,
) -> String {
    if concept_files.contains_key(alias) {
        return alias.to_string();
    }
    // Try known aliases
    for key in concept_files.keys() {
        if crate::config::concept_matches_selector(key, alias) {
            return key.clone();
        }
    }
    alias.to_string()
}

/// Render an impact report as terminal output.
pub fn render_impact_report(report: &ImpactReport, no_color: bool) -> String {
    use owo_colors::OwoColorize;

    let mut out = String::new();

    out.push_str(&format!(
        "conflic v{} - impact analysis\n\n",
        env!("CARGO_PKG_VERSION")
    ));

    // Root changes
    if !report.root_changes.is_empty() {
        if no_color {
            out.push_str("Root changes:\n");
        } else {
            out.push_str(&format!("{}\n", "Root changes:".bold()));
        }
        for file in &report.root_changes {
            out.push_str(&format!(
                "  {} [{}]\n",
                file.path.display(),
                file.concept_ids.join(", ")
            ));
        }
        out.push('\n');
    }

    // Direct impacts
    if !report.direct_impacts.is_empty() {
        if no_color {
            out.push_str(&format!(
                "Direct impacts ({} file(s)):\n",
                report.direct_impacts.len()
            ));
        } else {
            out.push_str(&format!(
                "{}\n",
                format!("Direct impacts ({} file(s)):", report.direct_impacts.len()).yellow()
            ));
        }
        for file in &report.direct_impacts {
            out.push_str(&format!("  {} - {}\n", file.path.display(), file.reason));
        }
        out.push('\n');
    }

    // Transitive impacts
    if !report.transitive_impacts.is_empty() {
        if no_color {
            out.push_str(&format!(
                "Transitive impacts ({} file(s)):\n",
                report.transitive_impacts.len()
            ));
        } else {
            out.push_str(&format!(
                "{}\n",
                format!(
                    "Transitive impacts ({} file(s)):",
                    report.transitive_impacts.len()
                )
                .red()
            ));
        }
        for file in &report.transitive_impacts {
            out.push_str(&format!("  {} - {}\n", file.path.display(), file.reason));
        }
        out.push('\n');
    }

    // Blast radius summary
    let severity_str = match report.blast_radius.worst_severity {
        Some(Severity::Error) => "ERROR",
        Some(Severity::Warning) => "WARNING",
        Some(Severity::Info) => "INFO",
        None => "NONE",
    };

    if no_color {
        out.push_str(&format!(
            "Blast radius: {} file(s), {} concept(s), worst severity: {}\n",
            report.blast_radius.files_affected, report.blast_radius.concepts_affected, severity_str,
        ));
    } else {
        out.push_str(&format!(
            "{}: {} file(s), {} concept(s), worst severity: {}\n",
            "Blast radius".bold(),
            report.blast_radius.files_affected,
            report.blast_radius.concepts_affected,
            severity_str,
        ));
    }

    out
}

/// Render an impact report as JSON.
pub fn render_impact_json(report: &ImpactReport) -> String {
    let mut entries = Vec::new();
    for file in &report.root_changes {
        entries.push(serde_json::json!({
            "type": "root",
            "path": file.path.display().to_string(),
            "concepts": file.concept_ids,
            "reason": file.reason,
        }));
    }
    for file in &report.direct_impacts {
        entries.push(serde_json::json!({
            "type": "direct",
            "path": file.path.display().to_string(),
            "concepts": file.concept_ids,
            "reason": file.reason,
        }));
    }
    for file in &report.transitive_impacts {
        entries.push(serde_json::json!({
            "type": "transitive",
            "path": file.path.display().to_string(),
            "concepts": file.concept_ids,
            "reason": file.reason,
        }));
    }

    let report_json = serde_json::json!({
        "affected_concepts": report.affected_concepts,
        "blast_radius": {
            "files_affected": report.blast_radius.files_affected,
            "concepts_affected": report.blast_radius.concepts_affected,
            "worst_severity": report.blast_radius.worst_severity.map(|s| s.to_string()),
        },
        "files": entries,
    });

    serde_json::to_string_pretty(&report_json)
        .unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConflicConfig;
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
    fn test_impact_direct_peers() {
        let a1 = make_assertion("node-version", "a/.nvmrc", "20");
        let a2 = make_assertion("node-version", "a/Dockerfile", "18");

        let result = ScanResult {
            concept_results: vec![ConceptResult {
                concept: a1.concept.clone(),
                assertions: vec![a1, a2],
                findings: vec![],
            }],
            parse_diagnostics: vec![],
        };

        let changed = vec![PathBuf::from("a/.nvmrc")];
        let config = ConflicConfig::default();
        let report = analyze_impact(&result, &changed, Path::new("."), &config);

        assert_eq!(report.root_changes.len(), 1);
        assert_eq!(report.direct_impacts.len(), 1);
        assert_eq!(report.blast_radius.concepts_affected, 1);
    }

    #[test]
    fn test_impact_transitive_via_concept_rules() {
        let a1 = make_assertion("python-version", ".python-version", "3.12");
        let a2 = make_assertion("pip-version", "requirements.txt", "21.0");

        let result = ScanResult {
            concept_results: vec![
                ConceptResult {
                    concept: a1.concept.clone(),
                    assertions: vec![a1],
                    findings: vec![],
                },
                ConceptResult {
                    concept: a2.concept.clone(),
                    assertions: vec![a2],
                    findings: vec![],
                },
            ],
            parse_diagnostics: vec![],
        };

        let changed = vec![PathBuf::from(".python-version")];
        let mut config = ConflicConfig::default();
        config.concept_rule.push(crate::config::ConceptRuleConfig {
            id: "XCON001".into(),
            when: crate::config::ConceptRuleWhen {
                concept: "python-version".into(),
                matches: ">=3.12".into(),
            },
            then: crate::config::ConceptRuleThen {
                concept: "pip-version".into(),
                requires: ">=22.3".into(),
            },
            severity: "warning".into(),
            message: None,
        });

        let report = analyze_impact(&result, &changed, Path::new("."), &config);

        assert_eq!(report.root_changes.len(), 1);
        assert!(!report.transitive_impacts.is_empty());
        assert!(
            report
                .affected_concepts
                .contains(&"pip-version".to_string())
        );
    }

    #[test]
    fn test_impact_no_change_empty_report() {
        let result = ScanResult {
            concept_results: vec![],
            parse_diagnostics: vec![],
        };

        let changed: Vec<PathBuf> = vec![];
        let config = ConflicConfig::default();
        let report = analyze_impact(&result, &changed, Path::new("."), &config);

        assert!(report.root_changes.is_empty());
        assert!(report.direct_impacts.is_empty());
        assert_eq!(report.blast_radius.files_affected, 0);
    }
}
