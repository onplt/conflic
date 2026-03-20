use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

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
}

/// Configuration for a user-defined custom extractor.
#[derive(Debug, Deserialize, Clone)]
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
#[derive(Debug, Deserialize, Clone)]
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

#[derive(Debug, Deserialize, Default, Clone)]
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

impl ConflicConfig {
    pub fn load(dir: &Path, explicit_path: Option<&Path>) -> Result<Self> {
        let config_path = if let Some(p) = explicit_path {
            p.to_path_buf()
        } else {
            dir.join(".conflic.toml")
        };

        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .with_context(|| format!("Failed to read config: {}", config_path.display()))?;
            let config: ConflicConfig = toml::from_str(&content)
                .with_context(|| format!("Failed to parse config: {}", config_path.display()))?;
            Ok(config)
        } else {
            Ok(ConflicConfig::default())
        }
    }

    pub fn should_skip_concept(&self, concept_id: &str) -> bool {
        self.conflic.skip_concepts.iter().any(|s| s == concept_id)
    }

    pub fn should_ignore_finding(&self, rule_id: &str, file1: &Path, file2: &Path) -> bool {
        for ignore in &self.ignore {
            if let Some(ref rule) = ignore.rule {
                if rule != rule_id {
                    continue;
                }
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
}

pub fn generate_template() -> String {
    r#"# conflic configuration
# See https://github.com/conflic/conflic for documentation

[conflic]
# Minimum severity to report: "error", "warning", or "info"
severity = "warning"

# Output format: "terminal" or "json"
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
