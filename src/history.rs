use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::error::{ConflicError, Result};
use crate::model::{Finding, ScanResult};

/// Snapshot of a single scan for history tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSnapshot {
    /// Git commit SHA at the time of the scan.
    pub commit_sha: String,
    /// Author of the commit.
    pub author: String,
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// Summary counts.
    pub errors: usize,
    pub warnings: usize,
    pub info: usize,
    /// Per-concept finding summaries.
    pub findings: Vec<FindingRecord>,
}

/// Record of a single finding in a snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingRecord {
    pub rule_id: String,
    pub concept_id: String,
    pub severity: String,
    pub left_file: String,
    pub left_value: String,
    pub right_file: String,
    pub right_value: String,
}

/// Persistent scan history stored as a JSON file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScanHistory {
    pub snapshots: Vec<ScanSnapshot>,
}

/// Information about when a finding was introduced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlameInfo {
    pub commit_sha: String,
    pub author: String,
    pub timestamp: String,
}

/// Result of a trend analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendReport {
    pub snapshots: Vec<TrendEntry>,
    pub new_findings: Vec<FindingRecord>,
    pub resolved_findings: Vec<FindingRecord>,
}

/// A single entry in a trend report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendEntry {
    pub commit_sha: String,
    pub timestamp: String,
    pub errors: usize,
    pub warnings: usize,
    pub info: usize,
    pub total: usize,
}

const HISTORY_FILE: &str = ".conflic-history.json";

impl ScanHistory {
    /// Load history from the default location in the scan root.
    pub fn load(scan_root: &Path) -> Result<Self> {
        let path = scan_root.join(HISTORY_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&path).map_err(|source| {
            ConflicError::Config(crate::error::ConfigError::Read {
                path: path.clone(),
                source,
            })
        })?;

        serde_json::from_str(&content).map_err(|e| {
            ConflicError::Config(crate::error::ConfigError::Read {
                path,
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
            })
        })
    }

    /// Save history to the default location in the scan root.
    pub fn save(&self, scan_root: &Path) -> Result<()> {
        let path = scan_root.join(HISTORY_FILE);
        let content = serde_json::to_string_pretty(self).map_err(|e| {
            ConflicError::Config(crate::error::ConfigError::Read {
                path: path.clone(),
                source: std::io::Error::other(e.to_string()),
            })
        })?;

        std::fs::write(&path, content).map_err(|source| {
            ConflicError::Config(crate::error::ConfigError::Read { path, source })
        })
    }

    /// Record a new scan snapshot from the current scan result and git state.
    pub fn record_scan(&mut self, result: &ScanResult, scan_root: &Path) {
        let git_info = get_head_info(scan_root);

        let findings: Vec<FindingRecord> = result
            .concept_results
            .iter()
            .flat_map(|cr| {
                cr.findings.iter().map(|f| FindingRecord {
                    rule_id: f.rule_id.clone(),
                    concept_id: cr.concept.id.clone(),
                    severity: f.severity.to_string().to_lowercase(),
                    left_file: f.left.source.file.to_string_lossy().to_string(),
                    left_value: f.left.raw_value.clone(),
                    right_file: f.right.source.file.to_string_lossy().to_string(),
                    right_value: f.right.raw_value.clone(),
                })
            })
            .collect();

        let snapshot = ScanSnapshot {
            commit_sha: git_info.sha,
            author: git_info.author,
            timestamp: git_info.timestamp,
            errors: result.error_count(),
            warnings: result.warning_count(),
            info: result.info_count(),
            findings,
        };

        self.snapshots.push(snapshot);
    }

    /// Generate a trend report from the history.
    pub fn trend(&self) -> TrendReport {
        let snapshots: Vec<TrendEntry> = self
            .snapshots
            .iter()
            .map(|s| TrendEntry {
                commit_sha: s.commit_sha.clone(),
                timestamp: s.timestamp.clone(),
                errors: s.errors,
                warnings: s.warnings,
                info: s.info,
                total: s.errors + s.warnings + s.info,
            })
            .collect();

        // Compare last two snapshots for new/resolved findings
        let (new_findings, resolved_findings) = if self.snapshots.len() >= 2 {
            let prev = &self.snapshots[self.snapshots.len() - 2];
            let current = &self.snapshots[self.snapshots.len() - 1];
            diff_findings(&prev.findings, &current.findings)
        } else {
            (Vec::new(), Vec::new())
        };

        TrendReport {
            snapshots,
            new_findings,
            resolved_findings,
        }
    }
}

