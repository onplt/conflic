use super::Extractor;
use crate::config::{CustomExtractorConfig, CustomSourceConfig};
use crate::model::ParseDiagnostic;
use crate::model::assertion::{Authority, ConfigAssertion, SourceLocation};
use crate::model::concept::{ConceptCategory, SemanticConcept};
use crate::model::semantic_type::{self, SemanticType};
use crate::parse::ParsedFile;
use regex::Regex;
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;

#[path = "custom/compile.rs"]
mod compile_impl;
#[path = "custom/extract_impl.rs"]
mod extract_impl;
#[path = "custom/navigation.rs"]
mod navigation_impl;

use compile_impl::{compile_source, config_diagnostic};
#[cfg(test)]
use navigation_impl::{apply_pattern, navigate_json, navigate_toml, navigate_yaml};

pub const CUSTOM_EXTRACTOR_CONFIG_RULE_ID: &str = "CONFIG001";
const COMPILED_CONFIG_FILE_FALLBACK: &str = ".conflic.toml";

#[derive(Clone)]
pub struct CustomExtractor {
    config: CustomExtractorConfig,
    sources: Vec<CompiledCustomSource>,
}

#[derive(Clone)]
struct CompiledCustomSource {
    config: CustomSourceConfig,
    normalized_file: String,
    filename_glob: Option<Arc<globset::GlobMatcher>>,
    path_glob: Option<Arc<globset::GlobMatcher>>,
    relative_path_glob: Option<Arc<globset::GlobMatcher>>,
    pattern_regex: Option<Regex>,
}

impl fmt::Debug for CustomExtractor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CustomExtractor")
            .field("concept", &self.config.concept)
            .field("display_name", &self.config.display_name)
            .field("source_count", &self.sources.len())
            .finish()
    }
}

pub fn custom_config_hash(configs: &[CustomExtractorConfig]) -> u64 {
    let mut hasher = DefaultHasher::new();
    configs.hash(&mut hasher);
    hasher.finish()
}

pub fn compile_custom_extractors(
    configs: &[CustomExtractorConfig],
    config_path: Option<&Path>,
) -> (Vec<CustomExtractor>, Vec<ParseDiagnostic>) {
    let mut extractors = Vec::new();
    let mut diagnostics = Vec::new();

    for config in configs {
        let (extractor, extractor_diagnostics) = CustomExtractor::new(config.clone(), config_path);
        diagnostics.extend(extractor_diagnostics);
        if let Some(extractor) = extractor {
            extractors.push(extractor);
        }
    }

    (extractors, diagnostics)
}

impl CustomExtractor {
    pub fn new(
        config: CustomExtractorConfig,
        config_path: Option<&Path>,
    ) -> (Option<Self>, Vec<ParseDiagnostic>) {
        let mut diagnostics = Vec::new();
        let mut sources = Vec::new();

        for (index, source) in config.source.iter().cloned().enumerate() {
            match compile_source(source, &config, index, config_path) {
                Ok(source) => sources.push(source),
                Err(diagnostic) => diagnostics.push(diagnostic),
            }
        }

        if sources.is_empty() {
            diagnostics.push(config_diagnostic(
                config_path,
                format!(
                    "Custom extractor '{}' has no valid sources after validation",
                    config.concept
                ),
            ));
            return (None, diagnostics);
        }

        (Some(Self { config, sources }), diagnostics)
    }

    fn concept(&self) -> SemanticConcept {
        let category = match self.config.category.as_str() {
            "runtime-version" => ConceptCategory::RuntimeVersion,
            "port" => ConceptCategory::Port,
            "strict-mode" => ConceptCategory::StrictMode,
            "build-tool" => ConceptCategory::BuildTool,
            "package-manager" => ConceptCategory::PackageManager,
            other => ConceptCategory::Custom(other.to_string()),
        };

        SemanticConcept {
            id: self.config.concept.clone(),
            display_name: self.config.display_name.clone(),
            category,
        }
    }

