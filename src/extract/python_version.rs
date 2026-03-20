use super::Extractor;
use super::runtime_version::{
    CiYamlVersionStrategy, DockerKeyPathStrategy, DockerRuntimeVersionStrategy,
    PlainTextNormalizer, PlainTextVersionStrategy, RuntimeVersionKind, VersionExtractionStrategy,
    YamlScalarStrategy,
};
use crate::model::*;
use crate::parse::source_location::*;
use crate::parse::*;
use regex::Regex;
use std::sync::LazyLock;

static PYTHON_REQUIRES_SUFFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([><=!~]+)(\d+\.\d+)$").unwrap());

// --- .python-version extractor ---

pub struct PythonVersionFileExtractor;

impl Extractor for PythonVersionFileExtractor {
    fn id(&self) -> &str {
        "python-version-file"
    }
    fn description(&self) -> &str {
        "Python version from .python-version"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec![".python-version"]
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        PlainTextVersionStrategy {
            runtime: RuntimeVersionKind::Python,
            authority: Authority::Advisory,
            extractor_id: self.id(),
            normalizer: PlainTextNormalizer::Identity,
        }
        .extract(file)
    }
}

// --- pyproject.toml extractor ---

pub struct PyprojectExtractor;

impl Extractor for PyprojectExtractor {
    fn id(&self) -> &str {
        "python-version-pyproject"
    }
    fn description(&self) -> &str {
        "Python version from pyproject.toml"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec!["pyproject.toml"]
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::Toml(ref value) = file.content {
            if let Some(req) = value
                .get("project")
                .and_then(|project| project.get("requires-python"))
                .and_then(|requires| requires.as_str())
            {
                let version = parse_python_requires(req);
                let line = find_line_for_key(&file.raw_text, "requires-python");
                return vec![ConfigAssertion::new(
                    SemanticConcept::python_version(),
                    SemanticType::Version(version),
                    req.to_string(),
                    SourceLocation {
                        file: file.path.clone(),
                        line,
                        column: 0,
                        key_path: "project.requires-python".into(),
                    },
                    Authority::Declared,
                    self.id(),
                )];
            }

            if let Some(req) = value
                .get("tool")
                .and_then(|tool| tool.get("poetry"))
                .and_then(|poetry| poetry.get("dependencies"))
                .and_then(|dependencies| dependencies.get("python"))
                .and_then(|python| python.as_str())
            {
                let version = parse_python_requires(req);
                let line = find_line_for_key(&file.raw_text, "python");
                return vec![ConfigAssertion::new(
                    SemanticConcept::python_version(),
                    SemanticType::Version(version),
                    req.to_string(),
                    SourceLocation {
                        file: file.path.clone(),
                        line,
                        column: 0,
                        key_path: "tool.poetry.dependencies.python".into(),
                    },
                    Authority::Declared,
                    self.id(),
                )];
            }
        }
        vec![]
    }
}

// --- Dockerfile FROM python:* ---

pub struct DockerfilePythonExtractor;

impl Extractor for DockerfilePythonExtractor {
    fn id(&self) -> &str {
        "python-version-dockerfile"
    }
    fn description(&self) -> &str {
        "Python version from Dockerfile FROM python:*"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec!["Dockerfile"]
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        DockerRuntimeVersionStrategy {
            runtime: RuntimeVersionKind::Python,
            extractor_id: self.id(),
            image_names: &["python"],
            key_path: DockerKeyPathStrategy::From,
        }
        .extract(file)
    }
}

// --- CI yaml python-version ---

pub struct CiPythonExtractor;

impl Extractor for CiPythonExtractor {
    fn id(&self) -> &str {
        "python-version-ci"
    }
    fn description(&self) -> &str {
        "Python version from CI config"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec![]
    }

    fn matches_file(&self, filename: &str) -> bool {
        (filename.ends_with(".yml") || filename.ends_with(".yaml")) || filename == ".gitlab-ci.yml"
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        CiYamlVersionStrategy {
            runtime: RuntimeVersionKind::Python,
            extractor_id: self.id(),
            keys: &["python-version", "python_version"],
            scalar_strategy: YamlScalarStrategy::StringOrNumber,
            scan_root_keys: true,
        }
        .extract(file)
    }
}

/// Convert Python version specifiers to something our version parser handles.
/// ">=3.10,<3.12" -> ">=3.10.0 <3.12.0"
fn parse_python_requires(req: &str) -> VersionSpec {
    let normalized = req
        .split(',')
        .map(|part| {
            let part = part.trim();
            if let Some(caps) = PYTHON_REQUIRES_SUFFIX_RE.captures(part) {
                format!("{}{}.0", &caps[1], &caps[2])
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    parse_version(&normalized)
}
