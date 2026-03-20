use super::{FixPlan, FixProposal};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[path = "patcher/apply.rs"]
mod apply_impl;
#[path = "patcher/atomic.rs"]
mod atomic_impl;
#[path = "patcher/preview.rs"]
mod preview_impl;

/// Result of applying fixes.
#[derive(Debug)]
pub struct ApplyResult {
    pub files_modified: Vec<PathBuf>,
    pub files_backed_up: Vec<PathBuf>,
    pub errors: Vec<(PathBuf, String)>,
}

/// Apply fixes to files on disk.
pub fn apply_fixes(plan: &FixPlan, create_backup: bool) -> ApplyResult {
    let mut files_modified = Vec::new();
    let mut files_backed_up = Vec::new();
    let mut errors: Vec<(PathBuf, String)> = Vec::new();

    let mut by_file: HashMap<&Path, Vec<&FixProposal>> = HashMap::new();
    for proposal in &plan.proposals {
        by_file.entry(&proposal.file).or_default().push(proposal);
    }

    for (file_path, mut proposals) in by_file {
        proposals
            .sort_by_key(|proposal| std::cmp::Reverse(apply_impl::proposal_start_offset(proposal)));

        let content = match std::fs::read_to_string(file_path) {
            Ok(content) => content,
            Err(error) => {
                errors.push((
                    file_path.to_path_buf(),
                    format!("Failed to read: {}", error),
                ));
                continue;
            }
        };

        let mut modified = content.clone();
        let mut failed = None;

        for proposal in &proposals {
            match apply_impl::apply_fix_to_content(&modified, proposal) {
                Ok(next) => modified = next,
                Err(error) => {
                    failed = Some(error);
                    break;
                }
            }
        }

        if let Some(error) = failed {
            errors.push((file_path.to_path_buf(), error));
            continue;
        }

        if modified == content {
            continue;
        }

        if create_backup {
            let canonical =
                std::fs::canonicalize(file_path).unwrap_or_else(|_| file_path.to_path_buf());
            let backup_path = PathBuf::from(format!("{}.conflic.bak", canonical.display()));
            if let Err(error) = atomic_impl::write_file_atomically(&backup_path, content.as_bytes())
            {
                errors.push((
                    file_path.to_path_buf(),
                    format!("Failed to create backup: {}", error),
                ));
                continue;
            }
            files_backed_up.push(backup_path);
        }

        if let Err(error) = atomic_impl::write_file_atomically(file_path, modified.as_bytes()) {
            errors.push((
                file_path.to_path_buf(),
                format!("Failed to write: {}", error),
            ));
            continue;
        }
        files_modified.push(file_path.to_path_buf());
    }

    ApplyResult {
        files_modified,
        files_backed_up,
        errors,
    }
}

/// Render a dry-run preview of the fix plan.
pub fn render_dry_run(plan: &FixPlan, no_color: bool) -> String {
    preview_impl::render_dry_run(plan, no_color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fix::FixOperation;
    use crate::model::concept::{ConceptCategory, SemanticConcept};
    use crate::parse::FileFormat;

    fn make_proposal(
        line: usize,
        current_raw: &str,
        proposed_raw: &str,
        operation: FixOperation,
    ) -> FixProposal {
        FixProposal {
            file: PathBuf::from("test.json"),
            concept: SemanticConcept {
                id: "test".into(),
                display_name: "Test".into(),
                category: ConceptCategory::RuntimeVersion,
            },
            current_raw: current_raw.into(),
            proposed_raw: proposed_raw.into(),
            key_path: String::new(),
            line,
            authority_winner: "enforced".into(),
            winner_file: PathBuf::from("Dockerfile"),
            format: FileFormat::PlainText,
            operation,
        }
    }

    #[test]
    fn test_apply_json_fix_updates_target_path() {
        let content = "{\"engines\":{\"node\":\"18.0.0\"}}\n";
        let proposal = make_proposal(
            1,
            "18.0.0",
            "20.0.0",
            FixOperation::ReplaceJsonString {
                path: vec!["engines".into(), "node".into()],
                value: "20.0.0".into(),
            },
        );

        let result = apply_impl::apply_fix_to_content(content, &proposal).unwrap();
        assert_eq!(result, "{\"engines\":{\"node\":\"20.0.0\"}}\n");
    }

    #[test]
    fn test_apply_json_fix_preserves_comments_and_crlf() {
        let content = "{\r\n  // keep comment\r\n  \"engines\": {\r\n    \"node\": \"18.0.0\"\r\n  }\r\n}\r\n";
        let proposal = make_proposal(
            4,
            "18.0.0",
            "20.0.0",
            FixOperation::ReplaceJsonString {
                path: vec!["engines".into(), "node".into()],
                value: "20.0.0".into(),
            },
        );

        let result = apply_impl::apply_fix_to_content(content, &proposal).unwrap();
        assert!(result.contains("// keep comment"));
        assert!(result.contains("\"node\": \"20.0.0\""));
        assert!(result.contains("\r\n"));
        assert!(!result.contains("// keep comment\n"));
    }

    #[test]
    fn test_apply_tool_versions_fix_updates_only_target_line() {
        let content = "nodejs 18.0.0\nruby 3.2.2\n";
        let proposal = make_proposal(
            1,
            "18.0.0",
            "20.0.0",
            FixOperation::ReplaceToolVersionsValue {
                value: "20.0.0".into(),
            },
        );

        let result = apply_impl::apply_fix_to_content(content, &proposal).unwrap();
        assert_eq!(result, "nodejs 20.0.0\nruby 3.2.2\n");
    }

    #[test]
    fn test_apply_docker_from_fix_preserves_stage_alias() {
        let content = "FROM node:18-alpine AS build\nRUN npm ci\n";
        let proposal = make_proposal(
            1,
            "node:18-alpine AS build",
            "node:20-alpine AS build",
            FixOperation::ReplaceDockerFromArguments {
                arguments: "node:20-alpine AS build".into(),
            },
        );

        let result = apply_impl::apply_fix_to_content(content, &proposal).unwrap();
        assert_eq!(result, "FROM node:20-alpine AS build\nRUN npm ci\n");
    }

    #[test]
    fn test_apply_whole_file_fix_preserves_missing_trailing_newline() {
        let content = "18";
        let proposal = make_proposal(
            1,
            "18",
            "20",
            FixOperation::ReplaceWholeFileValue { value: "20".into() },
        );

        let result = apply_impl::apply_fix_to_content(content, &proposal).unwrap();
        assert_eq!(result, "20");
    }
}
