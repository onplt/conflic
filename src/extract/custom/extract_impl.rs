use crate::extract::Extractor;
use crate::model::assertion::{Authority, ConfigAssertion, SourceLocation};
use crate::parse::{FileContent, ParsedFile};

use super::navigation_impl::{apply_pattern, navigate_json, navigate_toml, navigate_yaml};
use super::{CompiledCustomSource, CustomExtractor};

impl CustomExtractor {
    pub(super) fn extract_from_source(
        &self,
        source: &CompiledCustomSource,
        file: &ParsedFile,
    ) -> Vec<ConfigAssertion> {
        let authority = match source.config.authority.as_str() {
            "enforced" => Authority::Enforced,
            "declared" => Authority::Declared,
            _ => Authority::Advisory,
        };

        match source.config.format.as_str() {
            "json" => self.extract_from_json(source, file, authority),
            "yaml" => self.extract_from_yaml(source, file, authority),
            "toml" => self.extract_from_toml(source, file, authority),
            "env" => self.extract_from_env(source, file, authority),
            "plain" => self.extract_from_plain(source, file, authority),
            "dockerfile" => self.extract_from_dockerfile(source, file, authority),
            _ => vec![],
        }
    }

    fn extract_from_json(
        &self,
        source: &CompiledCustomSource,
        file: &ParsedFile,
        authority: Authority,
    ) -> Vec<ConfigAssertion> {
        if let FileContent::Json(ref value) = file.content
            && let Some(ref path) = source.config.path
            && let Some(raw) = navigate_json(value, path)
            && let Some(raw) = apply_pattern(&raw, source.pattern_regex.as_ref())
        {
            let parsed_value = self.parse_value(&raw);
            let assertion = if let Some((location, span)) =
                crate::parse::source_location::json_value_location(&file.path, &file.raw_text, path)
            {
                ConfigAssertion::new(
                    self.concept(),
                    parsed_value,
                    raw,
                    location,
                    authority,
                    self.id(),
                )
                .with_span(span)
            } else {
                self.make_assertion(&raw, path, file, authority)
            };
            return vec![assertion];
        }
        vec![]
    }

    fn extract_from_yaml(
        &self,
        source: &CompiledCustomSource,
        file: &ParsedFile,
        authority: Authority,
    ) -> Vec<ConfigAssertion> {
        if let FileContent::Yaml(ref value) = file.content
            && let Some(ref path) = source.config.path
            && let Some(raw) = navigate_yaml(value, path)
            && let Some(raw) = apply_pattern(&raw, source.pattern_regex.as_ref())
        {
            return vec![self.make_assertion(&raw, path, file, authority)];
        }
        vec![]
    }

    fn extract_from_toml(
        &self,
        source: &CompiledCustomSource,
        file: &ParsedFile,
        authority: Authority,
    ) -> Vec<ConfigAssertion> {
        if let FileContent::Toml(ref value) = file.content
            && let Some(ref path) = source.config.path
            && let Some(raw) = navigate_toml(value, path)
            && let Some(raw) = apply_pattern(&raw, source.pattern_regex.as_ref())
        {
            return vec![self.make_assertion(&raw, path, file, authority)];
        }
        vec![]
    }

    fn extract_from_env(
        &self,
        source: &CompiledCustomSource,
        file: &ParsedFile,
        authority: Authority,
    ) -> Vec<ConfigAssertion> {
        if let FileContent::Env(ref entries) = file.content
            && let Some(ref key) = source.config.key
        {
            for entry in entries {
                if entry.key == *key
                    && let Some(raw) = apply_pattern(&entry.value, source.pattern_regex.as_ref())
                {
                    let value = self.parse_value(&raw);
                    let location = SourceLocation {
                        file: file.path.clone(),
                        line: entry.line,
                        column: 0,
                        key_path: key.clone(),
                    };
                    return vec![ConfigAssertion::new(
                        self.concept(),
                        value,
                        raw,
                        location,
                        authority,
                        self.id(),
                    )];
                }
            }
        }
        vec![]
    }

    fn extract_from_plain(
        &self,
        source: &CompiledCustomSource,
        file: &ParsedFile,
        authority: Authority,
    ) -> Vec<ConfigAssertion> {
        if let FileContent::PlainText(ref text) = file.content {
            let trimmed = text.trim();
            if let Some(raw) = apply_pattern(trimmed, source.pattern_regex.as_ref())
                && !raw.is_empty()
            {
                let value = self.parse_value(&raw);
                let location = SourceLocation {
                    file: file.path.clone(),
                    line: 1,
                    column: 0,
                    key_path: String::new(),
                };
                return vec![ConfigAssertion::new(
                    self.concept(),
                    value,
                    raw,
                    location,
                    authority,
                    self.id(),
                )];
            }
        }
        vec![]
    }

    fn extract_from_dockerfile(
        &self,
        source: &CompiledCustomSource,
        file: &ParsedFile,
        authority: Authority,
    ) -> Vec<ConfigAssertion> {
        if let FileContent::Dockerfile(ref instructions) = file.content {
            for instruction in instructions {
                if instruction.instruction == "FROM"
                    && let Some(raw) =
                        apply_pattern(&instruction.arguments, source.pattern_regex.as_ref())
                {
                    let value = self.parse_value(&raw);
                    let location = SourceLocation {
                        file: file.path.clone(),
                        line: instruction.line,
                        column: 0,
                        key_path: "FROM".to_string(),
                    };
                    return vec![ConfigAssertion::new(
                        self.concept(),
                        value,
                        raw,
                        location,
                        authority,
                        self.id(),
                    )];
                }
            }
        }
        vec![]
    }
}
