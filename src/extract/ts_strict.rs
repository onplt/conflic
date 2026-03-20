use super::Extractor;
use crate::model::*;
use crate::parse::source_location::*;
use crate::parse::*;

// --- tsconfig.json strict mode ---

pub struct TsconfigStrictExtractor;

impl Extractor for TsconfigStrictExtractor {
    fn id(&self) -> &str {
        "ts-strict-tsconfig"
    }
    fn description(&self) -> &str {
        "TypeScript strict mode from tsconfig.json"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec!["tsconfig"]
    }

    fn matches_file(&self, filename: &str) -> bool {
        filename.starts_with("tsconfig") && filename.ends_with(".json")
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::Json(ref value) = file.content {
            // Resolve extends chain to get merged config
            let resolved = crate::parse::extends::resolve_json_extends(value, file);

            if let Some(strict) = resolved
                .get("compilerOptions")
                .and_then(|c| c.get("strict"))
                .and_then(|s| s.as_bool())
            {
                // Check if strict is in the current file or inherited
                let is_local = value
                    .get("compilerOptions")
                    .and_then(|c| c.get("strict"))
                    .is_some();

                let line = if is_local {
                    find_line_for_json_key(&file.raw_text, "compilerOptions.strict")
                } else {
                    find_line_for_json_key(&file.raw_text, "extends")
                };

                let key_path: String = if is_local {
                    "compilerOptions.strict".into()
                } else {
                    "compilerOptions.strict (inherited)".into()
                };
                let span = if is_local {
                    json_value_location(&file.path, &file.raw_text, "compilerOptions.strict")
                        .map(|(_, span)| span)
                } else {
                    json_value_location(&file.path, &file.raw_text, "extends").map(|(_, span)| span)
                };
                let source = span
                    .map(|span| location_from_span(&file.path, key_path.clone(), span))
                    .unwrap_or(SourceLocation {
                        file: file.path.clone(),
                        line,
                        column: 0,
                        key_path,
                    });

                return vec![
                    ConfigAssertion::new(
                        SemanticConcept::ts_strict_mode(),
                        SemanticType::Boolean(strict),
                        strict.to_string(),
                        source,
                        Authority::Enforced,
                        self.id(),
                    )
                    .with_optional_span(span),
                ];
            }
        }
        vec![]
    }
}

// --- ESLint strict-related rules ---

pub struct EslintStrictExtractor;

impl Extractor for EslintStrictExtractor {
    fn id(&self) -> &str {
        "ts-strict-eslint"
    }
    fn description(&self) -> &str {
        "TypeScript strict mode from ESLint config"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec![".eslintrc", "eslint.config."]
    }

    fn matches_file(&self, filename: &str) -> bool {
        filename.starts_with(".eslintrc") || filename.starts_with("eslint.config.")
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        match &file.content {
            FileContent::Json(value) => {
                let resolved = crate::parse::extends::resolve_structured_extends(value, file);
                extract_eslint_strict_from_value(&resolved, Some(value), file, self.id())
            }
            FileContent::Yaml(value) => {
                let resolved = crate::parse::extends::resolve_structured_extends(value, file);
                extract_eslint_strict_from_value(&resolved, Some(value), file, self.id())
            }
            FileContent::PlainText(_) if is_flat_eslint_config(file) => {
                extract_eslint_strict_from_flat_config(file, self.id())
            }
            _ => vec![],
        }
    }
}

fn extract_eslint_strict_from_flat_config(
    file: &ParsedFile,
    extractor_id: &str,
) -> Vec<ConfigAssertion> {
    match parse_eslint_flat_config(&file.raw_text) {
        Ok(value) => extract_eslint_strict_from_value(&value, Some(&value), file, extractor_id),
        Err(error) => {
            file.push_parse_diagnostic(crate::parse::parse_diagnostic(
                Severity::Error,
                file.path.clone(),
                crate::parse::PARSE_FILE_ERROR_RULE_ID,
                format!("Failed to parse eslint flat config: {}", error),
            ));
            vec![]
        }
    }
}

fn extract_eslint_strict_from_value(
    value: &serde_json::Value,
    local_value: Option<&serde_json::Value>,
    file: &ParsedFile,
    extractor_id: &str,
) -> Vec<ConfigAssertion> {
    match value {
        serde_json::Value::Object(map) => {
            let mut results = Vec::new();

            if let Some(rules) = map.get("rules").and_then(|rules| rules.as_object()) {
                results.extend(check_strict_rules_json(
                    rules,
                    local_value,
                    file,
                    extractor_id,
                ));
            }

            results
        }
        serde_json::Value::Array(entries) => {
            let local_entries = local_value.and_then(|local| local.as_array());
            let mut results = Vec::new();

            for (index, entry) in entries.iter().enumerate() {
                let local_entry = local_entries.and_then(|entries| entries.get(index));
                results.extend(extract_eslint_strict_from_value(
                    entry,
                    local_entry,
                    file,
                    extractor_id,
                ));
            }

            results
        }
        _ => vec![],
    }
}

