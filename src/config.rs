use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::error::ConfigError;

#[derive(Debug, Deserialize, Default, Clone)]
pub struct ConflicConfig {
    #[serde(default)]
    pub conflic: ConflicSettings,
    #[serde(default)]
    pub ignore: Vec<IgnoreRule>,
    #[serde(default)]
    pub monorepo: MonorepoSettings,
    #[serde(default)]
    pub custom_extractor: Vec<CustomExtractorConfig>,
    #[serde(skip)]
    compiled_custom_extractors: Vec<crate::extract::custom::CustomExtractor>,
    #[serde(skip)]
    config_diagnostics: Vec<crate::model::ParseDiagnostic>,
    #[serde(skip)]
    config_path: Option<PathBuf>,
    #[serde(skip)]
    custom_extractor_cache_hash: u64,
}

/// Configuration for a user-defined custom extractor.
#[derive(Debug, Deserialize, Clone, Hash, PartialEq, Eq)]
pub struct CustomExtractorConfig {
    pub concept: String,
    pub display_name: String,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default = "default_value_type", rename = "type")]
    pub value_type: String,
    pub source: Vec<CustomSourceConfig>,
}

/// A single source definition within a custom extractor.
#[derive(Debug, Deserialize, Clone, Hash, PartialEq, Eq)]
pub struct CustomSourceConfig {
    pub file: String,
    pub format: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub pattern: Option<String>,
    #[serde(default = "default_authority")]
    pub authority: String,
}

fn default_category() -> String {
    "custom".into()
}

fn default_value_type() -> String {
    "string".into()
}

fn default_authority() -> String {
    "declared".into()
}

