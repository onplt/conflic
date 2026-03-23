use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::ConflicConfig;
use crate::error::{ConflicError, Result};
use crate::model::*;

/// Configuration for federated scanning across multiple repositories.
#[derive(Debug, Clone, Deserialize)]
pub struct FederationConfig {
    #[serde(default)]
    pub repository: Vec<RepositoryEntry>,
}

/// A single repository in the federation.
#[derive(Debug, Clone, Deserialize)]
pub struct RepositoryEntry {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub group: Option<String>,
}

/// Result of scanning a single repository in a federation.
#[derive(Debug, Clone, Serialize)]
pub struct RepoScanResult {
    pub name: String,
    pub group: Option<String>,
    pub path: String,
    pub errors: usize,
    pub warnings: usize,
    pub info: usize,
    pub scan_error: Option<String>,
}

/// Cross-repo drift: same concept has different values across repos in the same group.
#[derive(Debug, Clone, Serialize)]
pub struct CrossRepoDrift {
    pub concept_id: String,
    pub group: String,
    pub entries: Vec<DriftEntry>,
}

/// A single entry in a cross-repo drift finding.
#[derive(Debug, Clone, Serialize)]
pub struct DriftEntry {
    pub repo: String,
    pub file: String,
    pub value: String,
}

/// The complete federation report.
#[derive(Debug, Clone, Serialize)]
pub struct FederationReport {
    pub repo_results: Vec<RepoScanResult>,
    pub cross_repo_drift: Vec<CrossRepoDrift>,
    pub summary: FederationSummary,
}

/// Summary statistics for the federation scan.
#[derive(Debug, Clone, Serialize)]
pub struct FederationSummary {
    pub repos_scanned: usize,
    pub repos_with_errors: usize,
    pub total_errors: usize,
    pub total_warnings: usize,
    pub cross_repo_drifts: usize,
}

impl FederationConfig {
    /// Load federation config from a TOML file.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|source| {
            ConflicError::Config(crate::error::ConfigError::Read {
                path: path.to_path_buf(),
                source,
            })
        })?;

        toml::from_str(&content).map_err(|e| {
            ConflicError::Config(crate::error::ConfigError::Read {
                path: path.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
            })
        })
    }
}

/// Run federated scanning across all configured repositories.
pub fn run_federation(config_path: &Path) -> Result<FederationReport> {
    let fed_config = FederationConfig::load(config_path)?;
    let config_dir = config_path.parent().unwrap_or(Path::new("."));

    let mut repo_results = Vec::new();
    // Collect per-repo assertions keyed by (group, concept_id)
    let mut group_assertions: HashMap<String, HashMap<String, Vec<(String, ConfigAssertion)>>> =
        HashMap::new();

    for repo in &fed_config.repository {
        let repo_path = resolve_repo_path(config_dir, &repo.path);

        if !repo_path.exists() {
            repo_results.push(RepoScanResult {
                name: repo.name.clone(),
                group: repo.group.clone(),
                path: repo.path.clone(),
                errors: 0,
                warnings: 0,
                info: 0,
                scan_error: Some(format!(
                    "Repository path not found: {}",
                    repo_path.display()
                )),
            });
            continue;
        }

        let scan_config = ConflicConfig::load(&repo_path, None).unwrap_or_default();
        match crate::scan(&repo_path, &scan_config) {
            Ok(result) => {
                let repo_scan = RepoScanResult {
                    name: repo.name.clone(),
                    group: repo.group.clone(),
                    path: repo.path.clone(),
                    errors: result.error_count(),
                    warnings: result.warning_count(),
                    info: result.info_count(),
                    scan_error: None,
                };
                repo_results.push(repo_scan);

                // Collect assertions for cross-repo comparison
                if let Some(group) = &repo.group {
                    let group_map = group_assertions.entry(group.clone()).or_default();

                    for cr in &result.concept_results {
                        for assertion in &cr.assertions {
                            group_map
                                .entry(cr.concept.id.clone())
                                .or_default()
                                .push((repo.name.clone(), assertion.clone()));
                        }
                    }
                }
            }
            Err(e) => {
                repo_results.push(RepoScanResult {
                    name: repo.name.clone(),
                    group: repo.group.clone(),
                    path: repo.path.clone(),
                    errors: 0,
                    warnings: 0,
                    info: 0,
                    scan_error: Some(e.to_string()),
                });
            }
        }
    }

    // Detect cross-repo drift within groups
    let cross_repo_drift = detect_cross_repo_drift(&group_assertions);

    let repos_with_errors = repo_results
        .iter()
        .filter(|r| r.errors > 0 || r.scan_error.is_some())
        .count();
    let total_errors: usize = repo_results.iter().map(|r| r.errors).sum();
    let total_warnings: usize = repo_results.iter().map(|r| r.warnings).sum();

    let summary = FederationSummary {
        repos_scanned: repo_results.len(),
        repos_with_errors,
        total_errors,
        total_warnings,
        cross_repo_drifts: cross_repo_drift.len(),
    };

    Ok(FederationReport {
        repo_results,
        cross_repo_drift,
        summary,
    })
}

