use std::collections::HashMap;
use std::sync::LazyLock;
use tower_lsp::lsp_types::*;

use crate::fix::{FixOperation, FixPlan, FixProposal};
use regex::Regex;

static GO_MOD_DIRECTIVE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*go\s+)(\S+)(.*)$").unwrap());
static TOOL_VERSIONS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*\S+\s+)(\S+)(.*)$").unwrap());
static GEMFILE_RUBY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^(\s*ruby\s+['"])([^'"]+)(['"].*)$"#).unwrap());
static DOCKER_EXPOSE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*EXPOSE\s+)(.*)$").unwrap());

/// Generate code actions (quick fixes) from the fix plan for a given file and range.
pub fn proposals_to_code_actions(
    plan: &FixPlan,
    uri: &Url,
    range: &Range,
    open_document_text: Option<&str>,
) -> Vec<CodeAction> {
    let file_path = match uri.to_file_path() {
        Ok(path) => path,
        Err(_) => return vec![],
    };
    let normalized_file_path = crate::pathing::normalize_path(&file_path);
    let original_content = match open_document_text {
        Some(content) => content.to_string(),
        None => match std::fs::read_to_string(&file_path) {
            Ok(content) => content,
            Err(_) => return vec![],
        },
    };

    let mut actions = Vec::new();

    for proposal in &plan.proposals {
        if !crate::pathing::paths_equivalent(&proposal.file, &normalized_file_path) {
            continue;
        }

        let proposal_line = if proposal.line > 0 {
            (proposal.line - 1) as u32
        } else {
            0
        };

        if proposal_line < range.start.line || proposal_line > range.end.line {
            continue;
        }

        let Ok(edit) = proposal_to_text_edit(&original_content, proposal) else {
            continue;
        };

        let title = format!(
            "Fix {}: change '{}' to '{}'",
            proposal.concept.display_name, proposal.current_raw, proposal.proposed_raw
        );

        let mut changes = HashMap::new();
        changes.insert(uri.clone(), vec![edit]);

        actions.push(CodeAction {
            title,
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: None,
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }),
            ..Default::default()
        });
    }

    actions
}