    fn parse_value(&self, raw: &str) -> SemanticType {
        match self.config.value_type.as_str() {
            "version" => SemanticType::Version(semantic_type::parse_version(raw)),
            "port" => semantic_type::parse_port(raw)
                .map(SemanticType::Port)
                .unwrap_or_else(|| SemanticType::StringValue(raw.to_string())),
            "boolean" => semantic_type::normalize_boolean(raw)
                .map(SemanticType::Boolean)
                .unwrap_or_else(|| SemanticType::StringValue(raw.to_string())),
            _ => SemanticType::StringValue(raw.to_string()),
        }
    }

    fn make_assertion(
        &self,
        raw: &str,
        key_path: &str,
        file: &ParsedFile,
        authority: Authority,
    ) -> ConfigAssertion {
        let value = self.parse_value(raw);
        let line = crate::parse::source_location::find_line_for_key_value(
            &file.raw_text,
            key_path.rsplit('.').next().unwrap_or(key_path),
            raw,
        );
        let location = SourceLocation {
            file: file.path.clone(),
            line,
            column: 0,
            key_path: key_path.to_string(),
        };
        ConfigAssertion::new(
            self.concept(),
            value,
            raw.to_string(),
            location,
            authority,
            self.id(),
        )
    }
}

impl Extractor for CustomExtractor {
    fn id(&self) -> &str {
        &self.config.concept
    }

    fn description(&self) -> &str {
        &self.config.display_name
    }

    fn relevant_filenames(&self) -> Vec<&str> {
        self.sources
            .iter()
            .map(|source| source.config.file.as_str())
            .collect()
    }

    fn matches_file(&self, filename: &str) -> bool {
        self.sources
            .iter()
            .any(|source| source.matches_filename(filename))
    }

