use super::FileContent;

#[path = "json/editor.rs"]
mod editor_impl;

use editor_impl::JsonEditor;

/// Parse JSON content, falling back to json5 for JSONC (comments, trailing commas).
pub fn parse_json(raw: &str) -> Result<FileContent, String> {
    parse_json_value(raw).map(FileContent::Json)
}

pub fn parse_json_value(raw: &str) -> Result<serde_json::Value, String> {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(value) => Ok(value),
        Err(json_error) => json5::from_str::<serde_json::Value>(raw).map_err(|json5_error| {
            format!("Failed to parse JSON: {}; {}", json_error, json5_error)
        }),
    }
}

pub fn replace_json_string_preserving_format(
    raw: &str,
    path: &[String],
    value: &str,
) -> Result<String, String> {
    let replacement = json_string_replacement(raw, path, value)?;
    let mut updated = String::with_capacity(
        raw.len() - (replacement.end - replacement.start) + replacement.replacement.len(),
    );
    updated.push_str(&raw[..replacement.start]);
    updated.push_str(&replacement.replacement);
    updated.push_str(&raw[replacement.end..]);
    Ok(updated)
}

pub fn json_value_offsets(raw: &str, path: &[String]) -> Result<Option<(usize, usize)>, String> {
    let editor = JsonEditor::new(raw);
    Ok(editor
        .find_value_span(path)?
        .map(|span| (span.start, span.end)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextReplacement {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

pub fn json_string_replacement(
    raw: &str,
    path: &[String],
    value: &str,
) -> Result<TextReplacement, String> {
    let parsed = parse_json_value(raw)?;
    let Some(slot) = json_path(&parsed, path) else {
        return Err(format!("JSON path {} not found", path.join(".")));
    };

    if !slot.is_string() {
        return Err(format!(
            "JSON path {} is not a string value",
            path.join(".")
        ));
    }

    let editor = JsonEditor::new(raw);
    let Some(span) = editor.find_string_value_span(path)? else {
        return Err(format!(
            "Failed to locate JSON string token for path {}",
            path.join(".")
        ));
    };

    Ok(TextReplacement {
        start: span.token_start,
        end: span.token_end,
        replacement: encode_string_literal(value, span.quote)?,
    })
}

fn json_path<'a>(
    mut value: &'a serde_json::Value,
    path: &[String],
) -> Option<&'a serde_json::Value> {
    for segment in path {
        value = value.get(segment)?;
    }
    Some(value)
}

fn encode_string_literal(value: &str, quote: char) -> Result<String, String> {
    match quote {
        '"' => serde_json::to_string(value)
            .map_err(|error| format!("Failed to encode JSON string literal: {}", error)),
        '\'' => {
            let mut encoded = String::with_capacity(value.len() + 2);
            encoded.push('\'');
            for ch in value.chars() {
                match ch {
                    '\\' => encoded.push_str("\\\\"),
                    '\'' => encoded.push_str("\\'"),
                    '\u{0008}' => encoded.push_str("\\b"),
                    '\u{000C}' => encoded.push_str("\\f"),
                    '\n' => encoded.push_str("\\n"),
                    '\r' => encoded.push_str("\\r"),
                    '\t' => encoded.push_str("\\t"),
                    control if control.is_control() => {
                        encoded.push_str(&format!("\\u{:04X}", control as u32))
                    }
                    other => encoded.push(other),
                }
            }
            encoded.push('\'');
            Ok(encoded)
        }
        other => Err(format!("Unsupported JSON string quote '{}'", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_line_endings_are_crlf(text: &str) -> bool {
        let bytes = text.as_bytes();
        for (index, byte) in bytes.iter().enumerate() {
            if *byte == b'\n' && (index == 0 || bytes[index - 1] != b'\r') {
                return false;
            }
        }
        true
    }

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

    #[test]
    fn test_replace_json_string_preserves_comments_and_crlf() {
        let input =
            "{\r\n  // keep comment\r\n  \"engines\": {\r\n    \"node\": \"18\"\r\n  }\r\n}\r\n";
        let output =
            replace_json_string_preserving_format(input, &["engines".into(), "node".into()], "20")
                .unwrap();

        assert!(output.contains("// keep comment"));
        assert!(output.contains("\"node\": \"20\""));
        assert!(all_line_endings_are_crlf(&output));
    }

    #[test]
    fn test_replace_json_string_handles_single_quoted_json5_strings() {
        let input = "{foo: {'bar': '18'}}";
        let output =
            replace_json_string_preserving_format(input, &["foo".into(), "bar".into()], "20")
                .unwrap();

        assert!(output.contains("'bar': '20'"));
    }

    #[test]
    fn test_json_value_offsets_ignore_comment_keys() {
        let input = "{\n  // \"node\": \"99\"\n  \"engines\": {\n    \"node\": \"18\"\n  }\n}\n";
        let offsets = json_value_offsets(input, &["engines".into(), "node".into()])
            .unwrap()
            .expect("engines.node offsets should exist");

        assert_eq!(&input[offsets.0..offsets.1], "\"18\"");
    }
}
