use super::{FixPlan, FixProposal};
use crate::parse::FileFormat;
use owo_colors::OwoColorize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

    // Group proposals by file, then sort by line descending to avoid line-shift issues
    let mut by_file: HashMap<&Path, Vec<&FixProposal>> = HashMap::new();
    for proposal in &plan.proposals {
        by_file.entry(&proposal.file).or_default().push(proposal);
    }

    for (file_path, mut proposals) in by_file {
        // Sort by line descending so we patch from bottom to top
        proposals.sort_by(|a, b| b.line.cmp(&a.line));

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(e) => {
                errors.push((file_path.to_path_buf(), format!("Failed to read: {}", e)));
                continue;
            }
        };

        // Create backup
        if create_backup {
            let backup_path = PathBuf::from(format!(
                "{}.conflic.bak",
                file_path.display()
            ));
            if let Err(e) = std::fs::write(&backup_path, &content) {
                errors.push((
                    file_path.to_path_buf(),
                    format!("Failed to create backup: {}", e),
                ));
                continue;
            }
            files_backed_up.push(backup_path);
        }

        let mut modified = content.clone();

        for proposal in &proposals {
            modified = apply_single_fix(&modified, proposal);
        }

        if modified != content {
            if let Err(e) = std::fs::write(file_path, &modified) {
                errors.push((file_path.to_path_buf(), format!("Failed to write: {}", e)));
                continue;
            }
            files_modified.push(file_path.to_path_buf());
        }
    }

    ApplyResult {
        files_modified,
        files_backed_up,
        errors,
    }
}

/// Apply a single fix to file content, returning the modified content.
fn apply_single_fix(content: &str, proposal: &FixProposal) -> String {
    match proposal.format {
        FileFormat::PlainText => {
            // For plain text files (.nvmrc, .python-version, etc.), replace entire content
            format!("{}\n", proposal.proposed_raw)
        }
        FileFormat::Env => apply_env_fix(content, proposal),
        FileFormat::Json | FileFormat::Yaml | FileFormat::Toml => {
            apply_line_based_fix(content, proposal)
        }
        FileFormat::Dockerfile => apply_dockerfile_fix(content, proposal),
    }
}

/// Fix an ENV file: find KEY=value and replace the value portion.
fn apply_env_fix(content: &str, proposal: &FixProposal) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::with_capacity(lines.len());

    for line in &lines {
        if let Some((key, _)) = line.split_once('=') {
            if key.trim() == proposal.key_path {
                result.push(format!("{}={}", key, proposal.proposed_raw));
                continue;
            }
        }
        result.push(line.to_string());
    }

    let mut out = result.join("\n");
    if content.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Line-based fix for JSON/YAML/TOML: find the current value on the target line and replace it.
fn apply_line_based_fix(content: &str, proposal: &FixProposal) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::with_capacity(lines.len());

    for (i, line) in lines.iter().enumerate() {
        let line_num = i + 1;
        if line_num == proposal.line && line.contains(&proposal.current_raw) {
            result.push(line.replacen(&proposal.current_raw, &proposal.proposed_raw, 1));
        } else {
            result.push(line.to_string());
        }
    }

    let mut out = result.join("\n");
    if content.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Fix a Dockerfile: replace image tag in FROM line.
fn apply_dockerfile_fix(content: &str, proposal: &FixProposal) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::with_capacity(lines.len());

    for (i, line) in lines.iter().enumerate() {
        let line_num = i + 1;
        if line_num == proposal.line && line.contains(&proposal.current_raw) {
            result.push(line.replacen(&proposal.current_raw, &proposal.proposed_raw, 1));
        } else {
            result.push(line.to_string());
        }
    }

    let mut out = result.join("\n");
    if content.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Render a dry-run preview of the fix plan.
pub fn render_dry_run(plan: &FixPlan, no_color: bool) -> String {
    let mut out = String::new();

    if plan.proposals.is_empty() && plan.unfixable.is_empty() {
        out.push_str("No fixes needed — all assertions are consistent.\n");
        return out;
    }

    out.push_str(&format!(
        "conflic v{} — fix preview\n\n",
        env!("CARGO_PKG_VERSION")
    ));

    // Group proposals by concept
    let mut by_concept: std::collections::BTreeMap<String, Vec<&super::FixProposal>> =
        std::collections::BTreeMap::new();
    for p in &plan.proposals {
        by_concept
            .entry(p.concept.display_name.clone())
            .or_default()
            .push(p);
    }

    for (concept_name, proposals) in &by_concept {
        if no_color {
            out.push_str(&format!("{}\n", concept_name));
        } else {
            out.push_str(&format!("{}\n", concept_name.bold()));
        }

        // Show the authority winner
        if let Some(first) = proposals.first() {
            if no_color {
                out.push_str(&format!(
                    "  Authority winner: {}\n\n",
                    first.authority_winner
                ));
            } else {
                out.push_str(&format!(
                    "  Authority winner: {}\n\n",
                    first.authority_winner.green()
                ));
            }
        }

        for proposal in proposals {
            let rel_path = simplify_path(&proposal.file);
            let key_info = if proposal.key_path.is_empty() {
                String::new()
            } else {
                format!(" ({})", proposal.key_path)
            };

            if no_color {
                out.push_str(&format!("  {}:{}{}\n", rel_path, proposal.line, key_info));
                out.push_str(&format!("  - {}\n", proposal.current_raw));
                out.push_str(&format!("  + {}\n", proposal.proposed_raw));
            } else {
                out.push_str(&format!(
                    "  {}:{}{}\n",
                    rel_path.cyan(),
                    proposal.line,
                    key_info
                ));
                out.push_str(&format!("  {}\n", format!("- {}", proposal.current_raw).red()));
                out.push_str(&format!(
                    "  {}\n",
                    format!("+ {}", proposal.proposed_raw).green()
                ));
            }
            out.push('\n');
        }
    }

    // Show unfixable items
    if !plan.unfixable.is_empty() {
        if no_color {
            out.push_str("Unfixable (manual resolution needed):\n");
        } else {
            out.push_str(&format!(
                "{}\n",
                "Unfixable (manual resolution needed):".yellow()
            ));
        }
        for item in &plan.unfixable {
            out.push_str(&format!("  {}: {}\n", item.concept.display_name, item.reason));
        }
        out.push('\n');
    }

    // Summary
    let separator = "─".repeat(50);
    if no_color {
        out.push_str(&separator);
    } else {
        out.push_str(&separator.dimmed().to_string());
    }
    out.push('\n');

    out.push_str(&format!(
        "{} fix(es) proposed, {} unfixable\n",
        plan.proposals.len(),
        plan.unfixable.len(),
    ));

    if !plan.proposals.is_empty() && no_color {
        out.push_str("Run `conflic fix` to apply.\n");
    } else if !plan.proposals.is_empty() {
        out.push_str("Run `conflic fix` to apply.\n");
    }

    out
}

fn simplify_path(path: &Path) -> String {
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(rel) = path.strip_prefix(&cwd) {
            return rel.to_string_lossy().to_string();
        }
    }
    path.to_string_lossy().to_string()
}