#[derive(Debug, Deserialize, Clone)]
pub struct ConflicSettings {
    #[serde(default = "default_severity")]
    pub severity: String,
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub skip_concepts: Vec<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct IgnoreRule {
    pub rule: Option<String>,
    pub files: Option<Vec<String>>,
    pub file: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct MonorepoSettings {
    #[serde(default)]
    pub per_package: bool,
    #[serde(default)]
    pub package_roots: Vec<String>,
    #[serde(default)]
    pub global_concepts: Vec<String>,
}

fn default_severity() -> String {
    "warning".into()
}

fn default_format() -> String {
    "terminal".into()
}

impl Default for ConflicSettings {
    fn default() -> Self {
        Self {
            severity: default_severity(),
            format: default_format(),
            exclude: Vec::new(),
            skip_concepts: Vec::new(),
        }
    }
}

impl ConflicConfig {
    pub fn load(
        dir: &Path,
        explicit_path: Option<&Path>,
    ) -> std::result::Result<Self, ConfigError> {
        let config_path = if let Some(path) = explicit_path {
            resolve_explicit_config_path(dir, path)
        } else {
            dir.join(".conflic.toml")
        };

        if explicit_path.is_some() && !config_path.exists() {
            return Err(ConfigError::Missing(config_path));
        }

        if config_path.exists() {
            let content =
                std::fs::read_to_string(&config_path).map_err(|source| ConfigError::Read {
                    path: config_path.clone(),
                    source,
                })?;
            Self::parse_loaded_config(config_path, &content)
        } else {
            Ok(ConflicConfig::default())
        }
    }

    pub fn load_from_content(
        dir: &Path,
        explicit_path: Option<&Path>,
        content: &str,
    ) -> std::result::Result<Self, ConfigError> {
        let config_path = if let Some(path) = explicit_path {
            resolve_explicit_config_path(dir, path)
        } else {
            dir.join(".conflic.toml")
        };

        Self::parse_loaded_config(config_path, content)
    }

    pub fn should_skip_concept(&self, concept_id: &str) -> bool {
        self.conflic
            .skip_concepts
            .iter()
            .any(|selector| concept_matches_selector(concept_id, selector))
    }

    pub fn output_format(&self) -> std::result::Result<crate::cli::OutputFormat, ConfigError> {
        parse_output_format(&self.conflic.format).ok_or_else(|| ConfigError::InvalidFormat {
            value: self.conflic.format.clone(),
            path: self.config_display_path().to_path_buf(),
        })
    }

    pub fn severity_filter(&self) -> std::result::Result<crate::cli::SeverityFilter, ConfigError> {
        parse_severity_filter(&self.conflic.severity).ok_or_else(|| ConfigError::InvalidSeverity {
            value: self.conflic.severity.clone(),
            path: self.config_display_path().to_path_buf(),
        })
    }

    pub fn should_ignore_finding(&self, rule_id: &str, file1: &Path, file2: &Path) -> bool {
        for ignore in &self.ignore {
            if let Some(ref rule) = ignore.rule
                && rule != rule_id
            {
                continue;
            }

            if let Some(ref file) = ignore.file {
                let file_path = PathBuf::from(file);
                if file1.ends_with(&file_path) || file2.ends_with(&file_path) {
                    return true;
                }
            }

            if let Some(ref files) = ignore.files {
                let matches_both = files.iter().any(|f| file1.ends_with(f))
                    && files.iter().any(|f| file2.ends_with(f));
                if matches_both {
                    return true;
                }
            }
        }
        false
    }

    pub fn config_diagnostics(&self) -> &[crate::model::ParseDiagnostic] {
        &self.config_diagnostics
    }

    pub(crate) fn resolved_config_path(&self, scan_root: &Path) -> PathBuf {
        self.config_path
            .clone()
            .unwrap_or_else(|| scan_root.join(".conflic.toml"))
    }

    pub(crate) fn compiled_custom_extractors(
        &self,
    ) -> (
        Vec<crate::extract::custom::CustomExtractor>,
        Vec<crate::model::ParseDiagnostic>,
    ) {
        let current_hash = crate::extract::custom::custom_config_hash(&self.custom_extractor);

        if current_hash == self.custom_extractor_cache_hash {
            return (
                self.compiled_custom_extractors.clone(),
                self.config_diagnostics.clone(),
            );
        }

        crate::extract::custom::compile_custom_extractors(
            &self.custom_extractor,
            self.config_path.as_deref(),
        )
    }

    fn refresh_custom_extractor_cache(&mut self) {
        let (extractors, diagnostics) = crate::extract::custom::compile_custom_extractors(
            &self.custom_extractor,
            self.config_path.as_deref(),
        );
        self.custom_extractor_cache_hash =
            crate::extract::custom::custom_config_hash(&self.custom_extractor);
        self.compiled_custom_extractors = extractors;
        self.config_diagnostics = diagnostics;
    }

    fn config_display_path(&self) -> &Path {
        self.config_path
            .as_deref()
            .unwrap_or_else(|| Path::new(".conflic.toml"))
    }

    fn parse_loaded_config(
        config_path: PathBuf,
        content: &str,
    ) -> std::result::Result<Self, ConfigError> {
        let mut config: ConflicConfig =
            toml::from_str(content).map_err(|source| ConfigError::Parse {
                path: config_path.clone(),
                source,
            })?;
        config.config_path = Some(config_path);
        config.refresh_custom_extractor_cache();
        Ok(config)
    }
}

fn parse_output_format(value: &str) -> Option<crate::cli::OutputFormat> {
    match value.trim().to_ascii_lowercase().as_str() {
        "terminal" => Some(crate::cli::OutputFormat::Terminal),
        "json" => Some(crate::cli::OutputFormat::Json),
        "sarif" => Some(crate::cli::OutputFormat::Sarif),
        _ => None,
    }
}

fn parse_severity_filter(value: &str) -> Option<crate::cli::SeverityFilter> {
    match value.trim().to_ascii_lowercase().as_str() {
        "error" => Some(crate::cli::SeverityFilter::Error),
        "warning" => Some(crate::cli::SeverityFilter::Warning),
        "info" => Some(crate::cli::SeverityFilter::Info),
        _ => None,
    }
}

pub fn concept_matches_selector(concept_id: &str, selector: &str) -> bool {
    if selector == concept_id {
        return true;
    }

    matches!(
        (selector, concept_id),
        ("port", "app-port")
            | ("node", "node-version")
            | ("python", "python-version")
            | ("ruby", "ruby-version")
            | ("java", "java-version")
            | ("go", "go-version")
            | ("dotnet", "dotnet-version")
            | ("ts-strict", "ts-strict-mode")
    )
}

fn resolve_explicit_config_path(scan_root: &Path, explicit_path: &Path) -> PathBuf {
    if explicit_path.is_absolute() {
        explicit_path.to_path_buf()
    } else {
        scan_root.join(explicit_path)
    }
}

pub fn generate_template() -> String {
    r#"# conflic configuration
# See https://github.com/conflic/conflic for documentation

[conflic]
# Minimum severity to report: "error", "warning", or "info"
severity = "warning"

# Output format: "terminal", "json", or "sarif"
format = "terminal"

# Additional directories to exclude (node_modules, .git, vendor are always excluded)
exclude = []

# Concepts to skip entirely
skip_concepts = []

# Ignore specific contradictions
# [[ignore]]
# rule = "VER001"
# files = ["Dockerfile", ".nvmrc"]
# reason = "Multi-stage build; final stage matches"

# Monorepo settings
# [monorepo]
# per_package = true
# package_roots = ["packages/*", "apps/*"]
# global_concepts = ["node-version", "ts-strict-mode"]

# Custom extractors — define your own concepts without writing Rust
# [[custom_extractor]]
# concept = "redis-version"
# display_name = "Redis Version"
# category = "runtime-version"
# type = "version"
#
# [[custom_extractor.source]]
# file = "docker-compose.yml"
# format = "yaml"
# path = "services.redis.image"
# pattern = "redis:(.*)"
# authority = "enforced"
#
# [[custom_extractor.source]]
# file = ".env"
# format = "env"
# key = "REDIS_VERSION"
# authority = "declared"
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_relative_explicit_config_from_scan_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let config_dir = root.join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("custom.toml"),
            r#"[conflic]
severity = "error"
"#,
        )
        .unwrap();

        let config = ConflicConfig::load(root, Some(Path::new("config/custom.toml"))).unwrap();
        assert_eq!(config.conflic.severity, "error");
    }

    #[test]
    fn test_load_missing_explicit_config_errors() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let err = ConflicConfig::load(root, Some(Path::new("missing.toml"))).unwrap_err();

        assert!(
            err.to_string().contains("Config file not found"),
            "expected missing explicit config error, got {err}"
        );
        assert!(
            err.to_string().contains("missing.toml"),
            "expected resolved path in error, got {err}"
        );
    }

    #[test]
    fn test_load_missing_implicit_config_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let config = ConflicConfig::load(root, None).unwrap();
        assert_eq!(config.conflic.severity, "warning");
    }

    #[test]
    fn test_load_collects_custom_extractor_validation_diagnostics() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join(".conflic.toml"),
            r#"[[custom_extractor]]
concept = "redis-version"
display_name = "Redis Version"
category = "runtime-version"
type = "version"

[[custom_extractor.source]]
file = "*.json"
format = "json"
path = "custom.redis"
pattern = "redis:("
authority = "declared"
"#,
        )
        .unwrap();

        let config = ConflicConfig::load(root, None).unwrap();

        assert!(
            config
                .config_diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.rule_id == "CONFIG001"),
            "expected structured custom extractor diagnostics, got {:?}",
            config.config_diagnostics()
        );
    }
}
