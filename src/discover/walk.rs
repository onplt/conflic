use globset::Glob;
use ignore::WalkBuilder;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Discovers config files in a directory tree, respecting .gitignore.
pub struct FileDiscoverer {
    root: PathBuf,
    exclude_globs: Vec<globset::GlobMatcher>,
    exclude_segments: Vec<String>,
    exclude_path_prefixes: Vec<String>,
    extra_include_patterns: Vec<AdditionalFilePattern>,
}

struct AdditionalFilePattern {
    raw: String,
    normalized: String,
    glob: Option<globset::GlobMatcher>,
}

/// Known config file patterns we care about.
const KNOWN_CONFIG_FILES: &[&str] = &[
    "package.json",
    ".nvmrc",
    ".node-version",
    ".python-version",
    ".ruby-version",
    ".go-version",
    ".tool-versions",
    "pyproject.toml",
    "go.mod",
    "pom.xml",
    "docker-compose.yml",
    "docker-compose.yaml",
    "docker-compose.override.yml",
    ".env",
    ".eslintrc",
    ".eslintrc.json",
    ".eslintrc.yml",
    ".eslintrc.yaml",
    "Gemfile",
    ".sdkmanrc",
    "global.json",
    // Helm
    "values.yaml",
    "values.yml",
    // Kubernetes manifests
    "deployment.yaml",
    "deployment.yml",
    "service.yaml",
    "service.yml",
    "statefulset.yaml",
    "statefulset.yml",
    "pod.yaml",
    "pod.yml",
    "job.yaml",
    "job.yml",
    "cronjob.yaml",
    "cronjob.yml",
];

/// Filename prefixes that match config files.
const CONFIG_PREFIXES: &[&str] = &["Dockerfile", ".env."];

impl FileDiscoverer {
    pub fn new(
        root: &Path,
        extra_excludes: Vec<String>,
        extra_include_patterns: Vec<String>,
    ) -> Self {
        let mut exclude_globs = Vec::new();
        let mut exclude_segments = Vec::new();
        let mut exclude_path_prefixes = Vec::new();

        for pattern in &extra_excludes {
            // Normalize: strip trailing slashes for segment matching
            let normalized = pattern
                .trim_end_matches('/')
                .trim_end_matches('\\')
                .trim_start_matches("./")
                .trim_start_matches(".\\")
                .replace('\\', "/");

            // If it looks like a glob pattern, compile it (also try with **/ prefix)
            if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
                if let Ok(g) = Glob::new(pattern) {
                    exclude_globs.push(g.compile_matcher());
                }
                // Also try with **/ prefix for matching within subdirectories
                let prefixed = format!("**/{}", pattern);
                if let Ok(g) = Glob::new(&prefixed) {
                    exclude_globs.push(g.compile_matcher());
                }
            }

            if normalized.contains('/') {
                exclude_path_prefixes.push(normalized);
            } else {
                // Simple names still behave like directory-segment exclusions.
                exclude_segments.push(normalized);
            }
        }

        let extra_include_patterns = extra_include_patterns
            .into_iter()
            .filter_map(AdditionalFilePattern::new)
            .collect();

