use super::Extractor;
use super::runtime_version::{
    DockerKeyPathStrategy, DockerRuntimeVersionStrategy, RuntimeVersionKind,
    VersionExtractionStrategy,
};
use crate::model::*;
use crate::parse::*;

// --- go.mod extractor ---

pub struct GoModExtractor;

impl Extractor for GoModExtractor {
    fn id(&self) -> &str {
        "go-version-gomod"
    }
    fn description(&self) -> &str {
        "Go version from go.mod"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec!["go.mod"]
    }

    fn matches_file(&self, filename: &str) -> bool {
        filename == "go.mod"
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        for (line_num, line) in file.raw_text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("go ") {
                let version_str = trimmed.strip_prefix("go ").unwrap().trim();
                let version = parse_version(version_str);
                return vec![ConfigAssertion::new(
                    SemanticConcept::go_version(),
                    SemanticType::Version(version),
                    version_str.to_string(),
                    SourceLocation {
                        file: file.path.clone(),
                        line: line_num + 1,
                        column: 0,
                        key_path: "go".into(),
                    },
                    Authority::Declared,
                    self.id(),
                )];
            }
        }
        vec![]
    }
}

// --- Dockerfile FROM golang:* ---

pub struct DockerfileGoExtractor;

impl Extractor for DockerfileGoExtractor {
    fn id(&self) -> &str {
        "go-version-dockerfile"
    }
    fn description(&self) -> &str {
        "Go version from Dockerfile FROM golang:*"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec!["Dockerfile"]
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        DockerRuntimeVersionStrategy {
            runtime: RuntimeVersionKind::Go,
            extractor_id: self.id(),
            image_names: &["golang"],
            key_path: DockerKeyPathStrategy::From,
        }
        .extract(file)
    }
}
