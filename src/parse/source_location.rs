use std::path::Path;

use crate::model::{SourceLocation, SourceSpan};

pub fn span_from_offsets(raw: &str, start: usize, end: usize) -> SourceSpan {
    let (line, column) = line_column_for_offset(raw, start);
    let (end_line, end_column) = line_column_for_offset(raw, end);

    SourceSpan {
        start,
        end,
        line,
        column,
        end_line,
        end_column,
    }
}

pub fn location_from_span(
    file: &Path,
    key_path: impl Into<String>,
    span: SourceSpan,
) -> SourceLocation {
    SourceLocation {
        file: file.to_path_buf(),
        line: span.line,
        column: span.column,
        key_path: key_path.into(),
    }
}

pub fn location_and_span_from_offsets(
    file: &Path,
    raw: &str,
    key_path: impl Into<String>,
    start: usize,
    end: usize,
) -> (SourceLocation, SourceSpan) {
    let span = span_from_offsets(raw, start, end);
    let location = location_from_span(file, key_path, span);
    (location, span)
}

pub fn json_value_location(
    file: &Path,
    raw: &str,
    key_path: &str,
) -> Option<(SourceLocation, SourceSpan)> {
    let path: Vec<String> = key_path.split('.').map(str::to_string).collect();
    let (start, end) = crate::parse::json::json_value_offsets(raw, &path)
        .ok()
        .flatten()?;
    Some(location_and_span_from_offsets(
        file, raw, key_path, start, end,
    ))
}

pub fn find_exact_text_span(raw: &str, needle: &str) -> Option<SourceSpan> {
    let (start, matched) = raw.match_indices(needle).next()?;
    Some(span_from_offsets(raw, start, start + matched.len()))
}

/// Find the line number (1-based) of a key in raw text.
/// This is a fallback for formats where parser-level spans are unavailable.
pub fn find_line_for_key(raw: &str, key: &str) -> usize {
    raw.lines()
        .enumerate()
        .find_map(|(index, line)| {
            let trimmed = normalized_line_for_key_match(line);
            if trimmed.starts_with('#') {
                return None;
            }

            line_matches_key(trimmed, key).then_some(index + 1)
        })
        .unwrap_or(1)
}

pub fn find_line_for_key_value(raw: &str, key: &str, value: &str) -> usize {
    let key_hint = key.rsplit('.').next().unwrap_or(key);

    raw.lines()
        .enumerate()
        .find_map(|(index, line)| {
            let trimmed = normalized_line_for_key_match(line);
            if trimmed.starts_with('#') {
                return None;
            }

            (line_matches_key(trimmed, key_hint) && line_matches_value(trimmed, value))
                .then_some(index + 1)
        })
        .unwrap_or_else(|| find_line_for_key(raw, key_hint))
}

/// Find line number for a JSON key path like "engines.node" using exact token spans.
pub fn find_line_for_json_key(raw: &str, key_path: &str) -> usize {
    let path: Vec<String> = key_path.split('.').map(str::to_string).collect();
    crate::parse::json::json_value_offsets(raw, &path)
        .ok()
        .flatten()
        .map(|(start, end)| span_from_offsets(raw, start, end).line)
        .unwrap_or(1)
}

pub fn line_column_for_offset(raw: &str, offset: usize) -> (usize, usize) {
    let clamped = offset.min(raw.len());
    let mut line = 1_usize;
    let mut column = 1_usize;

    for ch in raw[..clamped].chars() {
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += ch.len_utf16();
        }
    }

    (line, column)
}

fn line_matches_key(line: &str, key: &str) -> bool {
    let quoted_double = format!("\"{}\"", key);
    let quoted_single = format!("'{}'", key);

    line_matches_structured_key(line, &quoted_double)
        || line_matches_structured_key(line, &quoted_single)
        || line_matches_structured_key(line, key)
}

fn normalized_line_for_key_match(line: &str) -> &str {
    let mut trimmed = line.trim_start();
    while let Some(stripped) = trimmed.strip_prefix("- ") {
        trimmed = stripped.trim_start();
    }

    trimmed.strip_prefix("export ").unwrap_or(trimmed)
}

fn line_matches_structured_key(line: &str, key: &str) -> bool {
    let Some(remainder) = line.strip_prefix(key) else {
        return false;
    };

    let remainder = remainder.trim_start();
    remainder.starts_with(':') || remainder.starts_with('=')
}

fn line_matches_value(line: &str, value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }

    line.match_indices(trimmed)
        .any(|(start, matched)| value_match_is_delimited(line, start, matched.len()))
}

fn value_match_is_delimited(line: &str, start: usize, len: usize) -> bool {
    let before = line[..start].chars().next_back();
    let after = line[start + len..].chars().next();
    is_value_boundary(before) && is_value_boundary(after)
}

fn is_value_boundary(ch: Option<char>) -> bool {
    match ch {
        None => true,
        Some(ch) => {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '"' | '\'' | ':' | '=' | '[' | ']' | '{' | '}' | '(' | ')' | ',' | '-'
                )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_line_for_key() {
        let raw = "line1\nPORT=8080\nline3\n";
        assert_eq!(find_line_for_key(raw, "PORT"), 2);
    }

    #[test]
    fn test_find_line_for_key_ignores_comment_lines() {
        let raw = "# node-version: 99\njobs:\n  build:\n    node-version: 18\n";
        assert_eq!(find_line_for_key(raw, "node-version"), 4);
    }

    #[test]
    fn test_find_line_for_key_ignores_freeform_mentions() {
        let raw = "jobs:\n  build:\n    steps:\n      - run: echo node-version\n        with:\n          node-version: 18\n";
        assert_eq!(find_line_for_key(raw, "node-version"), 6);
    }

    #[test]
    fn test_find_line_for_key_value_uses_leaf_key_and_value() {
        let raw = "services:\n  web:\n    image: nginx:1.27\n  redis:\n    image: redis:7.2\n";
        assert_eq!(
            find_line_for_key_value(raw, "services.redis.image", "redis:7.2"),
            5
        );
    }

    #[test]
    fn test_find_line_for_json_key_uses_exact_value_span() {
        let raw = "{\n  // \"node\": \"comment\"\n  \"name\": \"test\",\n  \"engines\": {\n    \"node\": \">=18\"\n  }\n}";
        assert_eq!(find_line_for_json_key(raw, "engines.node"), 5);
    }

    #[test]
    fn test_span_from_offsets_tracks_multiline_columns() {
        let raw = "<tag>\n  value\n</tag>\n";
        let start = raw.find("value").unwrap();
        let end = start + "value".len();
        let span = span_from_offsets(raw, start, end);

        assert_eq!(span.line, 2);
        assert_eq!(span.column, 3);
        assert_eq!(span.end_line, 2);
        assert_eq!(span.end_column, 8);
    }
}
