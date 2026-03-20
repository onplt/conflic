use crate::model::*;
use crate::parse::*;
use crate::parse::source_location::*;
use super::Extractor;
use super::node_version::extract_docker_image_version;

// --- .python-version extractor ---

pub struct PythonVersionFileExtractor;

impl Extractor for PythonVersionFileExtractor {
    fn id(&self) -> &str { "python-version-file" }
    fn description(&self) -> &str { "Python version from .python-version" }
    fn relevant_filenames(&self) -> Vec<&str> { vec![".python-version"] }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::PlainText(ref text) = file.content {
            if text.is_empty() {
                return vec![];
            }
            let version = parse_version(text);
            vec![ConfigAssertion::new(
                SemanticConcept::python_version(),
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

// --- pyproject.toml extractor ---

pub struct PyprojectExtractor;

impl Extractor for PyprojectExtractor {
    fn id(&self) -> &str { "python-version-pyproject" }
    fn description(&self) -> &str { "Python version from pyproject.toml" }
    fn relevant_filenames(&self) -> Vec<&str> { vec!["pyproject.toml"] }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::Toml(ref value) = file.content {
            // Try project.requires-python
            if let Some(req) = value
                .get("project")
                .and_then(|p| p.get("requires-python"))
                .and_then(|r| r.as_str())
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

            // Try tool.poetry.dependencies.python
            if let Some(req) = value
                .get("tool")
                .and_then(|t| t.get("poetry"))
                .and_then(|p| p.get("dependencies"))
                .and_then(|d| d.get("python"))
                .and_then(|p| p.as_str())
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
    fn id(&self) -> &str { "python-version-dockerfile" }
    fn description(&self) -> &str { "Python version from Dockerfile FROM python:*" }
    fn relevant_filenames(&self) -> Vec<&str> { vec!["Dockerfile"] }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::Dockerfile(ref instructions) = file.content {
            let mut results = Vec::new();
            for instr in instructions {
                if instr.instruction == "FROM" {
                    if let Some(version) = extract_docker_image_version(&instr.arguments, "python") {
                        let authority = if instr.is_final_stage {
                            Authority::Enforced
                        } else {
                            Authority::Advisory
                        };
                        results.push(ConfigAssertion::new(
                            SemanticConcept::python_version(),
                            SemanticType::Version(parse_version(&version)),
                            instr.arguments.clone(),
                            SourceLocation {
                                file: file.path.clone(),
                                line: instr.line,
                                column: 0,
                                key_path: "FROM".into(),
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

// --- CI yaml python-version ---

pub struct CiPythonExtractor;

impl Extractor for CiPythonExtractor {
    fn id(&self) -> &str { "python-version-ci" }
    fn description(&self) -> &str { "Python version from CI config" }
    fn relevant_filenames(&self) -> Vec<&str> { vec![] }

    fn matches_file(&self, filename: &str) -> bool {
        (filename.ends_with(".yml") || filename.ends_with(".yaml"))
            || filename == ".gitlab-ci.yml"
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        let path_str = file.path.to_string_lossy();
        let is_ci = path_str.contains(".github/workflows")
            || path_str.contains(".github\\workflows")
            || path_str.contains(".circleci")
            || file.path.file_name().is_some_and(|n| n == ".gitlab-ci.yml");

        if !is_ci {
            return vec![];
        }

        if let FileContent::Yaml(ref value) = file.content {
            let mut results = Vec::new();
            find_python_version_recursive(value, &file.path, &file.raw_text, self.id(), &mut results);
            return results;
        }
        vec![]
    }
}

fn find_python_version_recursive(
    value: &serde_yml::Value,
    path: &std::path::PathBuf,
    raw_text: &str,
    extractor_id: &str,
    results: &mut Vec<ConfigAssertion>,
) {
    match value {
        serde_yml::Value::Mapping(map) => {
            for (key, val) in map {
                let key_str = key.as_str().unwrap_or("");
                if key_str == "python-version" || key_str == "python_version" {
                    let line = find_line_for_key(raw_text, key_str);
                    match val {
                        serde_yml::Value::Sequence(seq) => {
                            for item in seq {
                                if let Some(v) = yaml_value_to_string(item) {
                                    let version = parse_version(&v);
                                    results.push(
                                        ConfigAssertion::new(
                                            SemanticConcept::python_version(),
                                            SemanticType::Version(version),
                                            v,
                                            SourceLocation {
                                                file: path.clone(),
                                                line,
                                                column: 0,
                                                key_path: format!("matrix.{}", key_str),
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
                            if let Some(v) = yaml_value_to_string(val) {
                                let version = parse_version(&v);
                                results.push(ConfigAssertion::new(
                                    SemanticConcept::python_version(),
                                    SemanticType::Version(version),
                                    v,
                                    SourceLocation {
                                        file: path.clone(),
                                        line,
                                        column: 0,
                                        key_path: key_str.into(),
                                    },
                                    Authority::Enforced,
                                    extractor_id,
                                ));
                            }
                        }
                    }
                } else {
                    find_python_version_recursive(val, path, raw_text, extractor_id, results);
                }
            }
        }
        serde_yml::Value::Sequence(seq) => {
            for item in seq {
                find_python_version_recursive(item, path, raw_text, extractor_id, results);
            }
        }
        _ => {}
    }
}

fn yaml_value_to_string(value: &serde_yml::Value) -> Option<String> {
    match value {
        serde_yml::Value::String(s) => Some(s.clone()),
        serde_yml::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Convert Python version specifiers to something our version parser handles.
/// ">=3.10,<3.12" → ">=3.10.0 <3.12.0"
fn parse_python_requires(req: &str) -> VersionSpec {
    // Simple conversion: replace commas with spaces, add .0 to partial versions
    let normalized = req
        .split(',')
        .map(|part| {
            let part = part.trim();
            // Ensure three-part version for semver compatibility
            let re = regex::Regex::new(r"([><=!~]+)(\d+\.\d+)$").unwrap();
            if let Some(caps) = re.captures(part) {
                format!("{}{}.0", &caps[1], &caps[2])
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    parse_version(&normalized)
}
