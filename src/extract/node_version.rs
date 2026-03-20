use crate::model::*;
use crate::parse::*;
use crate::parse::source_location::*;
use super::Extractor;
use std::path::PathBuf;

// --- .nvmrc extractor ---

pub struct NvmrcExtractor;

impl Extractor for NvmrcExtractor {
    fn id(&self) -> &str { "node-version-nvmrc" }
    fn description(&self) -> &str { "Node.js version from .nvmrc" }
    fn relevant_filenames(&self) -> Vec<&str> { vec![".nvmrc", ".node-version"] }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::PlainText(ref text) = file.content {
            if text.is_empty() {
                return vec![];
            }
            // Strip leading 'v' if present
            let version_str = text.strip_prefix('v').unwrap_or(text);
            let version = parse_version(version_str);
            vec![ConfigAssertion::new(
                SemanticConcept::node_version(),
                SemanticType::Version(version),
                text.clone(),
                SourceLocation {
                    file: file.path.clone(),
                    line: 1,
                    column: 0,
                    key_path: String::new(),
                },
                Authority::Advisory,
                self.id(),
            )]
        } else {
            vec![]
        }
    }
}

// --- package.json engines.node extractor ---

pub struct PackageJsonNodeExtractor;

impl Extractor for PackageJsonNodeExtractor {
    fn id(&self) -> &str { "node-version-package-json" }
    fn description(&self) -> &str { "Node.js version from package.json engines.node" }
    fn relevant_filenames(&self) -> Vec<&str> { vec!["package.json"] }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::Json(ref value) = file.content {
            if let Some(node_version) = value
                .get("engines")
                .and_then(|e| e.get("node"))
                .and_then(|n| n.as_str())
            {
                let version = parse_version(node_version);
                let line = find_line_for_json_key(&file.raw_text, "engines.node");
                return vec![ConfigAssertion::new(
                    SemanticConcept::node_version(),
                    SemanticType::Version(version),
                    node_version.to_string(),
                    SourceLocation {
                        file: file.path.clone(),
                        line,
                        column: 0,
                        key_path: "engines.node".into(),
                    },
                    Authority::Declared,
                    self.id(),
                )];
            }
        }
        vec![]
    }
}

// --- Dockerfile FROM node:* extractor ---

pub struct DockerfileNodeExtractor;

impl Extractor for DockerfileNodeExtractor {
    fn id(&self) -> &str { "node-version-dockerfile" }
    fn description(&self) -> &str { "Node.js version from Dockerfile FROM node:*" }
    fn relevant_filenames(&self) -> Vec<&str> { vec!["Dockerfile"] }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::Dockerfile(ref instructions) = file.content {
            let mut results = Vec::new();
            for instr in instructions {
                if instr.instruction == "FROM" {
                    if let Some(version) = extract_docker_image_version(&instr.arguments, "node") {
                        let authority = if instr.is_final_stage {
                            Authority::Enforced
                        } else {
                            Authority::Advisory
                        };
                        let key_path = if let Some(ref name) = instr.stage_name {
                            format!("FROM (stage: {})", name)
                        } else {
                            "FROM".into()
                        };
                        results.push(ConfigAssertion::new(
                            SemanticConcept::node_version(),
                            SemanticType::Version(parse_version(&version)),
                            instr.arguments.clone(),
                            SourceLocation {
                                file: file.path.clone(),
                                line: instr.line,
                                column: 0,
                                key_path,
                            },
                            authority,
                            self.id(),
                        ));
                    }
                }
            }
            return results;
        }
        vec![]
    }
}

// --- CI yaml node-version extractor ---

pub struct CiNodeExtractor;

impl Extractor for CiNodeExtractor {
    fn id(&self) -> &str { "node-version-ci" }
    fn description(&self) -> &str { "Node.js version from CI config" }
    fn relevant_filenames(&self) -> Vec<&str> { vec![] } // Matched by path, not filename

    fn matches_file(&self, filename: &str) -> bool {
        (filename.ends_with(".yml") || filename.ends_with(".yaml"))
            || filename == ".gitlab-ci.yml"
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        // Only process CI files (in .github/workflows, .gitlab-ci, .circleci)
        let path_str = file.path.to_string_lossy();
        let is_ci = path_str.contains(".github/workflows")
            || path_str.contains(".github\\workflows")
            || path_str.contains(".circleci")
            || file.path.file_name().is_some_and(|n| n == ".gitlab-ci.yml");

        if !is_ci {
            return vec![];
        }

        if let FileContent::Yaml(ref value) = file.content {
            return extract_node_versions_from_ci(value, &file.path, &file.raw_text, self.id());
        }
        vec![]
    }
}