/// Detect cross-repo drift: same concept, different values across repos in the same group.
#[allow(clippy::type_complexity)]
fn detect_cross_repo_drift(
    group_assertions: &HashMap<String, HashMap<String, Vec<(String, ConfigAssertion)>>>,
) -> Vec<CrossRepoDrift> {
    let mut drifts = Vec::new();

    for (group, concepts) in group_assertions {
        for (concept_id, repo_assertions) in concepts {
            // Group by repo → pick the highest-authority value per repo
            let mut per_repo: HashMap<String, Vec<&ConfigAssertion>> = HashMap::new();
            for (repo_name, assertion) in repo_assertions {
                per_repo
                    .entry(repo_name.clone())
                    .or_default()
                    .push(assertion);
            }

            // Pick the best assertion per repo (highest authority)
            let mut best_per_repo: Vec<(String, &ConfigAssertion)> = Vec::new();
            for (repo_name, assertions) in &per_repo {
                let best = assertions.iter().max_by_key(|a| a.authority).unwrap();
                best_per_repo.push((repo_name.clone(), best));
            }

            if best_per_repo.len() < 2 {
                continue;
            }

            // Check if all values are the same
            let first_value = &best_per_repo[0].1.raw_value;
            let has_drift = best_per_repo
                .iter()
                .any(|(_, a)| a.raw_value != *first_value);

            if has_drift {
                let entries: Vec<DriftEntry> = best_per_repo
                    .iter()
                    .map(|(repo_name, assertion)| DriftEntry {
                        repo: repo_name.clone(),
                        file: assertion.source.file.to_string_lossy().to_string(),
                        value: assertion.raw_value.clone(),
                    })
                    .collect();

                drifts.push(CrossRepoDrift {
                    concept_id: concept_id.clone(),
                    group: group.clone(),
                    entries,
                });
            }
        }
    }

    drifts.sort_by(|a, b| a.group.cmp(&b.group).then(a.concept_id.cmp(&b.concept_id)));
    drifts
}

fn resolve_repo_path(config_dir: &Path, repo_path: &str) -> PathBuf {
    let path = PathBuf::from(repo_path);
    if path.is_absolute() {
        path
    } else {
        config_dir.join(path)
    }
}

/// Render a federation report for terminal output.
pub fn render_federation_report(report: &FederationReport, no_color: bool) -> String {
    use owo_colors::OwoColorize;

    let mut out = String::new();

    out.push_str(&format!(
        "conflic v{} - federation report\n\n",
        env!("CARGO_PKG_VERSION")
    ));

    // Per-repo results
    out.push_str(&format!(
        "  {:<25} {:<15} {:>6} {:>6} {:>6}  {}\n",
        "Repository", "Group", "Errors", "Warns", "Info", "Status"
    ));

    let separator = "-".repeat(85);
    if no_color {
        out.push_str(&separator);
    } else {
        out.push_str(&separator.dimmed().to_string());
    }
    out.push('\n');

    for repo in &report.repo_results {
        let group_str = repo.group.as_deref().unwrap_or("-");
        let status = if let Some(ref err) = repo.scan_error {
            if no_color {
                format!("FAILED: {}", err)
            } else {
                format!("{}", format!("FAILED: {}", err).red())
            }
        } else if repo.errors > 0 {
            if no_color {
                "HAS ERRORS".to_string()
            } else {
                "HAS ERRORS".red().to_string()
            }
        } else if repo.warnings > 0 {
            if no_color {
                "HAS WARNINGS".to_string()
            } else {
                "HAS WARNINGS".yellow().to_string()
            }
        } else if no_color {
            "OK".to_string()
        } else {
            "OK".green().to_string()
        };

        out.push_str(&format!(
            "  {:<25} {:<15} {:>6} {:>6} {:>6}  {}\n",
            repo.name, group_str, repo.errors, repo.warnings, repo.info, status
        ));
    }

    out.push('\n');

    // Cross-repo drift
    if !report.cross_repo_drift.is_empty() {
        if no_color {
            out.push_str(&format!(
                "Cross-repository drift ({} concept(s)):\n\n",
                report.cross_repo_drift.len()
            ));
        } else {
            out.push_str(&format!(
                "{}:\n\n",
                format!(
                    "Cross-repository drift ({} concept(s))",
                    report.cross_repo_drift.len()
                )
                .red()
            ));
        }

        for drift in &report.cross_repo_drift {
            out.push_str(&format!(
                "  {} [group: {}]\n",
                drift.concept_id, drift.group
            ));
            for entry in &drift.entries {
                out.push_str(&format!(
                    "    {} => {} ({})\n",
                    entry.repo, entry.value, entry.file
                ));
            }
            out.push('\n');
        }
    }

    // Summary
    if no_color {
        out.push_str(&format!(
            "Summary: {} repo(s) scanned, {} error(s), {} warning(s), {} cross-repo drift(s)\n",
            report.summary.repos_scanned,
            report.summary.total_errors,
            report.summary.total_warnings,
            report.summary.cross_repo_drifts
        ));
    } else {
        out.push_str(&format!(
            "{}: {} repo(s) scanned, {} error(s), {} warning(s), {} cross-repo drift(s)\n",
            "Summary".bold(),
            report.summary.repos_scanned,
            report.summary.total_errors,
            report.summary.total_warnings,
            report.summary.cross_repo_drifts
        ));
    }

    out
}

