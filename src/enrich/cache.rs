use std::path::{Path, PathBuf};

use super::RegistryDb;

/// Default cache file name.
const CACHE_FILENAME: &str = ".conflic-registry-cache.json";

/// Resolve the cache file path. Checks:
/// 1. Project-local (.conflic-registry-cache.json in scan root)
/// 2. Home directory (~/.conflic/registry-cache.json)
pub fn resolve_cache_path(scan_root: &Path) -> PathBuf {
    let local = scan_root.join(CACHE_FILENAME);
    if local.exists() {
        return local;
    }

    if let Some(home) = home_dir() {
        let global = home.join(".conflic").join("registry-cache.json");
        if global.exists() {
            return global;
        }
    }

    // Default to project-local even if it doesn't exist
    local
}

/// Load the registry database from cache, returning default if not found.
pub fn load_cache(scan_root: &Path) -> RegistryDb {
    let path = resolve_cache_path(scan_root);
    load_cache_from_path(&path).unwrap_or_default()
}

/// Load a specific cache file.
pub fn load_cache_from_path(path: &Path) -> Option<RegistryDb> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Save the registry database to the project-local cache.
pub fn save_cache(scan_root: &Path, db: &RegistryDb) -> anyhow::Result<()> {
    let path = scan_root.join(CACHE_FILENAME);
    let json = serde_json::to_string_pretty(db)?;
    std::fs::write(&path, json)?;
    Ok(())
}

/// Get the user's home directory.
fn home_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enrich::{RegistryDb, VersionLifecycle};

    #[test]
    fn test_save_and_load_cache_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let mut db = RegistryDb::default();
        db.families.insert(
            "node".into(),
            vec![VersionLifecycle {
                cycle: "20".into(),
                release_date: Some("2023-04-18".into()),
                eol_date: Some("2026-04-30".into()),
                lts: true,
                latest_patch: Some("20.18.0".into()),
            }],
        );
        db.updated_at = Some("2025-01-01T00:00:00Z".into());

        save_cache(root, &db).unwrap();
        let loaded = load_cache(root);

        assert_eq!(loaded.families.len(), 1);
        assert!(loaded.families.contains_key("node"));
        assert_eq!(loaded.updated_at.as_deref(), Some("2025-01-01T00:00:00Z"));
    }

    #[test]
    fn test_load_cache_missing_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let db = load_cache(dir.path());
        assert!(db.families.is_empty());
    }
}
