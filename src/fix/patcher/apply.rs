use regex::Regex;

use crate::fix::{FixOperation, FixProposal};

pub(super) fn apply_fix_to_content(
    content: &str,
    proposal: &FixProposal,
) -> Result<String, String> {
    match &proposal.operation {
        FixOperation::ReplaceWholeFileValue { value } => {
            Ok(replace_whole_file_value(content, value))
        }
        FixOperation::ReplaceEnvValue { key, value } => {
            apply_env_fix(content, proposal.line, key, value)
        }
        FixOperation::ReplaceJsonString { path, value } => {
            apply_json_string_fix(content, path, value)
        }
        FixOperation::ReplaceGoModVersion { value } => {
            apply_go_mod_fix(content, proposal.line, value)
        }
        FixOperation::ReplaceToolVersionsValue { value } => {
            apply_tool_versions_fix(content, proposal.line, value)
        }
        FixOperation::ReplaceGemfileRubyVersion { value } => {
            apply_gemfile_fix(content, proposal.line, value)
        }
        FixOperation::ReplaceTextRange { start, end, value } => {
            apply_text_range_fix(content, *start, *end, value)
        }
        FixOperation::ReplaceDockerFromArguments { arguments } => {
            apply_docker_from_fix(content, proposal.line, arguments)
        }
        FixOperation::ReplaceDockerExposeToken { current, value } => {
            apply_docker_expose_fix(content, proposal.line, current, value)
        }
    }
}

pub(super) fn proposal_start_offset(proposal: &FixProposal) -> usize {
    match &proposal.operation {
        FixOperation::ReplaceTextRange { start, .. } => *start,
        _ => proposal.line,
    }
}

fn replace_whole_file_value(content: &str, value: &str) -> String {
    let mut out = value.to_string();
    if content.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn apply_env_fix(content: &str, line: usize, key: &str, value: &str) -> Result<String, String> {
    rewrite_target_line(content, line, |original| {
        super::rewrite_env_assignment_line(original, line, key, value)
    })
}

fn apply_json_string_fix(content: &str, path: &[String], value: &str) -> Result<String, String> {
    crate::parse::json::replace_json_string_preserving_format(content, path, value)
}

fn apply_go_mod_fix(content: &str, line: usize, value: &str) -> Result<String, String> {
    rewrite_target_line(content, line, |original| {
        let re = Regex::new(r"^(\s*go\s+)(\S+)(.*)$").unwrap();
        let Some(caps) = re.captures(original) else {
            return Err(format!("Line {} is not a go.mod go directive", line));
        };
        Ok(format!("{}{}{}", &caps[1], value, &caps[3]))
    })
}

fn apply_tool_versions_fix(content: &str, line: usize, value: &str) -> Result<String, String> {
    rewrite_target_line(content, line, |original| {
        let re = Regex::new(r"^(\s*\S+\s+)(\S+)(.*)$").unwrap();
        let Some(caps) = re.captures(original) else {
            return Err(format!("Line {} is not a .tool-versions entry", line));
        };
        Ok(format!("{}{}{}", &caps[1], value, &caps[3]))
    })
}

fn apply_gemfile_fix(content: &str, line: usize, value: &str) -> Result<String, String> {
    rewrite_target_line(content, line, |original| {
        let re = Regex::new(r#"^(\s*ruby\s+['"])([^'"]+)(['"].*)$"#).unwrap();
        let Some(caps) = re.captures(original) else {
            return Err(format!("Line {} is not a Gemfile ruby directive", line));
        };
        Ok(format!("{}{}{}", &caps[1], value, &caps[3]))
    })
}

fn apply_text_range_fix(
    content: &str,
    start: usize,
    end: usize,
    value: &str,
) -> Result<String, String> {
    if start > end || end > content.len() {
        return Err(format!(
            "text replacement range {}..{} is out of bounds for content length {}",
            start,
            end,
            content.len()
        ));
    }

    let mut out = String::with_capacity(content.len() - (end - start) + value.len());
    out.push_str(&content[..start]);
    out.push_str(value);
    out.push_str(&content[end..]);
    Ok(out)
}

fn apply_docker_from_fix(content: &str, line: usize, arguments: &str) -> Result<String, String> {
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

    apply_text_range_fix(content, start, end, &format!("FROM {}", arguments))
}

fn apply_docker_expose_fix(
    content: &str,
    line: usize,
    current: &str,
    value: &str,
) -> Result<String, String> {
    rewrite_target_line(content, line, |original| {
        let re = Regex::new(r"^(\s*EXPOSE\s+)(.*)$").unwrap();
        let Some(caps) = re.captures(original) else {
            return Err(format!(
                "Line {} is not a Dockerfile EXPOSE instruction",
                line
            ));
        };

        let tokens = caps[2]
            .split_whitespace()
            .map(|token| {
                if token == current {
                    value.to_string()
                } else {
                    token.to_string()
                }
            })
            .collect::<Vec<_>>();

        if !tokens.iter().any(|token| token == value) {
            return Err(format!(
                "Line {} did not contain EXPOSE token {}",
                line, current
            ));
        }

        Ok(format!("{}{}", &caps[1], tokens.join(" ")))
    })
}

fn rewrite_target_line<F>(
    content: &str,
    target_line: usize,
    mut rewrite: F,
) -> Result<String, String>
where
    F: FnMut(&str) -> Result<String, String>,
{
    let line_info = line_info(content, target_line)?;
    let replacement = rewrite(line_info.text)?;

    let mut out = String::with_capacity(
        content.len() - (line_info.end - line_info.start) + replacement.len(),
    );
    out.push_str(&content[..line_info.start]);
    out.push_str(&replacement);
    out.push_str(&content[line_info.end..]);
    Ok(out)
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
