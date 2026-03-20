use super::{FileContent, YamlValue};

pub fn parse_yaml(raw: &str) -> Result<FileContent, String> {
    let value: YamlValue =
        serde_saphyr::from_str(raw).map_err(|e| format!("Failed to parse YAML: {}", e))?;
    Ok(FileContent::Yaml(value))
}

pub fn inline_scalar_literal_for_key(raw: &str, line: usize, key: &str) -> Option<String> {
    let line = raw.lines().nth(line.checked_sub(1)?)?;
    let normalized = normalize_yaml_key_line(line);
    if normalized.starts_with('#') {
        return None;
    }

    let remainder = key_remainder(normalized, key)?;
    let remainder = strip_yaml_inline_comment(remainder.trim_start());
    let remainder = strip_yaml_prefix_tokens(remainder);
    parse_inline_yaml_scalar(remainder)
}

pub fn sequence_scalar_literal_for_key(
    raw: &str,
    key: &str,
    item_index: usize,
) -> Option<(usize, String)> {
    let lines: Vec<&str> = raw.lines().collect();

    for (line_index, line) in lines.iter().enumerate() {
        let normalized = normalize_yaml_key_line(line);
        if normalized.starts_with('#') {
            continue;
        }

        let Some(remainder) = key_remainder(normalized, key) else {
            continue;
        };
        let remainder = strip_yaml_inline_comment(remainder.trim_start());
        let remainder = strip_yaml_prefix_tokens(remainder);

        if let Some(literal) = nth_inline_sequence_scalar_literal(remainder, item_index) {
            return Some((line_index + 1, literal));
        }

        if remainder.trim().is_empty()
            && let Some((line, literal)) = nth_block_sequence_scalar_literal(
                &lines,
                line_index,
                indentation_width(line),
                item_index,
            )
        {
            return Some((line, literal));
        }
    }

    None
}

pub fn line_for_key_path(raw: &str, key_path: &str) -> Option<usize> {
    let segments: Vec<&str> = key_path
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.is_empty() {
        return None;
    }

    let lines: Vec<&str> = raw.lines().collect();
    let (root_start, root_indent) = first_block_start(&lines, 0, None)?;
    find_line_for_segments(&lines, &segments, root_start, root_indent)
}

fn normalize_yaml_key_line(line: &str) -> &str {
    let mut trimmed = line.trim_start();
    while let Some(stripped) = trimmed.strip_prefix("- ") {
        trimmed = stripped.trim_start();
    }
    trimmed
}

fn key_remainder<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    if let Some(remainder) = line.strip_prefix(key) {
        return remainder.trim_start().strip_prefix(':');
    }

    let single_quoted = format!("'{}'", key);
    if let Some(remainder) = line.strip_prefix(&single_quoted) {
        return remainder.trim_start().strip_prefix(':');
    }

    let double_quoted = format!("\"{}\"", key);
    line.strip_prefix(&double_quoted)?
        .trim_start()
        .strip_prefix(':')
}

fn strip_yaml_inline_comment(value: &str) -> &str {
    let mut in_single_quotes = false;
    let mut in_double_quotes = false;
    let mut escaped = false;
    let mut previous_is_whitespace = true;

    for (index, ch) in value.char_indices() {
        if escaped {
            escaped = false;
            previous_is_whitespace = false;
            continue;
        }

        match ch {
            '\\' if in_double_quotes => {
                escaped = true;
                previous_is_whitespace = false;
            }
            '\'' if !in_double_quotes => {
                in_single_quotes = !in_single_quotes;
                previous_is_whitespace = false;
            }
            '"' if !in_single_quotes => {
                in_double_quotes = !in_double_quotes;
                previous_is_whitespace = false;
            }
            '#' if !in_single_quotes && !in_double_quotes && previous_is_whitespace => {
                return value[..index].trim_end();
            }
            other => previous_is_whitespace = other.is_whitespace(),
        }
    }

    value.trim_end()
}

fn strip_yaml_prefix_tokens(mut value: &str) -> &str {
    loop {
        value = value.trim_start();
        match value.chars().next() {
            Some('!') | Some('&') => {
                let token_len = value.find(char::is_whitespace).unwrap_or(value.len());
                value = &value[token_len..];
            }
            _ => return value,
        }
    }
}

fn nth_inline_sequence_scalar_literal(value: &str, item_index: usize) -> Option<String> {
    let trimmed = value.trim();
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;

    let mut in_single_quotes = false;
    let mut in_double_quotes = false;
    let mut escaped = false;
    let mut bracket_depth = 0_usize;
    let mut brace_depth = 0_usize;
    let mut current_index = 0_usize;
    let mut start = 0_usize;

    for (index, ch) in inner.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }

        if in_single_quotes {
            if ch == '\'' {
                in_single_quotes = false;
            }
            continue;
        }

        if in_double_quotes {
            match ch {
                '\\' => escaped = true,
                '"' => in_double_quotes = false,
                _ => {}
            }
            continue;
        }

        match ch {
            '\'' => in_single_quotes = true,
            '"' => in_double_quotes = true,
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            ',' if bracket_depth == 0 && brace_depth == 0 => {
                if current_index == item_index {
                    return parse_sequence_item_literal(&inner[start..index]);
                }
                current_index += 1;
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }

    (current_index == item_index)
        .then(|| parse_sequence_item_literal(&inner[start..]))
        .flatten()
}

