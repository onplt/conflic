use owo_colors::OwoColorize;
use std::path::Path;

use crate::fix::{FixPlan, FixProposal};

pub(super) fn render_dry_run(plan: &FixPlan, no_color: bool) -> String {
    let mut out = String::new();

    if plan.proposals.is_empty() && plan.unfixable.is_empty() {
        out.push_str("No fixes needed - all assertions are consistent.\n");
        return out;
    }

    out.push_str(&format!(
        "conflic v{} - fix preview\n\n",
        env!("CARGO_PKG_VERSION")
    ));

    let mut by_concept: std::collections::BTreeMap<String, Vec<&FixProposal>> =
        std::collections::BTreeMap::new();
    for proposal in &plan.proposals {
        by_concept
            .entry(proposal.concept.display_name.clone())
            .or_default()
            .push(proposal);
    }

    for (concept_name, proposals) in &by_concept {
        if no_color {
            out.push_str(&format!("{}\n", concept_name));
        } else {
            out.push_str(&format!("{}\n", concept_name.bold()));
        }

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
                out.push_str(&format!(
                    "  {}\n",
                    format!("- {}", proposal.current_raw).red()
                ));
                out.push_str(&format!(
                    "  {}\n",
                    format!("+ {}", proposal.proposed_raw).green()
                ));
            }
            out.push('\n');
        }
    }

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
            out.push_str(&format!(
                "  {}: {}\n",
                item.concept.display_name, item.reason
            ));
        }
        out.push('\n');
    }

    let separator = "-".repeat(50);
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

    if !plan.proposals.is_empty() {
        out.push_str("Run `conflic fix` to apply.\n");
    }

    out
}

fn simplify_path(path: &Path) -> String {
    if let Ok(cwd) = std::env::current_dir()
        && let Ok(relative) = path.strip_prefix(&cwd)
    {
        return relative.to_string_lossy().to_string();
    }
    path.to_string_lossy().to_string()
}