    fn matches_path(&self, filename: &str, path: &Path) -> bool {
        self.sources
            .iter()
            .any(|source| source.matches_path(filename, path))
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        let filename = file
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");

        let mut results = Vec::new();
        for source in &self.sources {
            if source.matches_path(filename, &file.path) {
                results.extend(self.extract_from_source(source, file));
            }
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_navigate_json() {
        let json: serde_json::Value = serde_json::json!({
            "services": {
                "redis": {
                    "image": "redis:7.2"
                }
            }
        });
        assert_eq!(
            navigate_json(&json, "services.redis.image"),
            Some("redis:7.2".to_string())
        );
        assert_eq!(navigate_json(&json, "services.missing"), None);
    }

    #[test]
    fn test_navigate_yaml() {
        let yaml: crate::parse::YamlValue =
            serde_saphyr::from_str("services:\n  redis:\n    image: redis:7.2\n").unwrap();
        assert_eq!(
            navigate_yaml(&yaml, "services.redis.image"),
            Some("redis:7.2".to_string())
        );
    }

    #[test]
    fn test_navigate_toml() {
        let toml_value: toml::Value =
            toml::from_str("[services.redis]\nimage = \"redis:7.2\"\n").unwrap();
        assert_eq!(
            navigate_toml(&toml_value, "services.redis.image"),
            Some("redis:7.2".to_string())
        );
    }

    #[test]
    fn test_apply_pattern() {
        assert_eq!(
            apply_pattern("redis:7.2", None),
            Some("redis:7.2".to_string())
        );
        assert_eq!(
            apply_pattern("redis:7.2", Some(&Regex::new("redis:(.*)").unwrap())),
            Some("7.2".to_string())
        );
        assert_eq!(
            apply_pattern("redis:7.2", Some(&Regex::new("nginx:(.*)").unwrap())),
            None
        );
    }

    #[test]
    fn test_apply_pattern_no_capture_group() {
        assert_eq!(
            apply_pattern("hello world", Some(&Regex::new("hello").unwrap())),
            Some("hello".to_string())
        );
    }

    #[test]
    fn test_source_matches_filename_glob() {
        let source = compile_source(
            CustomSourceConfig {
                file: "*.json".into(),
                format: "json".into(),
                path: Some("custom.redis".into()),
                key: None,
                pattern: None,
                authority: "declared".into(),
            },
            &CustomExtractorConfig {
                concept: "redis-version".into(),
                display_name: "Redis Version".into(),
                category: "runtime-version".into(),
                value_type: "version".into(),
                source: vec![],
            },
            0,
            Some(Path::new(".conflic.toml")),
        )
        .unwrap();

        assert!(source.matches_filename("package.json"));
        assert!(!source.matches_filename("Dockerfile"));
    }

    #[test]
    fn test_source_matches_path_relative_glob() {
        let source = compile_source(
            CustomSourceConfig {
                file: "configs/*.json".into(),
                format: "json".into(),
                path: Some("custom.redis".into()),
                key: None,
                pattern: None,
                authority: "declared".into(),
            },
            &CustomExtractorConfig {
                concept: "redis-version".into(),
                display_name: "Redis Version".into(),
                category: "runtime-version".into(),
                value_type: "version".into(),
                source: vec![],
            },
            0,
            Some(Path::new(".conflic.toml")),
        )
        .unwrap();

        let path = Path::new("C:/repo/configs/app.json");
        assert!(source.matches_path("app.json", path));
    }

    #[test]
    fn test_compile_custom_extractor_reports_invalid_regex_as_diagnostic() {
        let config = CustomExtractorConfig {
            concept: "redis-version".into(),
            display_name: "Redis Version".into(),
            category: "runtime-version".into(),
            value_type: "version".into(),
            source: vec![CustomSourceConfig {
                file: "*.json".into(),
                format: "json".into(),
                path: Some("custom.redis".into()),
                key: None,
                pattern: Some("redis:(".into()),
                authority: "declared".into(),
            }],
        };

        let (extractors, diagnostics) =
            compile_custom_extractors(&[config], Some(Path::new("workspace/.conflic.toml")));

        assert!(
            extractors.is_empty(),
            "invalid sources should not be compiled"
        );
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].rule_id, CUSTOM_EXTRACTOR_CONFIG_RULE_ID);
        assert!(
            diagnostics[0].message.contains("invalid regex pattern"),
            "expected regex validation diagnostic, got {:?}",
            diagnostics
        );
    }

    #[test]
    fn test_compile_custom_extractor_reports_invalid_glob_as_diagnostic() {
        let config = CustomExtractorConfig {
            concept: "redis-version".into(),
            display_name: "Redis Version".into(),
            category: "runtime-version".into(),
            value_type: "version".into(),
            source: vec![CustomSourceConfig {
                file: "[*.json".into(),
                format: "json".into(),
                path: Some("custom.redis".into()),
                key: None,
                pattern: None,
                authority: "declared".into(),
            }],
        };

        let (extractors, diagnostics) =
            compile_custom_extractors(&[config], Some(Path::new("workspace/.conflic.toml")));

        assert!(
            extractors.is_empty(),
            "invalid sources should not be compiled"
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("invalid file glob")),
            "expected glob validation diagnostic, got {:?}",
            diagnostics
        );
    }

    #[test]
    fn test_compile_custom_extractor_reports_invalid_format_as_diagnostic() {
        let config = CustomExtractorConfig {
            concept: "redis-version".into(),
            display_name: "Redis Version".into(),
            category: "runtime-version".into(),
            value_type: "version".into(),
            source: vec![CustomSourceConfig {
                file: "docker-compose.yml".into(),
                format: "yamll".into(),
                path: Some("services.redis.image".into()),
                key: None,
                pattern: Some("redis:(.*)".into()),
                authority: "declared".into(),
            }],
        };

        let (extractors, diagnostics) =
            compile_custom_extractors(&[config], Some(Path::new("workspace/.conflic.toml")));

        assert!(
            extractors.is_empty(),
            "invalid formats should not produce compiled extractors"
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("invalid source format")),
            "expected source format validation diagnostic, got {:?}",
            diagnostics
        );
    }
}