/// Use git blame to find when a specific line was introduced.
pub fn blame_line(scan_root: &Path, file: &Path, line: usize) -> Option<BlameInfo> {
    let output = std::process::Command::new("git")
        .args([
            "blame",
            "-L",
            &format!("{},{}", line, line),
            "--porcelain",
            "--",
        ])
        .arg(file)
        .current_dir(scan_root)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    parse_blame_output(&String::from_utf8_lossy(&output.stdout))
}

/// Annotate findings with blame information.
pub fn annotate_findings(result: &ScanResult, scan_root: &Path) -> HashMap<FindingKey, BlameInfo> {
    let mut annotations = HashMap::new();

    for cr in &result.concept_results {
        for finding in &cr.findings {
            let key = FindingKey::from_finding(finding);

            // Blame the left side (the assertion that should change)
            if let Some(blame) = blame_line(
                scan_root,
                &finding.left.source.file,
                finding.left.source.line,
            ) {
                annotations.insert(key, blame);
            }
        }
    }

    annotations
}

/// Filter scan results to only show findings introduced since a given git ref.
pub fn filter_since(
    result: &mut ScanResult,
    scan_root: &Path,
    since_ref: &str,
) -> Result<SinceReport> {
    let since_timestamp = get_commit_timestamp(scan_root, since_ref)?;
    let mut kept = 0usize;
    let mut filtered = 0usize;

    for cr in &mut result.concept_results {
        let before_len = cr.findings.len();
        cr.findings.retain(|finding| {
            if let Some(blame) = blame_line(
                scan_root,
                &finding.left.source.file,
                finding.left.source.line,
            ) && let (Ok(blame_ts), Ok(since_ts)) = (
                blame.timestamp.parse::<i64>(),
                since_timestamp.parse::<i64>(),
            ) {
                return blame_ts >= since_ts;
            }
            // If we can't determine, keep the finding
            true
        });
        let after_len = cr.findings.len();
        kept += after_len;
        filtered += before_len - after_len;
    }

    Ok(SinceReport { kept, filtered })
}

/// Summary of --since filtering.
#[derive(Debug)]
pub struct SinceReport {
    pub kept: usize,
    pub filtered: usize,
}

/// Key for deduplicating findings across snapshots.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct FindingKey {
    pub rule_id: String,
    pub left_file: String,
    pub right_file: String,
    pub left_value: String,
    pub right_value: String,
}

impl FindingKey {
    fn from_finding(finding: &Finding) -> Self {
        Self {
            rule_id: finding.rule_id.clone(),
            left_file: finding.left.source.file.to_string_lossy().to_string(),
            right_file: finding.right.source.file.to_string_lossy().to_string(),
            left_value: finding.left.raw_value.clone(),
            right_value: finding.right.raw_value.clone(),
        }
    }

    fn from_record(record: &FindingRecord) -> Self {
        Self {
            rule_id: record.rule_id.clone(),
            left_file: record.left_file.clone(),
            right_file: record.right_file.clone(),
            left_value: record.left_value.clone(),
            right_value: record.right_value.clone(),
        }
    }
}

fn diff_findings(
    prev: &[FindingRecord],
    current: &[FindingRecord],
) -> (Vec<FindingRecord>, Vec<FindingRecord>) {
    let prev_keys: std::collections::HashSet<FindingKey> =
        prev.iter().map(FindingKey::from_record).collect();
    let current_keys: std::collections::HashSet<FindingKey> =
        current.iter().map(FindingKey::from_record).collect();

    let new_findings: Vec<FindingRecord> = current
        .iter()
        .filter(|f| !prev_keys.contains(&FindingKey::from_record(f)))
        .cloned()
        .collect();

    let resolved_findings: Vec<FindingRecord> = prev
        .iter()
        .filter(|f| !current_keys.contains(&FindingKey::from_record(f)))
        .cloned()
        .collect();

    (new_findings, resolved_findings)
}

struct GitHeadInfo {
    sha: String,
    author: String,
    timestamp: String,
}

