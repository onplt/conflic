use ignore::WalkBuilder;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Discovers config files in a directory tree, respecting .gitignore.
pub struct FileDiscoverer {
    root: PathBuf,
    extra_excludes: Vec<String>,
}

/// Known config file patterns we care about.
const KNOWN_CONFIG_FILES: &[&str] = &[
    "package.json",
    "tsconfig.json",
    "tsconfig.base.json",
    "tsconfig.build.json",
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
];

/// Filename prefixes that match config files.
const CONFIG_PREFIXES: &[&str] = &["Dockerfile", ".env."];

/// File extensions for CI/CD configs.
const CI_DIRS: &[&str] = &[".github/workflows", ".gitlab-ci", ".circleci"];

impl FileDiscoverer {
    pub fn new(root: &Path, extra_excludes: Vec<String>) -> Self {
        Self {
            root: root.to_path_buf(),
            extra_excludes,
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
        for dir in &[
            "node_modules",
            ".git",
            "vendor",
            "target",
            "dist",
            "build",
            "__pycache__",
            ".tox",
            ".venv",
            "venv",
        ] {
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
            let _ = dir; // suppress unused warning
        }

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
                results
                    .entry(filename)
                    .or_default()
                    .push(path);
            }
        }

        results
    }

    fn is_config_file(&self, filename: &str, path: &Path) -> bool {
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

        // CI/CD yaml files
        let path_str = path.to_string_lossy();
        for ci_dir in CI_DIRS {
            if path_str.contains(ci_dir) && (filename.ends_with(".yml") || filename.ends_with(".yaml"))
            {
                return true;
            }
        }

        // .gitlab-ci.yml at root
        if filename == ".gitlab-ci.yml" {
            return true;
        }

        false
    }

    fn is_excluded(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        for exclude in &self.extra_excludes {
            if path_str.contains(exclude.as_str()) {
                return true;
            }
        }
        false
    }
}