        Self {
            root: root.to_path_buf(),
            exclude_globs,
            exclude_segments,
            exclude_path_prefixes,
            extra_include_patterns,
        }
    }

    /// Walk the directory tree and return all config files, keyed by filename.
    pub fn discover(&self) -> HashMap<String, Vec<PathBuf>> {
        let mut results: HashMap<String, Vec<PathBuf>> = HashMap::new();

        let mut builder = WalkBuilder::new(&self.root);
        builder
            .hidden(false) // Don't skip hidden files (we need .nvmrc, .env, etc.)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true);

        // Always ignore these directories
        builder.filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                return !matches!(
                    name.as_ref(),
                    "node_modules"
                        | ".git"
                        | "vendor"
                        | "target"
                        | "dist"
                        | "build"
                        | "__pycache__"
                        | ".tox"
                        | ".venv"
                        | "venv"
                );
            }
            true
        });

        for entry in builder.build().flatten() {
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }

            let path = entry.path().to_path_buf();
            let filename = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            // Check if this path should be excluded
            if self.is_excluded(&path) {
                continue;
            }

            if self.is_config_file(&filename, &path) {
                results.entry(filename).or_default().push(path);
            }
        }

        results
    }

    fn is_config_file(&self, filename: &str, path: &Path) -> bool {
        if is_tsconfig_file(filename) {
            return true;
        }

        if is_docker_compose_file(filename) {
            return true;
        }

        // Exact match
        if KNOWN_CONFIG_FILES.contains(&filename) {
            return true;
        }

        // Prefix match (Dockerfile, Dockerfile.dev, .env.local, etc.)
        for prefix in CONFIG_PREFIXES {
            if filename.starts_with(prefix) || filename == prefix.trim_end_matches('.') {
                return true;
            }
        }

        // eslint.config.* (flat config)
        if filename.starts_with("eslint.config.") {
            return true;
        }

        // .csproj files (.NET)
        if filename.ends_with(".csproj") {
            return true;
        }

        // Terraform files
        if filename.ends_with(".tf") {
            return true;
        }

        if crate::pathing::classify_ci_config_path(&self.root, path).is_some() {
            return true;
        }

        self.extra_include_patterns
            .iter()
            .any(|pattern| pattern.matches(&self.root, filename, path))
    }

    fn is_excluded(&self, path: &Path) -> bool {
        // Check precompiled glob patterns
        for matcher in &self.exclude_globs {
            if matcher.is_match(path) {
                return true;
            }
        }

        let full_path = normalize_path_for_match(path);
        let relative_path = path
            .strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");

        for prefix in &self.exclude_path_prefixes {
            let candidate = if Path::new(prefix).is_absolute() || prefix.starts_with("//") {
                &full_path
            } else {
                &relative_path
            };

            if path_matches_prefix(candidate, prefix) {
                return true;
            }
        }

        // Check path segment matches (e.g. "vendor" matches "foo/vendor/bar")
        for segment in &self.exclude_segments {
            for component in path.components() {
                if component.as_os_str().to_string_lossy() == segment.as_str() {
                    return true;
                }
            }
        }
        false
    }
}

fn normalize_path_for_match(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn trim_relative_prefix(path: &str) -> String {
    path.trim_start_matches("./")
        .trim_start_matches(".\\")
        .to_string()
}

fn path_matches_prefix(path: &str, prefix: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn is_docker_compose_file(filename: &str) -> bool {
    filename.starts_with("docker-compose")
        && (filename.ends_with(".yml") || filename.ends_with(".yaml"))
}

fn is_tsconfig_file(filename: &str) -> bool {
    filename.starts_with("tsconfig") && filename.ends_with(".json")
}

impl AdditionalFilePattern {
    fn new(pattern: String) -> Option<Self> {
        let normalized = trim_relative_prefix(&pattern.replace('\\', "/"));
        let glob = is_glob_pattern(&pattern)
            .then(|| Glob::new(&normalized).ok())
            .flatten()
            .map(|compiled| compiled.compile_matcher());

        Some(Self {
            raw: pattern,
            normalized,
            glob,
        })
    }

    fn matches(&self, root: &Path, filename: &str, path: &Path) -> bool {
        let full_path = normalize_path_for_match(path);
        let relative_path = trim_relative_prefix(
            &path
                .strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/"),
        );

        if let Some(glob) = &self.glob {
            return glob.is_match(filename)
                || glob.is_match(&relative_path)
                || glob.is_match(&full_path);
        }

        if self.normalized.contains('/') {
            return if Path::new(&self.normalized).is_absolute() || self.normalized.starts_with("//")
            {
                full_path == self.normalized
            } else {
                relative_path == self.normalized
            };
        }

        filename == self.raw
    }
}

fn is_glob_pattern(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?') || pattern.contains('[')
}
