pub mod baseline;
pub mod cli;
pub mod config;
pub mod discover;
pub mod extract;
pub mod fix;
#[cfg(feature = "lsp")]
pub mod lsp;
pub mod model;
pub mod parse;
pub mod report;
pub mod solve;

use anyhow::Result;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use config::ConflicConfig;
use discover::FileDiscoverer;
use extract::Extractor;
use model::{ConfigAssertion, ScanResult};

/// Run the full conflic scan pipeline on a directory.
pub fn scan(root: &Path, config: &ConflicConfig) -> Result<ScanResult> {
    // 1. Discover config files
    let discoverer = FileDiscoverer::new(root, config.conflic.exclude.clone());
    let file_map = discoverer.discover();

    // 2. Get extractors
    let extractors = extract::build_extractors(config);

    // 3. Flatten file map into (filename, path) pairs for parallel processing
    let file_pairs: Vec<(&String, &PathBuf)> = file_map
        .iter()
        .flat_map(|(filename, paths)| paths.iter().map(move |p| (filename, p)))
        .collect();

    // 4. Parse files and extract assertions in parallel
    let results: Vec<Result<Vec<ConfigAssertion>, String>> = file_pairs
        .par_iter()
        .filter_map(|(filename, path)| {
            let relevant: Vec<&Box<dyn Extractor>> = extractors
                .iter()
                .filter(|ext| ext.matches_file(filename))
                .collect();

            if relevant.is_empty() {
                return None;
            }

            Some(match parse::parse_file(path) {
                Ok(parsed) => {
                    let mut assertions = Vec::new();
                    for ext in relevant {
                        assertions.extend(ext.extract(&parsed));
                    }
                    Ok(assertions)
                }
                Err(e) => Err(e.to_string()),
            })
        })
        .collect();

    // Collect results
    let mut all_assertions = Vec::new();
    let mut parse_errors = Vec::new();
    for result in results {
        match result {
            Ok(assertions) => all_assertions.extend(assertions),
            Err(e) => parse_errors.push(e),
        }
    }

    // 5. Compare assertions and find contradictions
    let concept_results = solve::compare_assertions(all_assertions, config);

    Ok(ScanResult {
        concept_results,
        parse_errors,
    })
}

/// Run a diff-scoped scan: only files in `changed_files` plus their concept peers.
pub fn scan_diff(
    root: &Path,
    config: &ConflicConfig,
    changed_files: &[PathBuf],
) -> Result<ScanResult> {
    // First do a full discovery and extraction
    let discoverer = FileDiscoverer::new(root, config.conflic.exclude.clone());
    let file_map = discoverer.discover();
    let extractors = extract::build_extractors(config);

    let mut all_assertions = Vec::new();
    let mut parse_errors = Vec::new();
    let mut changed_concepts = std::collections::HashSet::new();

    // Normalize changed file paths: resolve to absolute from root
    let changed_normalized: Vec<PathBuf> = changed_files
        .iter()
        .map(|p| {
            let full = if p.is_absolute() {
                p.clone()
            } else {
                root.join(p)
            };
            std::fs::canonicalize(&full).unwrap_or(full)
        })
        .collect();

    let is_changed_file = |path: &Path| -> bool {
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        changed_normalized.iter().any(|c| *c == canonical)
    };

    // First pass: extract from changed files and collect their concepts
    for (filename, paths) in &file_map {
        for path in paths {
            let is_changed = is_changed_file(path);
            if !is_changed {
                continue;
            }

            let relevant: Vec<&Box<dyn Extractor>> = extractors
                .iter()
                .filter(|ext| ext.matches_file(filename))
                .collect();

            if relevant.is_empty() {
                continue;
            }

            match parse::parse_file(path) {
                Ok(parsed) => {
                    for ext in &relevant {
                        let assertions = ext.extract(&parsed);
                        for a in &assertions {
                            changed_concepts.insert(a.concept.id.clone());
                        }
                        all_assertions.extend(assertions);
                    }
                }
                Err(e) => {
                    parse_errors.push(e.to_string());
                }
            }
        }
    }

    // Second pass: extract from peer files (same concept, different files)
    for (filename, paths) in &file_map {
        for path in paths {
            let is_changed = is_changed_file(path);
            if is_changed {
                continue; // Already processed
            }

            let relevant: Vec<&Box<dyn Extractor>> = extractors
                .iter()
                .filter(|ext| ext.matches_file(filename))
                .collect();

            if relevant.is_empty() {
                continue;
            }

            match parse::parse_file(path) {
                Ok(parsed) => {
                    for ext in &relevant {
                        let assertions = ext.extract(&parsed);
                        // Only keep assertions for concepts that changed files touch
                        let peer_assertions: Vec<_> = assertions
                            .into_iter()
                            .filter(|a| changed_concepts.contains(&a.concept.id))
                            .collect();
                        all_assertions.extend(peer_assertions);
                    }
                }
                Err(e) => {
                    parse_errors.push(e.to_string());
                }
            }
        }
    }

    let concept_results = solve::compare_assertions(all_assertions, config);

    Ok(ScanResult {
        concept_results,
        parse_errors,
    })
}

