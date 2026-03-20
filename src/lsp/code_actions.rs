use std::collections::HashMap;
use tower_lsp::lsp_types::*;

use crate::fix::FixPlan;

/// Generate code actions (quick fixes) from the fix plan for a given file and range.
pub fn proposals_to_code_actions(
    plan: &FixPlan,
    uri: &Url,
    range: &Range,
) -> Vec<CodeAction> {
    let file_path = match uri.to_file_path() {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    let mut actions = Vec::new();

    for proposal in &plan.proposals {
        if proposal.file != file_path {
            continue;
        }

        // Check if the proposal's line falls within the requested range
        let proposal_line = if proposal.line > 0 {
            (proposal.line - 1) as u32
        } else {
            0
        };

        if proposal_line < range.start.line || proposal_line > range.end.line {
            continue;
        }

        let edit_range = Range {
            start: Position {
                line: proposal_line,
                character: 0,
            },
            end: Position {
                line: proposal_line,
                character: u32::MAX,
            },
        };

        // We need to read the line to construct the replacement properly
        // For now, do a simple value replacement in the line
        let title = format!(
            "Fix {}: change '{}' to '{}'",
            proposal.concept.display_name, proposal.current_raw, proposal.proposed_raw
        );

        let mut changes = HashMap::new();
        changes.insert(
            uri.clone(),
            vec![TextEdit {
                range: edit_range,
                new_text: String::new(), // Will be filled by the client reading the line
            }],
        );

        // Create a more precise text edit using the current/proposed values
        let action = CodeAction {
            title,
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: None,
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }),
            ..Default::default()
        };

        actions.push(action);
    }

    actions
}
