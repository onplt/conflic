use super::{PARSE_EXTENDS_ERROR_RULE_ID, ParsedFile, parse_diagnostic};
use crate::model::Severity;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Resolve a JSON config that may have an "extends" field,
/// merging parent configs into the child with cycle detection.
/// Returns the merged JSON value.
pub fn resolve_json_extends(value: &serde_json::Value, file: &ParsedFile) -> serde_json::Value {
    resolve_structured_extends(value, file)
}

/// Resolve a structured config (JSON or YAML) that may have an "extends" field,
/// merging parent configs into the child with cycle detection.
pub fn resolve_structured_extends(
    value: &serde_json::Value,
    file: &ParsedFile,
) -> serde_json::Value {
    let mut visited = HashSet::new();
    let canonical_file = std::fs::canonicalize(&file.path).unwrap_or_else(|_| file.path.clone());
    if let Some(file_str) = canonical_file.to_str() {
        visited.insert(file_str.to_string());
    }
    resolve_recursive(value, &file.path, &file.scan_root, &mut visited, file)
}

fn resolve_recursive(
    value: &serde_json::Value,
    file_path: &Path,
    scan_root: &Path,
    visited: &mut HashSet<String>,
    diagnostics_sink: &ParsedFile,
) -> serde_json::Value {
    let extends_entries = extends_entries(value);
    if extends_entries.is_empty() {
        return value.clone();
    }

    let mut resolved_parent = serde_json::Value::Object(serde_json::Map::new());
    let mut handled_local_extend = false;

    for extends in extends_entries {
        if let Some(parent_value) =
            resolve_single_extend(&extends, file_path, scan_root, visited, diagnostics_sink)
        {
            handled_local_extend = true;
            resolved_parent = deep_merge_json(&resolved_parent, &parent_value);
        } else if should_resolve_locally(&extends, file_path) {
            handled_local_extend = true;
        }
    }

    if handled_local_extend {
        deep_merge_json(&resolved_parent, value)
    } else {
        value.clone()
    }
}

