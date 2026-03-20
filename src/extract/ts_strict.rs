use crate::model::*;
use crate::parse::*;
use crate::parse::source_location::*;
use super::Extractor;

// --- tsconfig.json strict mode ---

pub struct TsconfigStrictExtractor;

impl Extractor for TsconfigStrictExtractor {
    fn id(&self) -> &str { "ts-strict-tsconfig" }
    fn description(&self) -> &str { "TypeScript strict mode from tsconfig.json" }
    fn relevant_filenames(&self) -> Vec<&str> { vec!["tsconfig"] }

    fn matches_file(&self, filename: &str) -> bool {
        filename.starts_with("tsconfig") && filename.ends_with(".json")
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::Json(ref value) = file.content {
            // Resolve extends chain to get merged config
            let resolved = crate::parse::extends::resolve_json_extends(value, &file.path);

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

                let key_path = if is_local {
                    "compilerOptions.strict".into()
                } else {
                    "compilerOptions.strict (inherited)".into()
                };

                return vec![ConfigAssertion::new(
                    SemanticConcept::ts_strict_mode(),
                    SemanticType::Boolean(strict),
                    strict.to_string(),
                    SourceLocation {
                        file: file.path.clone(),
                        line,
                        column: 0,
                        key_path,
                    },
                    Authority::Enforced,
                    self.id(),
                )];
            }
        }
        vec![]
    }
}

// --- ESLint strict-related rules ---

pub struct EslintStrictExtractor;

impl Extractor for EslintStrictExtractor {
    fn id(&self) -> &str { "ts-strict-eslint" }
    fn description(&self) -> &str { "TypeScript strict mode from ESLint config" }
    fn relevant_filenames(&self) -> Vec<&str> { vec![".eslintrc", "eslint.config."] }

    fn matches_file(&self, filename: &str) -> bool {
        filename.starts_with(".eslintrc") || filename.starts_with("eslint.config.")
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        match &file.content {
            FileContent::Json(value) => {
                // Resolve extends for eslint JSON configs
                let resolved = crate::parse::extends::resolve_json_extends(value, &file.path);
                extract_eslint_strict_from_json(&resolved, file, self.id())
            }
            FileContent::Yaml(value) => extract_eslint_strict_from_yaml(value, file, self.id()),
            _ => vec![],
        }
    }
}

fn extract_eslint_strict_from_json(
    value: &serde_json::Value,
    file: &ParsedFile,
    extractor_id: &str,
) -> Vec<ConfigAssertion> {
    if let Some(rules) = value.get("rules").and_then(|r| r.as_object()) {
        return check_strict_rules_json(rules, file, extractor_id);
    }
    vec![]
}

fn check_strict_rules_json(
    rules: &serde_json::Map<String, serde_json::Value>,
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
                serde_json::Value::Array(arr) => {
                    arr.first().is_some_and(|v| {
                        v.as_str() == Some("off") || v.as_i64() == Some(0)
                    })
                }
                _ => false,
            };

            if is_off {
                let line = find_line_for_key(&file.raw_text, rule_name);
                results.push(ConfigAssertion::new(
                    SemanticConcept::ts_strict_mode(),
                    SemanticType::Boolean(false),
                    format!("{}: off", rule_name),
                    SourceLocation {
                        file: file.path.clone(),
                        line,
                        column: 0,
                        key_path: format!("rules.{}", rule_name),
                    },
                    Authority::Enforced,
                    extractor_id,
                ));
            }
        }
    }

    results
}

fn extract_eslint_strict_from_yaml(
    value: &serde_yml::Value,
    file: &ParsedFile,
    extractor_id: &str,
) -> Vec<ConfigAssertion> {
    if let Some(rules) = value.get("rules").and_then(|r| r.as_mapping()) {
        let strict_rules = [
            "@typescript-eslint/strict-boolean-expressions",
            "@typescript-eslint/strict-type-checked",
            "@typescript-eslint/no-explicit-any",
        ];

        let mut results = Vec::new();

        for rule_name in &strict_rules {
            if let Some(rule_val) = rules.get(serde_yml::Value::String(rule_name.to_string())) {
                let is_off = match rule_val {
                    serde_yml::Value::String(s) => s == "off" || s == "0",
                    serde_yml::Value::Number(n) => n.as_i64() == Some(0),
                    serde_yml::Value::Sequence(seq) => {
                        seq.first().is_some_and(|v| {
                            v.as_str() == Some("off") || v.as_i64() == Some(0)
                        })
                    }
                    _ => false,
                };

                if is_off {
                    let line = find_line_for_key(&file.raw_text, rule_name);
                    results.push(ConfigAssertion::new(
                        SemanticConcept::ts_strict_mode(),
                        SemanticType::Boolean(false),
                        format!("{}: off", rule_name),
                        SourceLocation {
                            file: file.path.clone(),
                            line,
                            column: 0,
                            key_path: format!("rules.{}", rule_name),
                        },
                        Authority::Enforced,
                        extractor_id,
                    ));
                }
            }
        }

        return results;
    }
    vec![]
}
