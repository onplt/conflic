pub mod baseline;
pub mod cli;
pub mod config;
pub mod discover;
pub mod error;
pub mod extract;
pub mod fix;
#[cfg(feature = "lsp")]
pub mod lsp;
pub mod model;
pub mod parse;
mod pathing;
mod pipeline;
mod planning;
pub mod report;
pub mod solve;
mod workspace;

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use config::ConflicConfig;
use error::{ConflicError, GitError, Result};
use model::ScanResult;

pub use pipeline::{DoctorFileInfo, DoctorReport};
pub use workspace::{IncrementalScanKind, IncrementalScanStats, IncrementalWorkspace};

/// Run the full conflic scan pipeline on a directory.
pub fn scan(root: &Path, config: &ConflicConfig) -> Result<ScanResult> {
    let pipeline = pipeline::run_scan_pipeline(root, config, None);
    Ok(pipeline.full_scan_result(root, config))
}

/// Run the scan pipeline while substituting in-memory content for selected files.
pub fn scan_with_overrides(
    root: &Path,
    config: &ConflicConfig,
    content_overrides: &HashMap<PathBuf, String>,
) -> Result<ScanResult> {
    let pipeline = pipeline::run_scan_pipeline(root, config, Some(content_overrides));
    Ok(pipeline.full_scan_result(root, config))
}

/// Run a diff-scoped scan: only files in `changed_files` plus their concept peers.
pub fn scan_diff(
    root: &Path,
    config: &ConflicConfig,
    changed_files: &[PathBuf],
) -> Result<ScanResult> {
    let pipeline = pipeline::run_diff_scan_pipeline(root, config, changed_files);
    Ok(pipeline.full_scan_result(root, config))
}

/// Get changed files from git diff against a ref.
pub fn git_changed_files(root: &Path, git_ref: &str) -> Result<Vec<PathBuf>> {
    let diff_args = ["diff", "--name-only", git_ref, "--"];
    let untracked_args = ["ls-files", "--others", "--exclude-standard"];

    let mut files = BTreeSet::new();
    files.extend(git_command_path_lines(root, &diff_args)?);
    files.extend(git_command_path_lines(root, &untracked_args)?);

    Ok(files.into_iter().collect())
}

/// Run the scan pipeline in diagnostic mode, collecting intermediate data.
pub fn scan_doctor(root: &Path, config: &ConflicConfig) -> Result<DoctorReport> {
    let pipeline = pipeline::run_scan_pipeline(root, config, None);
    Ok(pipeline.into_doctor_report(root, config))
}

fn git_command_path_lines(root: &Path, args: &[&str]) -> Result<Vec<PathBuf>> {
    let command = format_command(args);
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|source| ConflicError::from(GitError::Spawn { command, source }))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ConflicError::from(GitError::CommandFailed {
            command: format_command(args),
            stderr: stderr.trim().to_string(),
        }));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| PathBuf::from(line.trim()))
        .collect())
}

fn format_command(args: &[&str]) -> String {
    args.join(" ")
}
