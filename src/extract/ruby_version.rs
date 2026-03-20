use super::Extractor;
use super::runtime_version::{
    CiYamlVersionStrategy, DockerKeyPathStrategy, DockerRuntimeVersionStrategy,
    PlainTextNormalizer, PlainTextVersionStrategy, RuntimeVersionKind, ToolVersionsVersionStrategy,
    VersionExtractionStrategy, YamlScalarStrategy,
};
use crate::model::*;
use crate::parse::{FileContent, ParsedFile};
use regex::Regex;
use std::sync::LazyLock;

static GEMFILE_RUBY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\s*ruby\s+['"]([^'"]+)['"]"#).unwrap());

// --- .ruby-version extractor ---

pub struct RubyVersionFileExtractor;

impl Extractor for RubyVersionFileExtractor {
    fn id(&self) -> &str {
        "ruby-version-file"
    }
    fn description(&self) -> &str {
        "Ruby version from .ruby-version"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec![".ruby-version"]
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        PlainTextVersionStrategy {
            runtime: RuntimeVersionKind::Ruby,
            authority: Authority::Advisory,
            extractor_id: self.id(),
            normalizer: PlainTextNormalizer::StripPrefix("ruby-"),
        }
        .extract(file)
    }
}

// --- Gemfile ruby version extractor ---

pub struct GemfileExtractor;

impl Extractor for GemfileExtractor {
    fn id(&self) -> &str {
        "ruby-version-gemfile"
    }
    fn description(&self) -> &str {
        "Ruby version from Gemfile"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec!["Gemfile"]
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::PlainText(_) = file.content {
            for (line_num, line) in file.raw_text.lines().enumerate() {
                if let Some(caps) = GEMFILE_RUBY_RE.captures(line) {
                    let version_str = caps[1].to_string();
                    let version = parse_version(&version_str);
                    return vec![ConfigAssertion::new(
                        SemanticConcept::ruby_version(),
                        SemanticType::Version(version),
                        version_str,
                        SourceLocation {
                            file: file.path.clone(),
                            line: line_num + 1,
                            column: 0,
                            key_path: "ruby".into(),
                        },
                        Authority::Declared,
                        self.id(),
                    )];
                }
            }
        }
        vec![]
    }
}

// --- Dockerfile FROM ruby:* ---

pub struct DockerfileRubyExtractor;

impl Extractor for DockerfileRubyExtractor {
    fn id(&self) -> &str {
        "ruby-version-dockerfile"
    }
    fn description(&self) -> &str {
        "Ruby version from Dockerfile FROM ruby:*"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec!["Dockerfile"]
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        DockerRuntimeVersionStrategy {
            runtime: RuntimeVersionKind::Ruby,
            extractor_id: self.id(),
            image_names: &["ruby"],
            key_path: DockerKeyPathStrategy::From,
        }
        .extract(file)
    }
}

// --- .tool-versions ruby extractor ---

pub struct ToolVersionsRubyExtractor;

impl Extractor for ToolVersionsRubyExtractor {
    fn id(&self) -> &str {
        "ruby-version-tool-versions"
    }
    fn description(&self) -> &str {
        "Ruby version from .tool-versions"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec![".tool-versions"]
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        ToolVersionsVersionStrategy {
            runtime: RuntimeVersionKind::Ruby,
            extractor_id: self.id(),
            tool_names: &["ruby"],
            authority: Authority::Advisory,
            key_path: "ruby",
        }
        .extract(file)
    }
}

// --- CI yaml ruby-version ---

pub struct CiRubyExtractor;

impl Extractor for CiRubyExtractor {
    fn id(&self) -> &str {
        "ruby-version-ci"
    }
    fn description(&self) -> &str {
        "Ruby version from CI config"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec![]
    }

    fn matches_file(&self, filename: &str) -> bool {
        (filename.ends_with(".yml") || filename.ends_with(".yaml")) || filename == ".gitlab-ci.yml"
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        CiYamlVersionStrategy {
            runtime: RuntimeVersionKind::Ruby,
            extractor_id: self.id(),
            keys: &["ruby-version", "ruby_version"],
            scalar_strategy: YamlScalarStrategy::StringOrNumber,
            scan_root_keys: true,
        }
        .extract(file)
    }
}
