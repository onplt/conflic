use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Resolve a JSON config that may have an "extends" field,
/// merging parent configs into the child with cycle detection.
/// Returns the merged JSON value.
pub fn resolve_json_extends(
    value: &serde_json::Value,
    file_path: &Path,
) -> serde_json::Value {
    let mut visited = HashSet::new();
    if let Some(file_str) = file_path.to_str() {
        visited.insert(file_str.to_string());
    }
    resolve_recursive(value, file_path, &mut visited)
}

fn resolve_recursive(
    value: &serde_json::Value,
    file_path: &Path,
    visited: &mut HashSet<String>,
) -> serde_json::Value {
    let extends = match value.get("extends") {
        Some(serde_json::Value::String(s)) => s.clone(),
        _ => return value.clone(),
    };

    // Resolve the extends path relative to the current file's directory
    let base_dir = file_path.parent().unwrap_or(Path::new("."));
    let extends_path = resolve_extends_path(base_dir, &extends);

    let canonical = std::fs::canonicalize(&extends_path)
        .unwrap_or_else(|_| extends_path.clone());
    let canonical_str = canonical.to_string_lossy().to_string();

    // Cycle detection
    if !visited.insert(canonical_str) {
        return value.clone();
    }

    // Try to read and parse the parent config
    let parent_value = match read_json_config(&extends_path) {
        Some(v) => v,
        None => return value.clone(),
    };

    // Recursively resolve the parent's extends
    let resolved_parent = resolve_recursive(&parent_value, &extends_path, visited);

    // Deep merge: child overrides parent
    deep_merge_json(&resolved_parent, value)
}

/// Resolve an extends path, handling:
/// - Relative paths: "./tsconfig.base.json"
/// - Bare paths without extension: "tsconfig.base" -> "tsconfig.base.json"
fn resolve_extends_path(base_dir: &Path, extends: &str) -> PathBuf {
    let path = base_dir.join(extends);

    // If the path doesn't have a JSON extension, try adding one
    if !path.exists() {
        if path.extension().is_none() {
            let with_json = path.with_extension("json");
            if with_json.exists() {
                return with_json;
            }
        }
    }

    path
}

/// Read and parse a JSON config file (with JSONC/json5 fallback).
fn read_json_config(path: &Path) -> Option<serde_json::Value> {
    let content = std::fs::read_to_string(path).ok()?;
    // Try standard JSON first, then json5 for JSONC support
    serde_json::from_str(&content)
        .ok()
        .or_else(|| json5::from_str(&content).ok())
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
}
