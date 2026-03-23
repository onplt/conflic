use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CiConfigPathKind {
    GithubWorkflows,
    CircleCi,
    GitlabDirectory,
    GitlabRootFile,
}

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

/// Make a path user-friendly for display: strip the Windows extended-length
/// prefix and attempt to make it relative to the current working directory.
pub(crate) fn simplify_path(path: &Path) -> String {
    let sanitized = strip_windows_extended_length_prefix(path);

    if let Ok(cwd) = std::env::current_dir() {
        let sanitized_cwd = strip_windows_extended_length_prefix(&cwd);
        if let Ok(rel) = sanitized.strip_prefix(&sanitized_cwd) {
            return rel.to_string_lossy().to_string();
        }
    }

    sanitized.to_string_lossy().to_string()
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

pub(crate) fn classify_ci_config_path(scan_root: &Path, path: &Path) -> Option<CiConfigPathKind> {
    let normalized_root = normalize_root(scan_root);
    let normalized_path = normalize_from_root(&normalized_root, path);
    let relative = normalized_path.strip_prefix(&normalized_root).ok()?;
    let filename = relative.file_name()?.to_str()?;
    let is_yaml = filename.ends_with(".yml") || filename.ends_with(".yaml");
    let component_count = relative.components().count();

    if component_count == 1 && filename == ".gitlab-ci.yml" {
        return Some(CiConfigPathKind::GitlabRootFile);
    }

    if !is_yaml {
        return None;
    }

    let mut components = relative
        .components()
        .filter_map(|component| component.as_os_str().to_str());
    let first = components.next()?;

    match first {
        ".github" => (components.next() == Some("workflows") && component_count == 3)
            .then_some(CiConfigPathKind::GithubWorkflows),
        ".circleci" => (component_count == 2 && matches!(filename, "config.yml" | "config.yaml"))
            .then_some(CiConfigPathKind::CircleCi),
        ".gitlab-ci" => Some(CiConfigPathKind::GitlabDirectory),
        _ => None,
    }
}

fn normalize_from_root(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        return normalize_path(path);
    }

    let normalized = normalize_path(path);
    if normalized.starts_with(root) {
        normalized
    } else {
        normalize_path(&root.join(path))
    }
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

    #[test]
    fn classify_ci_config_path_supports_gitlab_ci_directory_files() {
        let root = Path::new("workspace");
        let path = Path::new("workspace/.gitlab-ci/jobs/build.yml");

        assert_eq!(
            classify_ci_config_path(root, path),
            Some(CiConfigPathKind::GitlabDirectory)
        );
    }

    #[test]
    fn classify_ci_config_path_requires_root_gitlab_ci_yml() {
        let root = Path::new("workspace");
        let nested = Path::new("workspace/subdir/.gitlab-ci.yml");
        let root_file = Path::new("workspace/.gitlab-ci.yml");

        assert_eq!(classify_ci_config_path(root, nested), None);
        assert_eq!(
            classify_ci_config_path(root, root_file),
            Some(CiConfigPathKind::GitlabRootFile)
        );
    }

    #[test]
    fn classify_ci_config_path_requires_github_workflow_file_at_supported_depth() {
        let root = Path::new("workspace");
        let direct = Path::new("workspace/.github/workflows/ci.yml");
        let nested = Path::new("workspace/.github/workflows/nested/ci.yml");

        assert_eq!(
            classify_ci_config_path(root, direct),
            Some(CiConfigPathKind::GithubWorkflows)
        );
        assert_eq!(classify_ci_config_path(root, nested), None);
    }

    #[test]
    fn classify_ci_config_path_requires_root_circleci_config_file() {
        let root = Path::new("workspace");
        let config = Path::new("workspace/.circleci/config.yml");
        let alt_config = Path::new("workspace/.circleci/config.yaml");
        let notes = Path::new("workspace/.circleci/notes.yml");
        let nested = Path::new("workspace/.circleci/jobs/config.yml");

        assert_eq!(
            classify_ci_config_path(root, config),
            Some(CiConfigPathKind::CircleCi)
        );
        assert_eq!(
            classify_ci_config_path(root, alt_config),
            Some(CiConfigPathKind::CircleCi)
        );
        assert_eq!(classify_ci_config_path(root, notes), None);
        assert_eq!(classify_ci_config_path(root, nested), None);
    }
}