fn get_head_info(scan_root: &Path) -> GitHeadInfo {
    let output = std::process::Command::new("git")
        .args(["log", "-1", "--format=%H%n%an%n%ct"])
        .current_dir(scan_root)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let lines: Vec<&str> = stdout.trim().lines().collect();
            if lines.len() >= 3 {
                GitHeadInfo {
                    sha: lines[0].to_string(),
                    author: lines[1].to_string(),
                    timestamp: lines[2].to_string(),
                }
            } else {
                GitHeadInfo::default()
            }
        }
        _ => GitHeadInfo::default(),
    }
}

impl Default for GitHeadInfo {
    fn default() -> Self {
        Self {
            sha: "unknown".into(),
            author: "unknown".into(),
            timestamp: "0".into(),
        }
    }
}

fn get_commit_timestamp(scan_root: &Path, git_ref: &str) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["log", "-1", "--format=%ct", git_ref])
        .current_dir(scan_root)
        .output()
        .map_err(|source| {
            ConflicError::from(crate::error::GitError::Spawn {
                command: format!("git log -1 --format=%ct {}", git_ref),
                source,
            })
        })?;

    if !output.status.success() {
        return Err(ConflicError::from(crate::error::GitError::CommandFailed {
            command: format!("git log -1 --format=%ct {}", git_ref),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        }));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn parse_blame_output(output: &str) -> Option<BlameInfo> {
    let mut sha = String::new();
    let mut author = String::new();
    let mut timestamp = String::new();

    for line in output.lines() {
        if sha.is_empty() && line.len() >= 40 {
            // First line is the SHA
            sha = line.split_whitespace().next()?.to_string();
        } else if let Some(rest) = line.strip_prefix("author ") {
            author = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("author-time ") {
            timestamp = rest.to_string();
        }
    }

    if sha.is_empty() {
        return None;
    }

    Some(BlameInfo {
        commit_sha: sha,
        author,
        timestamp,
    })
}

/// Render a trend report for terminal output.
pub fn render_trend(trend: &TrendReport, no_color: bool) -> String {
    use owo_colors::OwoColorize;

    let mut out = String::new();

    out.push_str(&format!(
        "conflic v{} - trend report\n\n",
        env!("CARGO_PKG_VERSION")
    ));

    if trend.snapshots.is_empty() {
        out.push_str("No scan history found. Run `conflic --record` to start tracking.\n");
        return out;
    }

    // Table header
    out.push_str(&format!(
        "  {:<12} {:<20} {:>6} {:>6} {:>6} {:>6}\n",
        "Commit", "Timestamp", "Errors", "Warns", "Info", "Total"
    ));

    let separator = "-".repeat(70);
    if no_color {
        out.push_str(&separator);
    } else {
        out.push_str(&separator.dimmed().to_string());
    }
    out.push('\n');

    for entry in &trend.snapshots {
        let sha_short = if entry.commit_sha.len() > 10 {
            &entry.commit_sha[..10]
        } else {
            &entry.commit_sha
        };

        out.push_str(&format!(
            "  {:<12} {:<20} {:>6} {:>6} {:>6} {:>6}\n",
            sha_short, entry.timestamp, entry.errors, entry.warnings, entry.info, entry.total
        ));
    }

    out.push('\n');

    if !trend.new_findings.is_empty() {
        if no_color {
            out.push_str(&format!("New findings ({}):\n", trend.new_findings.len()));
        } else {
            out.push_str(&format!(
                "{}:\n",
                format!("New findings ({})", trend.new_findings.len()).red()
            ));
        }
        for f in &trend.new_findings {
            out.push_str(&format!(
                "  {} [{}]: {} vs {}\n",
                f.concept_id, f.rule_id, f.left_value, f.right_value
            ));
        }
        out.push('\n');
    }

    if !trend.resolved_findings.is_empty() {
        if no_color {
            out.push_str(&format!(
                "Resolved findings ({}):\n",
                trend.resolved_findings.len()
            ));
        } else {
            out.push_str(&format!(
                "{}:\n",
                format!("Resolved findings ({})", trend.resolved_findings.len()).green()
            ));
        }
        for f in &trend.resolved_findings {
            out.push_str(&format!(
                "  {} [{}]: {} vs {}\n",
                f.concept_id, f.rule_id, f.left_value, f.right_value
            ));
        }
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_blame_output() {
        let output = r#"abc1234567890def1234567890abc1234567890de 1 1 1
author John Doe
author-mail <john@example.com>
author-time 1700000000
author-tz +0000
committer John Doe
committer-mail <john@example.com>
committer-time 1700000000
committer-tz +0000
summary Initial commit
filename .nvmrc
	20
"#;
        let blame = parse_blame_output(output).unwrap();
        assert_eq!(
            blame.commit_sha,
            "abc1234567890def1234567890abc1234567890de"
        );
        assert_eq!(blame.author, "John Doe");
        assert_eq!(blame.timestamp, "1700000000");
    }

    #[test]
    fn test_diff_findings_detects_new_and_resolved() {
        let prev = vec![FindingRecord {
            rule_id: "VER001".into(),
            concept_id: "node-version".into(),
            severity: "error".into(),
            left_file: ".nvmrc".into(),
            left_value: "18".into(),
            right_file: "Dockerfile".into(),
            right_value: "20".into(),
        }];

        let current = vec![FindingRecord {
            rule_id: "VER001".into(),
            concept_id: "python-version".into(),
            severity: "warning".into(),
            left_file: ".python-version".into(),
            left_value: "3.10".into(),
            right_file: "pyproject.toml".into(),
            right_value: "3.12".into(),
        }];

        let (new, resolved) = diff_findings(&prev, &current);

        assert_eq!(new.len(), 1);
        assert_eq!(new[0].concept_id, "python-version");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].concept_id, "node-version");
    }

    #[test]
    fn test_scan_history_default_is_empty() {
        let history = ScanHistory::default();
        assert!(history.snapshots.is_empty());
    }

    #[test]
    fn test_trend_report_empty_history() {
        let history = ScanHistory::default();
        let trend = history.trend();
        assert!(trend.snapshots.is_empty());
        assert!(trend.new_findings.is_empty());
        assert!(trend.resolved_findings.is_empty());
    }

    #[test]
    fn test_trend_report_with_snapshots() {
        let history = ScanHistory {
            snapshots: vec![
                ScanSnapshot {
                    commit_sha: "abc123".into(),
                    author: "test".into(),
                    timestamp: "1700000000".into(),
                    errors: 2,
                    warnings: 1,
                    info: 0,
                    findings: vec![FindingRecord {
                        rule_id: "VER001".into(),
                        concept_id: "node-version".into(),
                        severity: "error".into(),
                        left_file: ".nvmrc".into(),
                        left_value: "18".into(),
                        right_file: "Dockerfile".into(),
                        right_value: "20".into(),
                    }],
                },
                ScanSnapshot {
                    commit_sha: "def456".into(),
                    author: "test".into(),
                    timestamp: "1700001000".into(),
                    errors: 0,
                    warnings: 0,
                    info: 0,
                    findings: vec![],
                },
            ],
        };

        let trend = history.trend();
        assert_eq!(trend.snapshots.len(), 2);
        assert_eq!(trend.resolved_findings.len(), 1);
        assert_eq!(trend.new_findings.len(), 0);
    }

    #[test]
    fn test_render_trend_no_history() {
        let trend = TrendReport {
            snapshots: vec![],
            new_findings: vec![],
            resolved_findings: vec![],
        };
        let output = render_trend(&trend, true);
        assert!(output.contains("No scan history found"));
    }

    #[test]
    fn test_render_trend_with_data() {
        let trend = TrendReport {
            snapshots: vec![TrendEntry {
                commit_sha: "abc1234567890".into(),
                timestamp: "1700000000".into(),
                errors: 1,
                warnings: 2,
                info: 0,
                total: 3,
            }],
            new_findings: vec![],
            resolved_findings: vec![],
        };
        let output = render_trend(&trend, true);
        assert!(output.contains("abc1234567"));
        assert!(output.contains("trend report"));
    }

    #[test]
    fn test_finding_key_dedup() {
        let f1 = FindingRecord {
            rule_id: "VER001".into(),
            concept_id: "node-version".into(),
            severity: "error".into(),
            left_file: "a".into(),
            left_value: "18".into(),
            right_file: "b".into(),
            right_value: "20".into(),
        };
        let f2 = f1.clone();
        assert_eq!(FindingKey::from_record(&f1), FindingKey::from_record(&f2));
    }
}
