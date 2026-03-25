pub mod baseline;
pub mod cli;
pub mod config;
pub mod discover;
pub mod drift;
pub mod enrich;
pub mod error;
pub mod extract;
pub mod federation;
pub mod fix;
pub mod graph;
pub mod history;
pub mod impact;
#[cfg(feature = "lsp")]
pub mod lsp;
pub mod model;
pub mod parse;
pub(crate) mod pathing;
mod pipeline;
mod planning;
pub mod plugin;
pub mod policy;
pub mod promote;
pub mod report;
pub mod solve;
pub mod topology;
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
    validate_git_diff_ref(git_ref)?;

    let diff_args = ["diff", "--name-only", "-z", git_ref, "--"];
    let untracked_args = ["ls-files", "--others", "--exclude-standard", "-z"];

    let mut files = BTreeSet::new();
    files.extend(git_command_path_lines(root, &diff_args)?);
    files.extend(git_command_path_lines(root, &untracked_args)?);

    Ok(files.into_iter().collect())
}

/// Discover and parse files for topology analysis.
pub fn parse_files_for_topology(root: &Path, config: &ConflicConfig) -> Vec<parse::ParsedFile> {
    let discovered = planning::discover_files(root, config);
    let mut parsed = Vec::new();
    for paths in discovered.values() {
        for path in paths {
            if let Ok(file) = parse::parse_file(path, root) {
                parsed.push(file);
            }
        }
    }
    parsed
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

    Ok(parse_git_path_output(&output.stdout, args.contains(&"-z")))
}

fn format_command(args: &[&str]) -> String {
    args.join(" ")
}

fn validate_git_diff_ref(git_ref: &str) -> Result<()> {
    if git_ref.starts_with('-') {
        return Err(ConflicError::from(GitError::InvalidDiffRef {
            value: git_ref.to_string(),
        }));
    }

    Ok(())
}

fn parse_git_path_output(stdout: &[u8], nul_terminated: bool) -> Vec<PathBuf> {
    let parts: Vec<&[u8]> = if nul_terminated {
        stdout.split(|byte| *byte == b'\0').collect()
    } else {
        stdout.split(|byte| *byte == b'\n').collect()
    };

    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .map(String::from_utf8_lossy)
        .map(|path| {
            let path = if nul_terminated {
                path.as_ref()
            } else {
                path.trim_end_matches('\r')
            };
            PathBuf::from(path)
        })
        .collect()
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod hardening_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn run_git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "git {:?} failed\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn test_git_changed_files_rejects_option_like_ref() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        run_git(root, &["init"]);
        run_git(root, &["config", "user.email", "codex@example.com"]);
        run_git(root, &["config", "user.name", "Codex"]);
        std::fs::write(root.join(".nvmrc"), "20\n").unwrap();
        run_git(root, &["add", "."]);
        run_git(root, &["commit", "-m", "initial"]);

        let err = git_changed_files(root, "--output=owned.txt").unwrap_err();
        let owned_path = root.join("owned.txt");

        assert!(
            err.to_string().contains("must not start with '-'"),
            "unexpected error message: {err}"
        );
        assert!(
            !owned_path.exists(),
            "option-like refs must not be forwarded to git"
        );
    }

    #[test]
    fn test_git_changed_files_handles_unicode_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let nested = root.join("unicode-é");
        let package = nested.join("package.json");

        run_git(root, &["init"]);
        run_git(root, &["config", "user.email", "codex@example.com"]);
        run_git(root, &["config", "user.name", "Codex"]);
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(&package, r#"{"engines":{"node":"20"}}"#).unwrap();
        run_git(root, &["add", "."]);
        run_git(root, &["commit", "-m", "initial"]);

        std::fs::write(&package, r#"{"engines":{"node":"18"}}"#).unwrap();

        let changed = git_changed_files(root, "HEAD").unwrap();

        assert!(
            changed
                .iter()
                .any(|path| path == Path::new("unicode-é/package.json")),
            "unicode paths should round-trip through git diff output: {:?}",
            changed
        );
    }

    #[test]
    fn test_parse_git_path_output_supports_nul_terminated_entries() {
        let output = b"package.json\0unicode-\xC3\xA9/.nvmrc\0";
        let parsed = parse_git_path_output(output, true);

        assert_eq!(
            parsed,
            vec![
                PathBuf::from("package.json"),
                PathBuf::from("unicode-é/.nvmrc"),
            ]
        );
    }

    #[test]
    fn test_parse_git_path_output_preserves_significant_whitespace() {
        let output = b" pkg/settings.json\0trailing-space /.env\0";
        let parsed = parse_git_path_output(output, true);

        assert_eq!(
            parsed,
            vec![
                PathBuf::from(" pkg/settings.json"),
                PathBuf::from("trailing-space /.env"),
            ]
        );
    }
}
