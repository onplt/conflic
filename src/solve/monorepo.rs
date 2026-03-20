use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::ConflicConfig;
use crate::model::{ConfigAssertion, Finding};

use super::findings::{collect_findings_across, collect_findings_within, sort_findings};

pub(super) fn find_monorepo_contradictions(
    scan_root: &Path,
    assertions: &[ConfigAssertion],
    config: &ConflicConfig,
) -> Vec<Finding> {
    let mut findings = HashMap::new();

    let patterns: Vec<(String, regex::Regex)> = config
        .monorepo
        .package_roots
        .iter()
        .filter_map(
            |pattern| match regex::Regex::new(&glob_pattern_to_regex(pattern)) {
                Ok(regex) => Some((pattern.clone(), regex)),
                Err(error) => {
                    eprintln!(
                        "Warning: invalid monorepo package_root glob '{}': {}",
                        pattern, error
                    );
                    None
                }
            },
        )
        .collect();

    let package_of = |assertion: &ConfigAssertion| -> Option<String> {
        let relative_path = path_relative_to_root(&assertion.source.file, scan_root);
        let mut best_match: Option<String> = None;
        let mut accumulated = PathBuf::new();

        for component in relative_path.components() {
            accumulated.push(component);
            let accumulated_str = normalize_glob_path(&accumulated.to_string_lossy());

            if patterns
                .iter()
                .any(|(_pattern, regex)| regex.is_match(&accumulated_str))
            {
                let replace_best = best_match.as_ref().is_none_or(|current| {
                    package_root_specificity(&accumulated_str)
                        > package_root_specificity(current.as_str())
                });

                if replace_best {
                    best_match = Some(accumulated_str);
                }
            }
        }

        best_match
    };

    let mut by_package: HashMap<String, Vec<&ConfigAssertion>> = HashMap::new();
    let mut root_level: Vec<&ConfigAssertion> = Vec::new();

    for assertion in assertions {
        if let Some(package) = package_of(assertion) {
            by_package.entry(package).or_default().push(assertion);
        } else {
            root_level.push(assertion);
        }
    }

    for package_assertions in by_package.values() {
        collect_findings_within(package_assertions, config, &mut findings);
        collect_findings_across(package_assertions, &root_level, config, &mut findings);
    }

    collect_findings_within(&root_level, config, &mut findings);

    sort_findings(findings)
}

fn path_relative_to_root(path: &Path, scan_root: &Path) -> PathBuf {
    if let Ok(relative) = path.strip_prefix(scan_root) {
        return relative.to_path_buf();
    }

    if path.is_absolute() {
        let normalized_root =
            std::fs::canonicalize(scan_root).unwrap_or_else(|_| scan_root.to_path_buf());
        let normalized_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

        if let Ok(relative) = normalized_path.strip_prefix(&normalized_root) {
            return relative.to_path_buf();
        }
    }

    path.strip_prefix(scan_root).unwrap_or(path).to_path_buf()
}

fn normalize_glob_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn package_root_specificity(path: &str) -> (usize, usize) {
    (path.split('/').count(), path.len())
}

fn glob_pattern_to_regex(pattern: &str) -> String {
    let normalized = normalize_glob_path(pattern);
    let mut regex = String::from("^");
    let mut chars = normalized.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    regex.push_str(".*");
                } else {
                    regex.push_str("[^/]*");
                }
            }
            '?' => regex.push_str("[^/]"),
            other => regex.push_str(&regex::escape(&other.to_string())),
        }
    }

    regex.push('$');
    regex
}
