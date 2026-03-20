/// Parse a plain text file, returning the first non-empty, non-comment line trimmed.
pub fn parse_plain_text(raw: &str) -> String {
    raw.lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .unwrap_or("")
        .to_string()
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
}