fn extends_entries(value: &serde_json::Value) -> Vec<String> {
    match value.get("extends") {
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        Some(serde_json::Value::Array(entries)) => entries
            .iter()
            .filter_map(|entry| entry.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn resolve_single_extend(
    extends: &str,
    file_path: &Path,
    scan_root: &Path,
    visited: &mut HashSet<String>,
    diagnostics_sink: &ParsedFile,
) -> Option<serde_json::Value> {
    let base_dir = file_path.parent().unwrap_or(Path::new("."));
    let extends_path = resolve_extends_path(base_dir, extends);

    if !should_resolve_locally(extends, file_path) {
        return None;
    }

    if !extends_path.exists() {
        diagnostics_sink.push_parse_diagnostic(parse_diagnostic(
            Severity::Error,
            file_path.to_path_buf(),
            PARSE_EXTENDS_ERROR_RULE_ID,
            format!("Failed to resolve local extends path '{}'", extends),
        ));
        return None;
    }

    let canonical = match std::fs::canonicalize(&extends_path) {
        Ok(path) => path,
        Err(err) => {
            diagnostics_sink.push_parse_diagnostic(parse_diagnostic(
                Severity::Error,
                extends_path.clone(),
                PARSE_EXTENDS_ERROR_RULE_ID,
                format!("Failed to canonicalize inherited config: {}", err),
            ));
            return None;
        }
    };

    if !canonical.starts_with(scan_root) {
        diagnostics_sink.push_parse_diagnostic(parse_diagnostic(
            Severity::Warning,
            file_path.to_path_buf(),
            PARSE_EXTENDS_ERROR_RULE_ID,
            format!(
                "Blocked extends path '{}' because it resolves outside scan root {}",
                canonical.display(),
                scan_root.display()
            ),
        ));
        return None;
    }

    let canonical_str = canonical.to_string_lossy().to_string();
    if !visited.insert(canonical_str.clone()) {
        diagnostics_sink.push_parse_diagnostic(parse_diagnostic(
            Severity::Warning,
            file_path.to_path_buf(),
            PARSE_EXTENDS_ERROR_RULE_ID,
            format!(
                "Detected a cyclic extends reference involving '{}'",
                extends_path.display()
            ),
        ));
        return None;
    }

    let resolved = match read_structured_config(&canonical) {
        Ok(parent_value) => Some(resolve_recursive(
            &parent_value,
            &canonical,
            scan_root,
            visited,
            diagnostics_sink,
        )),
        Err(err) => {
            diagnostics_sink.push_parse_diagnostic(parse_diagnostic(
                Severity::Error,
                canonical.clone(),
                PARSE_EXTENDS_ERROR_RULE_ID,
                err,
            ));
            None
        }
    };

    visited.remove(&canonical_str);
    resolved
}

/// Resolve an extends path, handling:
/// - Relative paths: "./tsconfig.base.json"
/// - Bare paths without extension: "tsconfig.base" -> "tsconfig.base.json"
fn resolve_extends_path(base_dir: &Path, extends: &str) -> PathBuf {
    let path = base_dir.join(extends);

    // If the path doesn't have a format extension, try the common config variants.
    if !path.exists() && path.extension().is_none() {
        for extension in ["json", "yaml", "yml"] {
            let with_extension = path.with_extension(extension);
            if with_extension.exists() {
                return with_extension;
            }
        }
    }

    path
}

fn should_resolve_locally(extends: &str, file_path: &Path) -> bool {
    let base_dir = file_path.parent().unwrap_or(Path::new("."));
    let resolved_path = resolve_extends_path(base_dir, extends);

    resolved_path.exists()
        || Path::new(extends).is_absolute()
        || extends.starts_with('.')
        || extends.starts_with("..")
        || looks_like_local_config_reference(extends)
}

/// Read and parse an inherited JSON/YAML config file into a structured value.
fn read_structured_config(path: &Path) -> Result<serde_json::Value, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read inherited config: {}", e))?;

    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let lower = filename.to_ascii_lowercase();

    if lower == ".eslintrc" {
        return parse_json_or_yaml_value(&content);
    }

    match crate::parse::detect_format(filename, path) {
        crate::parse::FileFormat::Json => crate::parse::json::parse_json_value(&content),
        crate::parse::FileFormat::Yaml => parse_yaml_value(&content),
        crate::parse::FileFormat::PlainText => parse_json_or_yaml_value(&content),
        other => Err(format!(
            "Unsupported inherited config format {:?} for {}",
            other,
            path.display()
        )),
    }
}

fn parse_json_or_yaml_value(content: &str) -> Result<serde_json::Value, String> {
    crate::parse::json::parse_json_value(content).or_else(|json_err| {
        parse_yaml_value(content).map_err(|yaml_err| {
            format!(
                "Failed to parse inherited config as JSON/JSON5 or YAML: {}; {}",
                json_err, yaml_err
            )
        })
    })
}

fn parse_yaml_value(content: &str) -> Result<serde_json::Value, String> {
    match crate::parse::yaml::parse_yaml(content)? {
        crate::parse::FileContent::Yaml(value) => Ok(value),
        _ => Err("YAML parser returned unexpected content kind".into()),
    }
}

fn looks_like_local_config_reference(extends: &str) -> bool {
    let path = Path::new(extends);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(extends);
    let lower = file_name.to_ascii_lowercase();

    path.extension().is_some()
        || lower.starts_with("tsconfig")
        || lower.starts_with(".eslintrc")
        || lower.starts_with("eslint.config.")
}

/// Deep merge two JSON objects. Child values override parent values.
/// For objects, merge recursively. For all other types, child wins.
fn deep_merge_json(parent: &serde_json::Value, child: &serde_json::Value) -> serde_json::Value {
    match (parent, child) {
        (serde_json::Value::Object(parent_map), serde_json::Value::Object(child_map)) => {
            let mut merged = parent_map.clone();
            for (key, child_val) in child_map {
                if key == "extends" {
                    continue; // Don't propagate extends key
                }
                let merged_val = if let Some(parent_val) = parent_map.get(key) {
                    deep_merge_json(parent_val, child_val)
                } else {
                    child_val.clone()
                };
                merged.insert(key.clone(), merged_val);
            }
            serde_json::Value::Object(merged)
        }
        // For non-objects, child wins
        (_, child) => child.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Severity;
    use crate::parse::{FileContent, FileFormat};
    use std::cell::RefCell;
    use tempfile::tempdir;

    fn parsed_file(path: PathBuf, scan_root: PathBuf, value: serde_json::Value) -> ParsedFile {
        ParsedFile {
            path,
            scan_root,
            format: FileFormat::Json,
            content: FileContent::Json(value.clone()),
            raw_text: value.to_string(),
            parse_diagnostics: RefCell::new(Vec::new()),
        }
    }

    #[test]
    fn test_deep_merge_simple() {
        let parent: serde_json::Value = serde_json::json!({
            "compilerOptions": {
                "strict": true,
                "target": "es2020"
            }
        });
        let child: serde_json::Value = serde_json::json!({
            "compilerOptions": {
                "target": "es2022"
            }
        });

        let merged = deep_merge_json(&parent, &child);

        assert_eq!(merged["compilerOptions"]["strict"], true);
        assert_eq!(merged["compilerOptions"]["target"], "es2022");
    }

    #[test]
    fn test_resolve_json_extends_blocks_parent_outside_scan_root() {
        let dir = tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        let external = dir.path().join("external");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&external).unwrap();

        std::fs::write(
            external.join("tsconfig.base.json"),
            r#"{"compilerOptions":{"strict":true}}"#,
        )
        .unwrap();

        let child_path = workspace.join("tsconfig.json");
        let child_value = serde_json::json!({
            "extends": "../external/tsconfig.base.json"
        });
        std::fs::write(&child_path, child_value.to_string()).unwrap();

        let parsed = parsed_file(
            child_path,
            workspace.canonicalize().unwrap(),
            child_value.clone(),
        );

        let resolved = resolve_json_extends(&child_value, &parsed);
        let diagnostics = parsed.take_parse_diagnostics();

        assert!(resolved.get("compilerOptions").is_none());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule_id, PARSE_EXTENDS_ERROR_RULE_ID);
        assert_eq!(diagnostics[0].severity, Severity::Warning);
        assert!(diagnostics[0].message.contains("outside scan root"));
    }

    #[test]
    fn test_resolve_json_extends_merges_local_arrays_in_order() {
        let dir = tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        std::fs::write(
            workspace.join("eslint.strict.json"),
            r#"{"rules":{"@typescript-eslint/no-explicit-any":"warn"}}"#,
        )
        .unwrap();
        std::fs::write(
            workspace.join("eslint.off.json"),
            r#"{"rules":{"@typescript-eslint/no-explicit-any":"off"}}"#,
        )
        .unwrap();

        let child_path = workspace.join(".eslintrc.json");
        let child_value = serde_json::json!({
            "extends": ["./eslint.strict.json", "./eslint.off.json"]
        });
        std::fs::write(&child_path, child_value.to_string()).unwrap();

        let parsed = parsed_file(
            child_path,
            workspace.canonicalize().unwrap(),
            child_value.clone(),
        );

        let resolved = resolve_json_extends(&child_value, &parsed);

        assert_eq!(
            resolved["rules"]["@typescript-eslint/no-explicit-any"],
            "off"
        );
        assert!(parsed.take_parse_diagnostics().is_empty());
    }

    #[test]
    fn test_resolve_structured_extends_reads_yaml_parent() {
        let dir = tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        std::fs::write(
            workspace.join(".eslintrc.base.yml"),
            "rules:\n  '@typescript-eslint/no-explicit-any': off\n",
        )
        .unwrap();

        let child_path = workspace.join(".eslintrc.yml");
        let child_value = serde_json::json!({
            "extends": "./.eslintrc.base.yml"
        });
        std::fs::write(&child_path, "extends: ./.eslintrc.base.yml\n").unwrap();

        let parsed = ParsedFile {
            path: child_path,
            scan_root: workspace.canonicalize().unwrap(),
            format: FileFormat::Yaml,
            content: FileContent::Yaml(child_value.clone()),
            raw_text: "extends: ./.eslintrc.base.yml\n".into(),
            parse_diagnostics: RefCell::new(Vec::new()),
        };

        let resolved = resolve_structured_extends(&child_value, &parsed);

        assert_eq!(
            resolved["rules"]["@typescript-eslint/no-explicit-any"],
            false
        );
        assert!(parsed.take_parse_diagnostics().is_empty());
    }

    #[test]
    fn test_resolve_json_extends_reports_missing_bare_local_config() {
        let dir = tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let child_path = workspace.join("tsconfig.json");
        let child_value = serde_json::json!({
            "extends": "tsconfig.base"
        });
        std::fs::write(&child_path, child_value.to_string()).unwrap();

        let parsed = parsed_file(
            child_path,
            workspace.canonicalize().unwrap(),
            child_value.clone(),
        );

        let _ = resolve_json_extends(&child_value, &parsed);
        let diagnostics = parsed.take_parse_diagnostics();

        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.rule_id == PARSE_EXTENDS_ERROR_RULE_ID
                    && diagnostic.message.contains("tsconfig.base")
            }),
            "expected missing bare local config to produce PARSE002, got {:?}",
            diagnostics
        );
    }
}