fn nth_block_sequence_scalar_literal(
    lines: &[&str],
    key_line_index: usize,
    key_indent: usize,
    item_index: usize,
) -> Option<(usize, String)> {
    let mut current_index = 0_usize;

    for (line_index, line) in lines.iter().enumerate().skip(key_line_index + 1) {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = indentation_width(line);
        if indent <= key_indent {
            break;
        }

        let Some(remainder) = trimmed.strip_prefix("- ") else {
            continue;
        };

        if current_index == item_index {
            let literal = parse_sequence_item_literal(remainder)?;
            return Some((line_index + 1, literal));
        }

        current_index += 1;
    }

    None
}

fn parse_sequence_item_literal(value: &str) -> Option<String> {
    let value = strip_yaml_inline_comment(value.trim_start());
    let value = strip_yaml_prefix_tokens(value);
    parse_inline_yaml_scalar(value)
}

fn parse_inline_yaml_scalar(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let first = trimmed.chars().next()?;

    if matches!(first, '[' | '{' | '|' | '>') {
        return None;
    }

    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        return Some(trimmed[1..trimmed.len() - 1].to_string());
    }

    Some(trimmed.to_string())
}

fn indentation_width(line: &str) -> usize {
    line.chars().take_while(|ch| ch.is_whitespace()).count()
}

fn find_line_for_segments(
    lines: &[&str],
    segments: &[&str],
    start_index: usize,
    block_indent: usize,
) -> Option<usize> {
    let mut index = start_index;
    let target = *segments.first()?;

    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim_start();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            index += 1;
            continue;
        }

        let indent = indentation_width(line);
        if indent < block_indent {
            break;
        }

        if indent > block_indent {
            index += 1;
            continue;
        }

        let normalized = normalize_yaml_key_line(line);
        if key_remainder(normalized, target).is_some() {
            if segments.len() == 1 {
                return Some(index + 1);
            }

            if let Some((child_start, child_indent)) =
                first_block_start(lines, index + 1, Some(block_indent))
                && let Some(line) =
                    find_line_for_segments(lines, &segments[1..], child_start, child_indent)
            {
                return Some(line);
            }
        }

        index = next_sibling_index(lines, index, block_indent);
    }

    None
}

fn first_block_start(
    lines: &[&str],
    start_index: usize,
    parent_indent: Option<usize>,
) -> Option<(usize, usize)> {
    for (index, line) in lines.iter().enumerate().skip(start_index) {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = indentation_width(line);
        if parent_indent.is_some_and(|parent| indent <= parent) {
            return None;
        }

        return Some((index, indent));
    }

    None
}

fn next_sibling_index(lines: &[&str], current_index: usize, current_indent: usize) -> usize {
    let mut index = current_index + 1;

    while index < lines.len() {
        let trimmed = lines[index].trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            index += 1;
            continue;
        }

        if indentation_width(lines[index]) <= current_indent {
            break;
        }

        index += 1;
    }

    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_yaml() {
        let input = "name: test\nversion: 1.0.0\n";
        let result = parse_yaml(input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_yaml_with_anchors() {
        let input =
            "defaults: &defaults\n  timeout: 30\nproduction:\n  <<: *defaults\n  port: 8080\n";
        let result = parse_yaml(input).unwrap();

        let FileContent::Yaml(value) = result else {
            panic!("expected YAML content");
        };

        assert_eq!(value["production"]["timeout"], 30);
        assert_eq!(value["production"]["port"], 8080);
    }

    #[test]
    fn test_parse_yaml_preserves_multiline_scalars_and_comment_like_content() {
        let input = "script: |\n  echo \"# not a comment\"\n  echo done\n# real comment\n";
        let result = parse_yaml(input).unwrap();

        let FileContent::Yaml(value) = result else {
            panic!("expected YAML content");
        };

        assert_eq!(
            value["script"].as_str(),
            Some("echo \"# not a comment\"\necho done\n")
        );
    }

    #[test]
    fn test_inline_scalar_literal_for_key_preserves_decimal_format() {
        let raw = "python-version: 3.10 # keep precision\n";

        assert_eq!(
            inline_scalar_literal_for_key(raw, 1, "python-version"),
            Some("3.10".to_string())
        );
    }

    #[test]
    fn test_inline_scalar_literal_for_key_supports_quoted_keys_and_values() {
        let raw = "'python-version': \"3.10\"\n";

        assert_eq!(
            inline_scalar_literal_for_key(raw, 1, "python-version"),
            Some("3.10".to_string())
        );
    }

    #[test]
    fn test_sequence_scalar_literal_for_key_preserves_inline_decimal_format() {
        let raw = "python-version: [3.10, '3.11'] # keep precision\n";

        assert_eq!(
            sequence_scalar_literal_for_key(raw, "python-version", 0),
            Some((1, "3.10".to_string()))
        );
        assert_eq!(
            sequence_scalar_literal_for_key(raw, "python-version", 1),
            Some((1, "3.11".to_string()))
        );
    }

    #[test]
    fn test_sequence_scalar_literal_for_key_supports_block_sequences() {
        let raw = "python-version:\n  - 3.10\n  - \"3.11\"\n";

        assert_eq!(
            sequence_scalar_literal_for_key(raw, "python-version", 0),
            Some((2, "3.10".to_string()))
        );
        assert_eq!(
            sequence_scalar_literal_for_key(raw, "python-version", 1),
            Some((3, "3.11".to_string()))
        );
    }

    #[test]
    fn test_line_for_key_path_uses_full_yaml_path() {
        let raw = "services:\n  web:\n    image: redis:7.2\n  redis:\n    image: redis:7.2\n";

        assert_eq!(line_for_key_path(raw, "services.redis.image"), Some(5));
    }
}
