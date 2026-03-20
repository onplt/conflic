use super::FileContent;

/// Parse JSON content, falling back to json5 for JSONC (comments, trailing commas).
pub fn parse_json(raw: &str) -> Result<FileContent, String> {
    // Try standard JSON first
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(value) => Ok(FileContent::Json(value)),
        Err(_) => {
            // Fall back to json5 for JSONC support (tsconfig.json, etc.)
            match json5::from_str::<serde_json::Value>(raw) {
                Ok(value) => Ok(FileContent::Json(value)),
                Err(e) => Err(format!("Failed to parse JSON: {}", e)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_standard_json() {
        let input = r#"{"name": "test", "version": "1.0.0"}"#;
        let result = parse_json(input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_jsonc_with_comments() {
        let input = r#"{
            // This is a comment
            "compilerOptions": {
                "strict": true,
            }
        }"#;
        let result = parse_json(input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_invalid_json() {
        let input = "not json at all {{{";
        let result = parse_json(input);
        assert!(result.is_err());
    }
}
