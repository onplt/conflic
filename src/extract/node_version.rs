use super::Extractor;
use super::runtime_version::{
    CiYamlVersionStrategy, DockerKeyPathStrategy, DockerRuntimeVersionStrategy,
    PlainTextNormalizer, PlainTextVersionStrategy, RuntimeVersionKind, ToolVersionsVersionStrategy,
    VersionExtractionStrategy, YamlScalarStrategy,
};
use crate::model::*;
use crate::parse::source_location::*;
use crate::parse::*;

#[cfg(test)]
use super::runtime_version::extract_docker_image_version;

// --- .nvmrc extractor ---

pub struct NvmrcExtractor;

impl Extractor for NvmrcExtractor {
    fn id(&self) -> &str {
        "node-version-nvmrc"
    }
    fn description(&self) -> &str {
        "Node.js version from .nvmrc"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec![".nvmrc", ".node-version"]
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        PlainTextVersionStrategy {
            runtime: RuntimeVersionKind::Node,
            authority: Authority::Advisory,
            extractor_id: self.id(),
            normalizer: PlainTextNormalizer::StripPrefixChar('v'),
        }
        .extract(file)
    }
}

// --- package.json engines.node extractor ---

pub struct PackageJsonNodeExtractor;

impl Extractor for PackageJsonNodeExtractor {
    fn id(&self) -> &str {
        "node-version-package-json"
    }
    fn description(&self) -> &str {
        "Node.js version from package.json engines.node"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec!["package.json"]
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::Json(ref value) = file.content
            && let Some(node_version) = value
                .get("engines")
                .and_then(|e| e.get("node"))
                .and_then(|n| n.as_str())
        {
            let version = parse_version(node_version);
            let (location, span) =
                match json_value_location(&file.path, &file.raw_text, "engines.node") {
                    Some((location, span)) => (location, Some(span)),
                    None => (
                        SourceLocation {
                            file: file.path.clone(),
                            line: find_line_for_json_key(&file.raw_text, "engines.node"),
                            column: 0,
                            key_path: "engines.node".into(),
                        },
                        None,
                    ),
                };
            return vec![
                ConfigAssertion::new(
                    SemanticConcept::node_version(),
                    SemanticType::Version(version),
                    node_version.to_string(),
                    location,
                    Authority::Declared,
                    self.id(),
                )
                .with_optional_span(span),
            ];
        }
        vec![]
    }
}

// --- Dockerfile FROM node:* extractor ---

pub struct DockerfileNodeExtractor;

impl Extractor for DockerfileNodeExtractor {
    fn id(&self) -> &str {
        "node-version-dockerfile"
    }
    fn description(&self) -> &str {
        "Node.js version from Dockerfile FROM node:*"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec!["Dockerfile"]
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        DockerRuntimeVersionStrategy {
            runtime: RuntimeVersionKind::Node,
            extractor_id: self.id(),
            image_names: &["node"],
            key_path: DockerKeyPathStrategy::StageAwareFrom,
        }
        .extract(file)
    }
}

// --- CI yaml node-version extractor ---

pub struct CiNodeExtractor;

impl Extractor for CiNodeExtractor {
    fn id(&self) -> &str {
        "node-version-ci"
    }
    fn description(&self) -> &str {
        "Node.js version from CI config"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec![]
    }

    fn matches_file(&self, filename: &str) -> bool {
        (filename.ends_with(".yml") || filename.ends_with(".yaml")) || filename == ".gitlab-ci.yml"
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        CiYamlVersionStrategy {
            runtime: RuntimeVersionKind::Node,
            extractor_id: self.id(),
            keys: &["node-version", "node_version"],
            scalar_strategy: YamlScalarStrategy::StringNumberOrBool,
            scan_root_keys: false,
        }
        .extract(file)
    }
}

// --- .tool-versions extractor ---

pub struct ToolVersionsNodeExtractor;

impl Extractor for ToolVersionsNodeExtractor {
    fn id(&self) -> &str {
        "node-version-tool-versions"
    }
    fn description(&self) -> &str {
        "Node.js version from .tool-versions"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec![".tool-versions"]
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        ToolVersionsVersionStrategy {
            runtime: RuntimeVersionKind::Node,
            extractor_id: self.id(),
            tool_names: &["nodejs", "node"],
            authority: Authority::Advisory,
            key_path: "nodejs",
        }
        .extract(file)
    }
}

#[cfg(test)]
mod tests {
    use super::extract_docker_image_version;

    #[test]
    fn test_extract_docker_image_version_skips_platform_flags() {
        assert_eq!(
            extract_docker_image_version("--platform=linux/amd64 node:20-alpine", "node"),
            Some("20-alpine".to_string())
        );
    }

    #[test]
    fn test_extract_docker_image_version_handles_registry_ports() {
        assert_eq!(
            extract_docker_image_version("localhost:5000/node:20-alpine", "node"),
            Some("20-alpine".to_string())
        );
    }
}