/// Get changed files from git diff against a ref.
pub fn git_changed_files(root: &Path, git_ref: &str) -> Result<Vec<PathBuf>> {
    let output = std::process::Command::new("git")
        .args(["diff", "--name-only", git_ref])
        .current_dir(root)
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("git diff failed: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let files: Vec<PathBuf> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| PathBuf::from(l.trim()))
        .collect();

    Ok(files)
}

/// Diagnostic info about a discovered file.
#[derive(Debug)]
pub struct DoctorFileInfo {
    pub path: PathBuf,
    pub filename: String,
    pub matched_extractors: Vec<String>,
    pub assertions: Vec<ConfigAssertion>,
    pub parse_error: Option<String>,
}

/// Full diagnostic report for `--doctor` mode.
#[derive(Debug)]
pub struct DoctorReport {
    pub root: PathBuf,
    pub discovered_files: HashMap<String, Vec<PathBuf>>,
    pub file_details: Vec<DoctorFileInfo>,
    pub scan_result: ScanResult,
    pub extractor_count: usize,
    pub extractor_names: Vec<(String, String)>,
}

/// Run the scan pipeline in diagnostic mode, collecting intermediate data.
pub fn scan_doctor(root: &Path, config: &ConflicConfig) -> Result<DoctorReport> {
    let discoverer = FileDiscoverer::new(root, config.conflic.exclude.clone());
    let file_map = discoverer.discover();
    let extractors = extract::build_extractors(config);

    let extractor_names: Vec<(String, String)> = extractors
        .iter()
        .map(|e| (e.id().to_string(), e.description().to_string()))
        .collect();
    let extractor_count = extractors.len();

    let mut all_assertions = Vec::new();
    let mut parse_errors = Vec::new();
    let mut file_details = Vec::new();

    for (filename, paths) in &file_map {
        for path in paths {
            let relevant: Vec<&Box<dyn Extractor>> = extractors
                .iter()
                .filter(|ext| ext.matches_file(filename))
                .collect();

            let matched_extractors: Vec<String> =
                relevant.iter().map(|e| e.id().to_string()).collect();

            let mut file_assertions = Vec::new();
            let mut file_error = None;

            if !relevant.is_empty() {
                match parse::parse_file(path) {
                    Ok(parsed) => {
                        for ext in &relevant {
                            let assertions = ext.extract(&parsed);
                            file_assertions.extend(assertions);
                        }
                    }
                    Err(e) => {
                        file_error = Some(e.to_string());
                        parse_errors.push(e.to_string());
                    }
                }
            }

            all_assertions.extend(file_assertions.clone());

            file_details.push(DoctorFileInfo {
                path: path.clone(),
                filename: filename.clone(),
                matched_extractors,
                assertions: file_assertions,
                parse_error: file_error,
            });
        }
    }

    let concept_results = solve::compare_assertions(all_assertions, config);
    let scan_result = ScanResult {
        concept_results,
        parse_errors,
    };

    Ok(DoctorReport {
        root: root.to_path_buf(),
        discovered_files: file_map,
        file_details,
        scan_result,
        extractor_count,
        extractor_names,
    })
}