fn extract_node_versions_from_ci(
    value: &serde_yml::Value,
    path: &PathBuf,
    raw_text: &str,
    extractor_id: &str,
) -> Vec<ConfigAssertion> {
    let mut results = Vec::new();

    // Search for node-version in strategy.matrix
    if let Some(mapping) = value.as_mapping() {
        for (_key, job_value) in mapping {
            find_node_version_recursive(job_value, path, raw_text, extractor_id, &mut results);
        }
    }

    results
}

fn find_node_version_recursive(
    value: &serde_yml::Value,
    path: &PathBuf,
    raw_text: &str,
    extractor_id: &str,
    results: &mut Vec<ConfigAssertion>,
) {
    match value {
        serde_yml::Value::Mapping(map) => {
            for (key, val) in map {
                let key_str = key.as_str().unwrap_or("");
                if key_str == "node-version" || key_str == "node_version" {
                    extract_version_values(val, path, raw_text, extractor_id, key_str, results);
                } else {
                    find_node_version_recursive(val, path, raw_text, extractor_id, results);
                }
            }
        }
        serde_yml::Value::Sequence(seq) => {
            for item in seq {
                find_node_version_recursive(item, path, raw_text, extractor_id, results);
            }
        }
        _ => {}
    }
}

fn extract_version_values(
    value: &serde_yml::Value,
    path: &PathBuf,
    raw_text: &str,
    extractor_id: &str,
    key_name: &str,
    results: &mut Vec<ConfigAssertion>,
) {
    let line = find_line_for_key(raw_text, key_name);

    match value {
        serde_yml::Value::Sequence(seq) => {
            for item in seq {
                if let Some(v) = yaml_value_to_string(item) {
                    let version = parse_version(&v);
                    results.push(
                        ConfigAssertion::new(
                            SemanticConcept::node_version(),
                            SemanticType::Version(version),
                            v,
                            SourceLocation {
                                file: path.clone(),
                                line,
                                column: 0,
                                key_path: format!("matrix.{}", key_name),
                            },
                            Authority::Enforced,
                            extractor_id,
                        )
                        .with_matrix(true),
                    );
                }
            }
        }
        _ => {
            if let Some(v) = yaml_value_to_string(value) {
                let version = parse_version(&v);
                results.push(ConfigAssertion::new(
                    SemanticConcept::node_version(),
                    SemanticType::Version(version),
                    v,
                    SourceLocation {
                        file: path.clone(),
                        line,
                        column: 0,
                        key_path: key_name.into(),
                    },
                    Authority::Enforced,
                    extractor_id,
                ));
            }
        }
    }
}

// --- .tool-versions extractor ---

pub struct ToolVersionsNodeExtractor;

impl Extractor for ToolVersionsNodeExtractor {
    fn id(&self) -> &str { "node-version-tool-versions" }
    fn description(&self) -> &str { "Node.js version from .tool-versions" }
    fn relevant_filenames(&self) -> Vec<&str> { vec![".tool-versions"] }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::PlainText(_) = file.content {
            // .tool-versions has multiple lines like "nodejs 20.11.0"
            for (line_num, line) in file.raw_text.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.starts_with("nodejs ") || trimmed.starts_with("node ") {
                    let version_str = trimmed
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("");
                    if !version_str.is_empty() {
                        let version = parse_version(version_str);
                        return vec![ConfigAssertion::new(
                            SemanticConcept::node_version(),
                            SemanticType::Version(version),
                            version_str.to_string(),
                            SourceLocation {
                                file: file.path.clone(),
                                line: line_num + 1,
                                column: 0,
                                key_path: "nodejs".into(),
                            },
                            Authority::Advisory,
                            self.id(),
                        )];
                    }
                }
            }
        }
        vec![]
    }
}

// --- Helpers ---

/// Extract version from a Docker image reference like "node:22-alpine" or "node:20.11.0"
pub fn extract_docker_image_version(from_args: &str, image_name: &str) -> Option<String> {
    // FROM image:tag [AS name]
    let image_part = from_args.split_whitespace().next()?;
    let (image, tag) = image_part.split_once(':')?;

    // Handle registry prefixes: docker.io/library/node:20
    let image_basename = image.rsplit('/').next()?;

    if image_basename == image_name {
        Some(tag.to_string())
    } else {
        None
    }
}

fn yaml_value_to_string(value: &serde_yml::Value) -> Option<String> {
    match value {
        serde_yml::Value::String(s) => Some(s.clone()),
        serde_yml::Value::Number(n) => Some(n.to_string()),
        serde_yml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}
