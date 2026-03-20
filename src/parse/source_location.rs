/// Find the line number (1-based) of a key in raw text.
/// This is a best-effort heuristic for locating keys in config files.
pub fn find_line_for_key(raw: &str, key: &str) -> usize {
    for (i, line) in raw.lines().enumerate() {
        if line.contains(key) {
            return i + 1;
        }
    }
    1 // fallback to line 1
}

/// Find line number for a JSON key path like "engines.node".
/// Searches for the last segment of the path.
pub fn find_line_for_json_key(raw: &str, key_path: &str) -> usize {
    let last_key = key_path.rsplit('.').next().unwrap_or(key_path);
    // Look for "key" pattern (quoted key in JSON)
    let quoted = format!("\"{}\"", last_key);
    find_line_for_key(raw, &quoted)
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
    fn test_find_line_for_json_key() {
        let raw = "{\n  \"name\": \"test\",\n  \"engines\": {\n    \"node\": \">=18\"\n  }\n}";
        assert_eq!(find_line_for_json_key(raw, "engines.node"), 4);
    }
}