fn proposal_to_text_edit(content: &str, proposal: &FixProposal) -> Result<TextEdit, String> {
    let replacement = match &proposal.operation {
        FixOperation::ReplaceWholeFileValue { value } => TextReplacement {
            start: 0,
            end: content.len(),
            replacement: replace_whole_file_value(content, value),
        },
        FixOperation::ReplaceEnvValue { key, value } => {
            env_value_replacement(content, proposal.line, key, value)?
        }
        FixOperation::ReplaceJsonString { path, value } => {
            json_text_replacement(content, path, value)?
        }
        FixOperation::ReplaceGoModVersion { value } => {
            regex_capture_replacement(content, proposal.line, &GO_MOD_DIRECTIVE_RE, 2, value)?
        }
        FixOperation::ReplaceToolVersionsValue { value } => {
            regex_capture_replacement(content, proposal.line, &TOOL_VERSIONS_RE, 2, value)?
        }
        FixOperation::ReplaceGemfileRubyVersion { value } => {
            regex_capture_replacement(content, proposal.line, &GEMFILE_RUBY_RE, 2, value)?
        }
        FixOperation::ReplaceTextRange { start, end, value } => TextReplacement {
            start: *start,
            end: *end,
            replacement: value.to_string(),
        },
        FixOperation::ReplaceDockerFromArguments { arguments } => {
            docker_from_replacement(content, proposal.line, arguments)?
        }
        FixOperation::ReplaceDockerExposeToken { current, value } => {
            docker_expose_token_replacement(content, proposal.line, current, value)?
        }
    };

    Ok(TextEdit {
        range: byte_range_to_lsp_range(content, replacement.start, replacement.end),
        new_text: replacement.replacement,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TextReplacement {
    start: usize,
    end: usize,
    replacement: String,
}

fn replace_whole_file_value(content: &str, value: &str) -> String {
    let mut out = value.to_string();
    if content.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn env_value_replacement(
    content: &str,
    line: usize,
    key: &str,
    value: &str,
) -> Result<TextReplacement, String> {
    let line_info = line_info(content, line)?;
    let replacement =
        crate::fix::patcher::rewrite_env_assignment_line(line_info.text, line, key, value)?;

    Ok(TextReplacement {
        start: line_info.start,
        end: line_info.end,
        replacement,
    })
}

fn json_text_replacement(
    content: &str,
    path: &[String],
    value: &str,
) -> Result<TextReplacement, String> {
    let replacement = crate::parse::json::json_string_replacement(content, path, value)?;
    Ok(TextReplacement {
        start: replacement.start,
        end: replacement.end,
        replacement: replacement.replacement,
    })
}

fn regex_capture_replacement(
    content: &str,
    line: usize,
    regex: &Regex,
    capture_group: usize,
    replacement: &str,
) -> Result<TextReplacement, String> {
    let line_info = line_info(content, line)?;
    let captures = regex
        .captures(line_info.text)
        .ok_or_else(|| format!("Line {} did not match the expected format", line))?;
    let capture = captures
        .get(capture_group)
        .ok_or_else(|| format!("Line {} is missing capture group {}", line, capture_group))?;

    Ok(TextReplacement {
        start: line_info.start + capture.start(),
        end: line_info.start + capture.end(),
        replacement: replacement.to_string(),
    })
}

fn docker_from_replacement(
    content: &str,
    line: usize,
    arguments: &str,
) -> Result<TextReplacement, String> {
    let Some((start, end)) = crate::parse::dockerfile::docker_instruction_offsets(content, line)
    else {
        return Err(format!(
            "Line {} is not a Dockerfile FROM instruction",
            line
        ));
    };

    let original = &content[start..end];
    if !original
        .split_whitespace()
        .next()
        .is_some_and(|keyword| keyword.eq_ignore_ascii_case("FROM"))
    {
        return Err(format!(
            "Line {} is not a Dockerfile FROM instruction",
            line
        ));
    }

    Ok(TextReplacement {
        start,
        end,
        replacement: format!("FROM {}", arguments),
    })
}

fn docker_expose_token_replacement(
    content: &str,
    line: usize,
    current: &str,
    value: &str,
) -> Result<TextReplacement, String> {
    let line_info = line_info(content, line)?;
    let captures = DOCKER_EXPOSE_RE
        .captures(line_info.text)
        .ok_or_else(|| format!("Line {} is not a Dockerfile EXPOSE instruction", line))?;
    let remainder = captures.get(2).unwrap();
    let text = remainder.as_str();

    let mut search_offset = 0;
    for token in text.split_whitespace() {
        let relative_index = text[search_offset..]
            .find(token)
            .ok_or_else(|| format!("Line {} did not contain EXPOSE token {}", line, current))?;
        let token_index = search_offset + relative_index;
        if token == current {
            return Ok(TextReplacement {
                start: line_info.start + remainder.start() + token_index,
                end: line_info.start + remainder.start() + token_index + token.len(),
                replacement: value.to_string(),
            });
        }
        search_offset = token_index + token.len();
    }

    Err(format!(
        "Line {} did not contain EXPOSE token {}",
        line, current
    ))
}

#[derive(Debug, Clone, Copy)]
struct LineInfo<'a> {
    start: usize,
    end: usize,
    text: &'a str,
}

fn line_info<'a>(content: &'a str, target_line: usize) -> Result<LineInfo<'a>, String> {
    if target_line == 0 {
        return Err("Target line 0 is out of range".into());
    }

    let bytes = content.as_bytes();
    let mut line = 1;
    let mut start = 0;
    let mut index = 0;

    while line < target_line && index < bytes.len() {
        match bytes[index] {
            b'\r' => {
                line += 1;
                index += 1;
                if index < bytes.len() && bytes[index] == b'\n' {
                    index += 1;
                }
                start = index;
            }
            b'\n' => {
                line += 1;
                index += 1;
                start = index;
            }
            _ => index += 1,
        }
    }

    if line != target_line || start > content.len() {
        return Err(format!("Target line {} is out of range", target_line));
    }

    let mut end = start;
    while end < bytes.len() && bytes[end] != b'\r' && bytes[end] != b'\n' {
        end += 1;
    }

    Ok(LineInfo {
        start,
        end,
        text: &content[start..end],
    })
}

fn byte_range_to_lsp_range(content: &str, start: usize, end: usize) -> Range {
    Range {
        start: byte_offset_to_position(content, start),
        end: byte_offset_to_position(content, end),
    }
}

fn byte_offset_to_position(content: &str, target_offset: usize) -> Position {
    let mut line = 0_u32;
    let mut character = 0_u32;
    let mut offset = 0_usize;

    for ch in content.chars() {
        if offset >= target_offset {
            break;
        }

        if ch == '\n' {
            line += 1;
            character = 0;
        } else if ch != '\r' {
            character += ch.len_utf16() as u32;
        }

        offset += ch.len_utf8();
    }

    Position { line, character }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::concept::{ConceptCategory, SemanticConcept};
    use crate::parse::FileFormat;
    use std::path::PathBuf;

    fn make_proposal(file: PathBuf) -> FixProposal {
        FixProposal {
            file,
            concept: SemanticConcept {
                id: "node-version".into(),
                display_name: "Node.js Version".into(),
                category: ConceptCategory::RuntimeVersion,
            },
            current_raw: "18.0.0".into(),
            proposed_raw: "20.0.0".into(),
            key_path: "engines.node".into(),
            line: 4,
            authority_winner: "enforced".into(),
            winner_file: PathBuf::from("Dockerfile"),
            format: FileFormat::Json,
            operation: FixOperation::ReplaceJsonString {
                path: vec!["engines".into(), "node".into()],
                value: "20.0.0".into(),
            },
        }
    }

    #[test]
    fn test_code_actions_include_targeted_replacement_text() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("package.json");
        std::fs::write(
            &file,
            "{\n  // comment\n  \"engines\": {\n    \"node\": \"18.0.0\"\n  }\n}\n",
        )
        .unwrap();

        let plan = FixPlan {
            proposals: vec![make_proposal(file.clone())],
            unfixable: vec![],
        };
        let uri = Url::from_file_path(&file).unwrap();
        let range = Range {
            start: Position {
                line: 3,
                character: 0,
            },
            end: Position {
                line: 3,
                character: u32::MAX,
            },
        };

        let actions = proposals_to_code_actions(&plan, &uri, &range, None);
        assert_eq!(actions.len(), 1);

        let changes = actions[0]
            .edit
            .as_ref()
            .and_then(|edit| edit.changes.as_ref())
            .and_then(|changes| changes.get(&uri))
            .unwrap();

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].new_text, "\"20.0.0\"");
        assert_eq!(changes[0].range.start.line, 3);
        assert_eq!(changes[0].range.end.line, 3);
        assert_ne!(
            changes[0].range.start,
            Position {
                line: 0,
                character: 0,
            }
        );
    }

    #[test]
    fn test_env_code_actions_preserve_inline_comments() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".env");
        std::fs::write(&file, "PORT=3000 # keep comment\nNAME=demo\n").unwrap();

        let plan = FixPlan {
            proposals: vec![FixProposal {
                file: file.clone(),
                concept: SemanticConcept {
                    id: "app-port".into(),
                    display_name: "Application Port".into(),
                    category: ConceptCategory::Port,
                },
                current_raw: "3000".into(),
                proposed_raw: "8080".into(),
                key_path: "PORT".into(),
                line: 1,
                authority_winner: "enforced".into(),
                winner_file: PathBuf::from("docker-compose.yml"),
                format: FileFormat::Env,
                operation: FixOperation::ReplaceEnvValue {
                    key: "PORT".into(),
                    value: "8080".into(),
                },
            }],
            unfixable: vec![],
        };
        let uri = Url::from_file_path(&file).unwrap();
        let range = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: u32::MAX,
            },
        };

        let actions = proposals_to_code_actions(&plan, &uri, &range, None);
        assert_eq!(actions.len(), 1);

        let changes = actions[0]
            .edit
            .as_ref()
            .and_then(|edit| edit.changes.as_ref())
            .and_then(|changes| changes.get(&uri))
            .unwrap();

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].new_text, "PORT=8080 # keep comment");
        assert_eq!(changes[0].range.start.line, 0);
        assert_eq!(changes[0].range.end.line, 0);
    }
}