/// Render a federation report as JSON.
pub fn render_federation_json(report: &FederationReport) -> String {
    serde_json::to_string_pretty(report).unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e))
}

/// Generate a template federation config file.
pub fn generate_federation_template() -> String {
    r#"# Conflic Federation Configuration
# Scan multiple repositories and detect cross-repo drift.

[[repository]]
name = "api-gateway"
path = "../api-gateway"
group = "backend"

[[repository]]
name = "user-service"
path = "../user-service"
group = "backend"

# [[repository]]
# name = "web-app"
# path = "../web-app"
# group = "frontend"
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_federation_config_deserialize() {
        let toml_str = r#"
[[repository]]
name = "api"
path = "../api"
group = "backend"

[[repository]]
name = "web"
path = "../web"
"#;
        let config: FederationConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.repository.len(), 2);
        assert_eq!(config.repository[0].name, "api");
        assert_eq!(config.repository[0].group.as_deref(), Some("backend"));
        assert_eq!(config.repository[1].name, "web");
        assert!(config.repository[1].group.is_none());
    }

    #[test]
    fn test_detect_cross_repo_drift() {
        let mut group_assertions: HashMap<String, HashMap<String, Vec<(String, ConfigAssertion)>>> =
            HashMap::new();

        let node_version = SemanticConcept::node_version();

        let assertion_a = ConfigAssertion::new(
            node_version.clone(),
            SemanticType::Version(parse_version("18")),
            "18".into(),
            assertion::SourceLocation {
                file: PathBuf::from(".nvmrc"),
                line: 1,
                column: 0,
                key_path: String::new(),
            },
            assertion::Authority::Advisory,
            "test",
        );

        let assertion_b = ConfigAssertion::new(
            node_version.clone(),
            SemanticType::Version(parse_version("20")),
            "20".into(),
            assertion::SourceLocation {
                file: PathBuf::from(".nvmrc"),
                line: 1,
                column: 0,
                key_path: String::new(),
            },
            assertion::Authority::Advisory,
            "test",
        );

        group_assertions
            .entry("backend".into())
            .or_default()
            .entry("node-version".into())
            .or_default()
            .push(("api".into(), assertion_a));

        group_assertions
            .entry("backend".into())
            .or_default()
            .entry("node-version".into())
            .or_default()
            .push(("web".into(), assertion_b));

        let drifts = detect_cross_repo_drift(&group_assertions);
        assert_eq!(drifts.len(), 1);
        assert_eq!(drifts[0].concept_id, "node-version");
        assert_eq!(drifts[0].group, "backend");
        assert_eq!(drifts[0].entries.len(), 2);
    }

    #[test]
    fn test_no_drift_when_same_values() {
        let mut group_assertions: HashMap<String, HashMap<String, Vec<(String, ConfigAssertion)>>> =
            HashMap::new();

        let node_version = SemanticConcept::node_version();

        let make = |_repo: &str| {
            ConfigAssertion::new(
                node_version.clone(),
                SemanticType::Version(parse_version("20")),
                "20".into(),
                assertion::SourceLocation {
                    file: PathBuf::from(".nvmrc"),
                    line: 1,
                    column: 0,
                    key_path: String::new(),
                },
                assertion::Authority::Advisory,
                "test",
            )
        };

        group_assertions
            .entry("backend".into())
            .or_default()
            .entry("node-version".into())
            .or_default()
            .push(("api".into(), make("api")));

        group_assertions
            .entry("backend".into())
            .or_default()
            .entry("node-version".into())
            .or_default()
            .push(("web".into(), make("web")));

        let drifts = detect_cross_repo_drift(&group_assertions);
        assert!(drifts.is_empty());
    }

    #[test]
    fn test_resolve_repo_path_relative() {
        let config_dir = Path::new("/home/user/workspace");
        let resolved = resolve_repo_path(config_dir, "../api");
        assert_eq!(resolved, PathBuf::from("/home/user/workspace/../api"));
    }

    #[test]
    fn test_resolve_repo_path_absolute() {
        let config_dir = Path::new("/home/user/workspace");
        let resolved = resolve_repo_path(config_dir, "/opt/repos/api");
        assert_eq!(resolved, PathBuf::from("/opt/repos/api"));
    }

    #[test]
    fn test_render_federation_report_empty() {
        let report = FederationReport {
            repo_results: vec![],
            cross_repo_drift: vec![],
            summary: FederationSummary {
                repos_scanned: 0,
                repos_with_errors: 0,
                total_errors: 0,
                total_warnings: 0,
                cross_repo_drifts: 0,
            },
        };
        let output = render_federation_report(&report, true);
        assert!(output.contains("federation report"));
        assert!(output.contains("0 repo(s) scanned"));
    }

    #[test]
    fn test_render_federation_json() {
        let report = FederationReport {
            repo_results: vec![RepoScanResult {
                name: "api".into(),
                group: Some("backend".into()),
                path: "../api".into(),
                errors: 1,
                warnings: 0,
                info: 0,
                scan_error: None,
            }],
            cross_repo_drift: vec![],
            summary: FederationSummary {
                repos_scanned: 1,
                repos_with_errors: 1,
                total_errors: 1,
                total_warnings: 0,
                cross_repo_drifts: 0,
            },
        };
        let json = render_federation_json(&report);
        assert!(json.contains("\"name\": \"api\""));
        assert!(json.contains("\"repos_scanned\": 1"));
    }

    #[test]
    fn test_generate_federation_template() {
        let template = generate_federation_template();
        assert!(template.contains("[[repository]]"));
        assert!(template.contains("name ="));
        assert!(template.contains("path ="));
        assert!(template.contains("group ="));
    }

    #[test]
    fn test_federation_with_real_repos() {
        // Create two temp repos with different .nvmrc files
        let dir = tempfile::tempdir().unwrap();
        let repo_a = dir.path().join("repo-a");
        let repo_b = dir.path().join("repo-b");
        std::fs::create_dir_all(&repo_a).unwrap();
        std::fs::create_dir_all(&repo_b).unwrap();

        std::fs::write(repo_a.join(".nvmrc"), "18\n").unwrap();
        std::fs::write(repo_b.join(".nvmrc"), "20\n").unwrap();

        let _fed_config = FederationConfig {
            repository: vec![
                RepositoryEntry {
                    name: "repo-a".into(),
                    path: repo_a.to_string_lossy().to_string(),
                    group: Some("services".into()),
                },
                RepositoryEntry {
                    name: "repo-b".into(),
                    path: repo_b.to_string_lossy().to_string(),
                    group: Some("services".into()),
                },
            ],
        };

        // Write config to file and run
        let config_path = dir.path().join("federation.toml");
        let config_str = format!(
            r#"
[[repository]]
name = "repo-a"
path = "{}"
group = "services"

[[repository]]
name = "repo-b"
path = "{}"
group = "services"
"#,
            repo_a.to_string_lossy().replace('\\', "\\\\"),
            repo_b.to_string_lossy().replace('\\', "\\\\"),
        );
        std::fs::write(&config_path, &config_str).unwrap();

        let report = run_federation(&config_path).unwrap();
        assert_eq!(report.repo_results.len(), 2);
        assert!(
            report.repo_results.iter().all(|r| r.scan_error.is_none()),
            "Both repos should scan successfully"
        );

        // Should detect cross-repo drift in node-version
        assert!(
            !report.cross_repo_drift.is_empty(),
            "Should detect node-version drift between repos: {:?}",
            report.cross_repo_drift
        );
        assert_eq!(report.cross_repo_drift[0].concept_id, "node-version");
    }
}
