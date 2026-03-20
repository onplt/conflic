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
        let stripped = trimmed
            .strip_prefix("export ")
            .unwrap_or(trimmed);

        // Split on first '='
        if let Some((key, value)) = stripped.split_once('=') {
            let key = key.trim().to_string();
            let value = unquote(value.trim());
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
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
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
}
