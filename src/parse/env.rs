use super::EnvEntry;

/// Parse a .env file into key-value entries.
pub fn parse_env(raw: &str) -> Vec<EnvEntry> {
    let mut entries = Vec::new();

    for (line_num, line) in raw.lines().enumerate() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Strip optional "export " prefix
        let stripped = trimmed.strip_prefix("export ").unwrap_or(trimmed);

        // Split on first '='
        if let Some((key, value)) = stripped.split_once('=') {
            let key = key.trim().to_string();
            let value = unquote(strip_inline_comment(value).trim());
            entries.push(EnvEntry {
                key,
                value,
                line: line_num + 1,
            });
        }
    }

    entries
}

fn unquote(s: &str) -> String {
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn strip_inline_comment(s: &str) -> &str {
    let mut in_single_quotes = false;
    let mut in_double_quotes = false;
    let mut escaped = false;
    let mut previous_is_whitespace = false;

    for (idx, ch) in s.char_indices() {
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
                return s[..idx].trim_end();
            }
            other => {
                previous_is_whitespace = other.is_whitespace();
            }
        }
    }

    s.trim_end()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_env() {
        let input = "# comment\nPORT=8080\nHOST=\"localhost\"\nexport DEBUG=true\n";
        let entries = parse_env(input);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].key, "PORT");
        assert_eq!(entries[0].value, "8080");
        assert_eq!(entries[1].key, "HOST");
        assert_eq!(entries[1].value, "localhost");
        assert_eq!(entries[2].key, "DEBUG");
        assert_eq!(entries[2].value, "true");
    }

    #[test]
    fn test_parse_env_strips_unquoted_inline_comments() {
        let input = "PORT=8080 # app port\nAPP_PORT='9090' # quoted\n";
        let entries = parse_env(input);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].value, "8080");
        assert_eq!(entries[1].value, "9090");
    }

    #[test]
    fn test_parse_env_preserves_hashes_inside_quotes() {
        let input = "MESSAGE=\"hello # still here\"\n";
        let entries = parse_env(input);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].value, "hello # still here");
    }

    #[test]
    fn test_parse_env_preserves_hash_at_value_start() {
        // Bug 2: `CHANNEL=#general` was incorrectly stripped because
        // previous_is_whitespace started as true and value was pre-trimmed.
        let input = "CHANNEL=#general\n";
        let entries = parse_env(input);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].value, "#general");
    }

    #[test]
    fn test_parse_env_strips_comment_after_space_hash() {
        // `KEY=value #comment` should still strip the comment
        let input = "KEY=value #comment\n";
        let entries = parse_env(input);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].value, "value");
    }

    #[test]
    fn test_parse_env_space_then_hash_is_comment() {
        // `KEY= #comment` — value is empty, `#comment` is a comment
        let input = "KEY= #comment\n";
        let entries = parse_env(input);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].value, "");
    }
}
