use regex::Regex;
use super::Extractor;
use crate::config::CustomExtractorConfig;
use crate::model::assertion::{Authority, ConfigAssertion, SourceLocation};
use crate::model::concept::{ConceptCategory, SemanticConcept};
use crate::model::semantic_type::{self, SemanticType};
use crate::parse::{FileContent, ParsedFile};

/// A dynamically-defined extractor loaded from `.conflic.toml`.
pub struct CustomExtractor {
    config: CustomExtractorConfig,
}

impl CustomExtractor {
    pub fn new(config: CustomExtractorConfig) -> Self {
        Self { config }
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
            "port" => {
                if let Some(port) = semantic_type::parse_port(raw) {
                    SemanticType::Port(port)
                } else {
                    SemanticType::StringValue(raw.to_string())
                }
            }
            "boolean" => {
                if let Some(b) = semantic_type::normalize_boolean(raw) {
                    SemanticType::Boolean(b)
                } else {
                    SemanticType::StringValue(raw.to_string())
                }
            }
            _ => SemanticType::StringValue(raw.to_string()),
        }
    }

    fn extract_from_source(
        &self,
        source: &crate::config::CustomSourceConfig,
        file: &ParsedFile,
    ) -> Vec<ConfigAssertion> {
        let authority = match source.authority.as_str() {
            "enforced" => Authority::Enforced,
            "declared" => Authority::Declared,
            _ => Authority::Advisory,
        };

        match source.format.as_str() {
            "json" => self.extract_from_json(source, file, authority),
            "yaml" => self.extract_from_yaml(source, file, authority),
            "toml" => self.extract_from_toml(source, file, authority),
            "env" => self.extract_from_env(source, file, authority),
            "plain" => self.extract_from_plain(file, authority),
            "dockerfile" => self.extract_from_dockerfile(source, file, authority),
            _ => vec![],
        }
    }

    fn extract_from_json(
        &self,
        source: &crate::config::CustomSourceConfig,
        file: &ParsedFile,
        authority: Authority,
    ) -> Vec<ConfigAssertion> {
        if let FileContent::Json(ref value) = file.content {
            if let Some(ref path) = source.path {
                if let Some(raw) = navigate_json(value, path) {
                    let raw = apply_pattern(&raw, source.pattern.as_deref());
                    if let Some(raw) = raw {
                        return vec![self.make_assertion(&raw, path, file, authority)];
                    }
                }
            }
        }
        vec![]
    }

    fn extract_from_yaml(
        &self,
        source: &crate::config::CustomSourceConfig,
        file: &ParsedFile,
        authority: Authority,
    ) -> Vec<ConfigAssertion> {
        if let FileContent::Yaml(ref value) = file.content {
            if let Some(ref path) = source.path {
                if let Some(raw) = navigate_yaml(value, path) {
                    let raw = apply_pattern(&raw, source.pattern.as_deref());
                    if let Some(raw) = raw {
                        return vec![self.make_assertion(&raw, path, file, authority)];
                    }
                }
            }
        }
        vec![]
    }

    fn extract_from_toml(
        &self,
        source: &crate::config::CustomSourceConfig,
        file: &ParsedFile,
        authority: Authority,
    ) -> Vec<ConfigAssertion> {
        if let FileContent::Toml(ref value) = file.content {
            if let Some(ref path) = source.path {
                if let Some(raw) = navigate_toml(value, path) {
                    let raw = apply_pattern(&raw, source.pattern.as_deref());
                    if let Some(raw) = raw {
                        return vec![self.make_assertion(&raw, path, file, authority)];
                    }
                }
            }
        }
        vec![]
    }

    fn extract_from_env(
        &self,
        source: &crate::config::CustomSourceConfig,
        file: &ParsedFile,
        authority: Authority,
    ) -> Vec<ConfigAssertion> {
        if let FileContent::Env(ref entries) = file.content {
            if let Some(ref key) = source.key {
                for entry in entries {
                    if entry.key == *key {
                        let raw = apply_pattern(&entry.value, source.pattern.as_deref());
                        if let Some(raw) = raw {
                            let value = self.parse_value(&raw);
                            let loc = SourceLocation {
                                file: file.path.clone(),
                                line: entry.line,
                                column: 0,
                                key_path: key.clone(),
                            };
                            return vec![ConfigAssertion::new(
                                self.concept(),
                                value,
                                raw,
                                loc,
                                authority,
                                self.id(),
                            )];
                        }
                    }
                }
            }
        }
        vec![]
    }

    fn extract_from_plain(
        &self,
        file: &ParsedFile,
        authority: Authority,
    ) -> Vec<ConfigAssertion> {
        if let FileContent::PlainText(ref text) = file.content {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                let value = self.parse_value(trimmed);
                let loc = SourceLocation {
                    file: file.path.clone(),
                    line: 1,
                    column: 0,
                    key_path: String::new(),
                };
                return vec![ConfigAssertion::new(
                    self.concept(),
                    value,
                    trimmed.to_string(),
                    loc,
                    authority,
                    self.id(),
                )];
            }
        }
        vec![]
    }

    fn extract_from_dockerfile(
        &self,
        source: &crate::config::CustomSourceConfig,
        file: &ParsedFile,
        authority: Authority,
    ) -> Vec<ConfigAssertion> {
        if let FileContent::Dockerfile(ref instructions) = file.content {
            for instr in instructions {
                if instr.instruction == "FROM" {
                    let raw = apply_pattern(&instr.arguments, source.pattern.as_deref());
                    if let Some(raw) = raw {
                        let value = self.parse_value(&raw);
                        let loc = SourceLocation {
                            file: file.path.clone(),
                            line: instr.line,
                            column: 0,
                            key_path: "FROM".to_string(),
                        };
                        return vec![ConfigAssertion::new(
                            self.concept(),
                            value,
                            raw,
                            loc,
                            authority,
                            self.id(),
                        )];
                    }
                }
            }
        }
        vec![]
    }

    fn make_assertion(
        &self,
        raw: &str,
        key_path: &str,
        file: &ParsedFile,
        authority: Authority,
    ) -> ConfigAssertion {
        let value = self.parse_value(raw);
        let line = crate::parse::source_location::find_line_for_key(&file.raw_text, key_path);
        let loc = SourceLocation {
            file: file.path.clone(),
            line,
            column: 0,
            key_path: key_path.to_string(),
        };
        ConfigAssertion::new(self.concept(), value, raw.to_string(), loc, authority, self.id())
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
        self.config
            .source
            .iter()
            .map(|s| s.file.as_str())
            .collect()
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        let filename = file
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        let mut results = Vec::new();
        for source in &self.config.source {
            if filename == source.file || filename.starts_with(&source.file) {
                results.extend(self.extract_from_source(source, file));
            }
        }
        results
    }
}

