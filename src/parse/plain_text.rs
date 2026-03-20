/// Return the first non-empty, non-comment line and its 1-based line number.
pub fn first_meaningful_line(raw: &str) -> Option<(usize, String)> {
    raw.lines().enumerate().find_map(|(index, line)| {
        let trimmed = line.trim();
        (!trimmed.is_empty() && !trimmed.starts_with('#')).then(|| (index + 1, trimmed.to_string()))
    })
}

/// Parse a plain text file, returning the first non-empty, non-comment line trimmed.
pub fn parse_plain_text(raw: &str) -> String {
    first_meaningful_line(raw)
        .map(|(_, value)| value)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_nvmrc() {
        assert_eq!(parse_plain_text("20.11.0\n"), "20.11.0");
    }

    #[test]
    fn test_parse_with_comment() {
        assert_eq!(parse_plain_text("# comment\n20\n"), "20");
    }

    #[test]
    fn test_parse_with_whitespace() {
        assert_eq!(parse_plain_text("  3.12  \n"), "3.12");
    }

    #[test]
    fn test_first_meaningful_line_tracks_original_line_number() {
        assert_eq!(
            first_meaningful_line("# comment\n\n  20 \n"),
            Some((3, "20".to_string()))
        );
    }
}
