use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

pub(crate) fn normalize_root(root: &Path) -> PathBuf {
    normalize_path(root)
}

pub(crate) fn normalize_path(path: &Path) -> PathBuf {
    canonicalize_with_ancestor(path).unwrap_or_else(|| lexical_normalize(path))
}

#[allow(dead_code)]
pub(crate) fn paths_equivalent(left: &Path, right: &Path) -> bool {
    normalize_path(left) == normalize_path(right)
}

pub(crate) fn normalize_for_workspace(root: &Path, path: &Path) -> PathBuf {
    let normalized_root = normalize_root(root);
    normalize_from_root(&normalized_root, path)
}

pub(crate) fn normalize_if_within_root(root: &Path, path: &Path) -> Option<PathBuf> {
    let normalized_root = normalize_root(root);
    let normalized_path = normalize_from_root(&normalized_root, path);
    normalized_path
        .starts_with(&normalized_root)
        .then_some(normalized_path)
}

pub(crate) fn strip_windows_extended_length_prefix(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();

    if let Some(stripped) = raw.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{}", stripped));
    }

    if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        return PathBuf::from(stripped);
    }

    path.to_path_buf()
}

fn normalize_from_root(root: &Path, path: &Path) -> PathBuf {
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };

    normalize_path(&candidate)
}

fn canonicalize_with_ancestor(path: &Path) -> Option<PathBuf> {
    let mut current = path;
    let mut suffix = Vec::new();

    loop {
        if let Ok(canonical) = std::fs::canonicalize(current) {
            let mut rebuilt = canonical;
            for component in suffix.iter().rev() {
                rebuilt.push(component);
            }
            return Some(rebuilt);
        }

        let name = current.file_name()?.to_os_string();
        suffix.push(name);
        current = current.parent()?;
    }
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut prefix: Option<OsString> = None;
    let mut has_root = false;
    let mut parts: Vec<OsString> = Vec::new();

    for component in path.components() {
        match component {
            Component::Prefix(value) => prefix = Some(value.as_os_str().to_os_string()),
            Component::RootDir => has_root = true,
            Component::CurDir => {}
            Component::ParentDir => {
                if let Some(last) = parts.last() {
                    if last != ".." {
                        parts.pop();
                    } else if !has_root {
                        parts.push(OsString::from(".."));
                    }
                } else if !has_root {
                    parts.push(OsString::from(".."));
                }
            }
            Component::Normal(part) => parts.push(part.to_os_string()),
        }
    }

    let mut normalized = PathBuf::new();
    if let Some(prefix) = prefix {
        normalized.push(PathBuf::from(prefix));
    }
    if has_root {
        normalized.push(Path::new(std::path::MAIN_SEPARATOR_STR));
    }
    for part in parts {
        normalized.push(part);
    }

    if normalized.as_os_str().is_empty() {
        if has_root {
            PathBuf::from(std::path::MAIN_SEPARATOR_STR)
        } else {
            PathBuf::from(".")
        }
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_windows_extended_length_prefix_handles_local_paths() {
        let sanitized =
            strip_windows_extended_length_prefix(Path::new(r"\\?\C:\workspace\package.json"));

        assert_eq!(sanitized, PathBuf::from(r"C:\workspace\package.json"));
    }

    #[test]
    fn strip_windows_extended_length_prefix_handles_unc_paths() {
        let sanitized =
            strip_windows_extended_length_prefix(Path::new(r"\\?\UNC\server\share\package.json"));

        assert_eq!(sanitized, PathBuf::from(r"\\server\share\package.json"));
    }
}