// --- Dot-path navigation helpers ---

/// Navigate a JSON value by dot-separated path (e.g., "services.redis.image").
fn navigate_json(value: &serde_json::Value, path: &str) -> Option<String> {
    let mut current = value;
    for key in path.split('.') {
        current = current.get(key)?;
    }
    match current {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => Some(current.to_string()),
    }
}

/// Navigate a YAML value by dot-separated path.
fn navigate_yaml(value: &serde_yml::Value, path: &str) -> Option<String> {
    let mut current = value;
    for key in path.split('.') {
        current = current.get(key)?;
    }
    match current {
        serde_yml::Value::String(s) => Some(s.clone()),
        serde_yml::Value::Number(n) => Some(n.to_string()),
        serde_yml::Value::Bool(b) => Some(b.to_string()),
        _ => Some(format!("{:?}", current)),
    }
}

/// Navigate a TOML value by dot-separated path.
fn navigate_toml(value: &toml::Value, path: &str) -> Option<String> {
    let mut current = value;
    for key in path.split('.') {
        current = current.get(key)?;
    }
    match current {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Integer(n) => Some(n.to_string()),
        toml::Value::Float(n) => Some(n.to_string()),
        toml::Value::Boolean(b) => Some(b.to_string()),
        _ => Some(current.to_string()),
    }
}

/// Apply an optional regex pattern to extract a capture group.
/// If pattern is None, returns the raw value unchanged.
/// If pattern has a capture group, returns group 1.
fn apply_pattern(raw: &str, pattern: Option<&str>) -> Option<String> {
    match pattern {
        None => Some(raw.to_string()),
        Some(pat) => {
            let re = Regex::new(pat).ok()?;
            let caps = re.captures(raw)?;
            if let Some(m) = caps.get(1) {
                Some(m.as_str().to_string())
            } else {
                Some(caps.get(0)?.as_str().to_string())
            }
        }
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
        let yaml: serde_yml::Value = serde_yml::from_str(
            "services:\n  redis:\n    image: redis:7.2\n"
        ).unwrap();
        assert_eq!(
            navigate_yaml(&yaml, "services.redis.image"),
            Some("redis:7.2".to_string())
        );
    }

    #[test]
    fn test_navigate_toml() {
        let toml_val: toml::Value = toml::from_str(
            "[services.redis]\nimage = \"redis:7.2\"\n"
        ).unwrap();
        assert_eq!(
            navigate_toml(&toml_val, "services.redis.image"),
            Some("redis:7.2".to_string())
        );
    }

    #[test]
    fn test_apply_pattern() {
        assert_eq!(apply_pattern("redis:7.2", None), Some("redis:7.2".to_string()));
        assert_eq!(
            apply_pattern("redis:7.2", Some("redis:(.*)")),
            Some("7.2".to_string())
        );
        assert_eq!(apply_pattern("redis:7.2", Some("nginx:(.*)")), None);
    }

    #[test]
    fn test_apply_pattern_no_capture_group() {
        assert_eq!(
            apply_pattern("hello world", Some("hello")),
            Some("hello".to_string())
        );
    }
}