fn check_strict_rules_json(
    rules: &serde_json::Map<String, serde_json::Value>,
    local_value: Option<&serde_json::Value>,
    file: &ParsedFile,
    extractor_id: &str,
) -> Vec<ConfigAssertion> {
    let strict_rules = [
        "@typescript-eslint/strict-boolean-expressions",
        "@typescript-eslint/strict-type-checked",
        "@typescript-eslint/no-explicit-any",
    ];

    let mut results = Vec::new();

    for rule_name in &strict_rules {
        if let Some(rule_val) = rules.get(*rule_name) {
            let is_off = match rule_val {
                serde_json::Value::String(s) => s == "off" || s == "0",
                serde_json::Value::Number(n) => n.as_i64() == Some(0),
                serde_json::Value::Bool(value) => !value,
                serde_json::Value::Array(arr) => arr.first().is_some_and(|v| {
                    v.as_str() == Some("off") || v.as_i64() == Some(0) || v.as_bool() == Some(false)
                }),
                _ => false,
            };

            if is_off {
                let is_local = local_value
                    .and_then(|value| value.get("rules"))
                    .and_then(|rules| rules.get(*rule_name))
                    .is_some();
                let inherited =
                    !is_local && local_value.is_some_and(|value| value.get("extends").is_some());
                let (line, key_path) = if inherited {
                    (
                        find_line_for_key(&file.raw_text, "extends"),
                        format!("rules.{} (inherited)", rule_name),
                    )
                } else {
                    (
                        find_line_for_key(&file.raw_text, rule_name),
                        format!("rules.{}", rule_name),
                    )
                };

                results.push(ConfigAssertion::new(
                    SemanticConcept::ts_strict_mode(),
                    SemanticType::Boolean(false),
                    format!("{}: off", rule_name),
                    SourceLocation {
                        file: file.path.clone(),
                        line,
                        column: 0,
                        key_path,
                    },
                    Authority::Enforced,
                    extractor_id,
                ));
            }
        }
    }

    results
}

fn is_flat_eslint_config(file: &ParsedFile) -> bool {
    file.path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("eslint.config."))
}

fn parse_eslint_flat_config(raw: &str) -> Result<serde_json::Value, String> {
    let exported = exported_expression(raw);
    let normalized = unwrap_flat_config_wrappers(trim_trailing_semicolons(exported.trim()))?;
    json5::from_str(normalized).map_err(|error| {
        format!(
            "JSON5 decode failed for exported config expression: {}",
            error
        )
    })
}

fn exported_expression(raw: &str) -> &str {
    let trimmed = raw.trim();

    if let Some(index) = trimmed.find("export default") {
        return &trimmed[index + "export default".len()..];
    }

    if let Some(index) = trimmed.find("module.exports") {
        let remainder = &trimmed[index + "module.exports".len()..];
        let remainder = remainder.trim_start();
        if let Some(remainder) = remainder.strip_prefix('=') {
            return remainder;
        }
        return remainder;
    }

    trimmed
}

fn unwrap_flat_config_wrappers(mut expression: &str) -> Result<&str, String> {
    expression = trim_trailing_semicolons(expression.trim());

    loop {
        let Some(inner) = unwrap_named_call(
            expression,
            &[
                "defineConfig",
                "tseslint.config",
                "typescriptEslint.config",
                "eslint.config",
            ],
        )?
        else {
            return Ok(expression);
        };

        expression = trim_trailing_semicolons(inner.trim());
    }
}

fn unwrap_named_call<'a>(expression: &'a str, names: &[&str]) -> Result<Option<&'a str>, String> {
    for name in names {
        let Some(remainder) = expression.strip_prefix(name) else {
            continue;
        };
        let remainder = remainder.trim_start();
        if !remainder.starts_with('(') {
            continue;
        }

        let close = find_matching_delimiter(remainder, '(', ')')
            .ok_or_else(|| format!("Unbalanced wrapper call '{}(...)'", name))?;
        if !remainder[close + 1..].trim().is_empty() {
            continue;
        }

        return Ok(Some(&remainder[1..close]));
    }

    Ok(None)
}

fn trim_trailing_semicolons(mut expression: &str) -> &str {
    expression = expression.trim_end();
    while let Some(stripped) = expression.strip_suffix(';') {
        expression = stripped.trim_end();
    }
    expression
}

fn find_matching_delimiter(text: &str, open: char, close: char) -> Option<usize> {
    let mut chars = text.char_indices().peekable();
    let mut depth = 0_usize;
    let mut in_string: Option<char> = None;
    let mut escaped = false;

    while let Some((index, ch)) = chars.next() {
        if let Some(quote) = in_string {
            if escaped {
                escaped = false;
                continue;
            }

            match ch {
                '\\' => escaped = true,
                current if current == quote => in_string = None,
                _ => {}
            }
            continue;
        }

        if ch == '/' {
            match chars.peek().map(|(_, next)| *next) {
                Some('/') => {
                    chars.next();
                    for (_, comment_char) in chars.by_ref() {
                        if comment_char == '\n' {
                            break;
                        }
                    }
                    continue;
                }
                Some('*') => {
                    chars.next();
                    let mut previous = '\0';
                    for (_, comment_char) in chars.by_ref() {
                        if previous == '*' && comment_char == '/' {
                            break;
                        }
                        previous = comment_char;
                    }
                    continue;
                }
                _ => {}
            }
        }

        match ch {
            '\'' | '"' | '`' => in_string = Some(ch),
            current if current == open => depth += 1,
            current if current == close => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }

    None
}
