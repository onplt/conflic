use crate::model::*;
use crate::parse::*;
use crate::parse::source_location::*;
use super::Extractor;
use super::node_version::extract_docker_image_version;

// --- .ruby-version extractor ---

pub struct RubyVersionFileExtractor;

impl Extractor for RubyVersionFileExtractor {
    fn id(&self) -> &str { "ruby-version-file" }
    fn description(&self) -> &str { "Ruby version from .ruby-version" }
    fn relevant_filenames(&self) -> Vec<&str> { vec![".ruby-version"] }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::PlainText(ref text) = file.content {
            if text.is_empty() {
                return vec![];
            }
            // Strip leading 'ruby-' prefix if present (e.g. "ruby-3.2.2")
            let version_str = text.strip_prefix("ruby-").unwrap_or(text);
            let version = parse_version(version_str);
            vec![ConfigAssertion::new(
                SemanticConcept::ruby_version(),
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

// --- Gemfile ruby version extractor ---

pub struct GemfileExtractor;

impl Extractor for GemfileExtractor {
    fn id(&self) -> &str { "ruby-version-gemfile" }
    fn description(&self) -> &str { "Ruby version from Gemfile" }
    fn relevant_filenames(&self) -> Vec<&str> { vec!["Gemfile"] }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::PlainText(ref _text) = file.content {
            // Parse `ruby "3.2.2"` or `ruby '3.2.2'` from Gemfile
            let re = regex::Regex::new(r#"^\s*ruby\s+['"]([^'"]+)['"]"#).unwrap();
            for (line_num, line) in file.raw_text.lines().enumerate() {
                if let Some(caps) = re.captures(line) {
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
    fn id(&self) -> &str { "ruby-version-dockerfile" }
    fn description(&self) -> &str { "Ruby version from Dockerfile FROM ruby:*" }
    fn relevant_filenames(&self) -> Vec<&str> { vec!["Dockerfile"] }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::Dockerfile(ref instructions) = file.content {
            let mut results = Vec::new();
            for instr in instructions {
                if instr.instruction == "FROM" {
                    if let Some(version) = extract_docker_image_version(&instr.arguments, "ruby") {
                        let authority = if instr.is_final_stage {
                            Authority::Enforced
                        } else {
                            Authority::Advisory
                        };
                        results.push(ConfigAssertion::new(
                            SemanticConcept::ruby_version(),
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

// --- .tool-versions ruby extractor ---

pub struct ToolVersionsRubyExtractor;

impl Extractor for ToolVersionsRubyExtractor {
    fn id(&self) -> &str { "ruby-version-tool-versions" }
    fn description(&self) -> &str { "Ruby version from .tool-versions" }
    fn relevant_filenames(&self) -> Vec<&str> { vec![".tool-versions"] }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::PlainText(_) = file.content {
            for (line_num, line) in file.raw_text.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.starts_with("ruby ") {
                    let version_str = trimmed
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("");
                    if !version_str.is_empty() {
                        let version = parse_version(version_str);
                        return vec![ConfigAssertion::new(
                            SemanticConcept::ruby_version(),
                            SemanticType::Version(version),
                            version_str.to_string(),
                            SourceLocation {
                                file: file.path.clone(),
                                line: line_num + 1,
                                column: 0,
                                key_path: "ruby".into(),
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

// --- CI yaml ruby-version ---

pub struct CiRubyExtractor;

impl Extractor for CiRubyExtractor {
    fn id(&self) -> &str { "ruby-version-ci" }
    fn description(&self) -> &str { "Ruby version from CI config" }
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
            find_ruby_version_recursive(value, &file.path, &file.raw_text, self.id(), &mut results);
            return results;
        }
        vec![]
    }
}

fn find_ruby_version_recursive(
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
                if key_str == "ruby-version" || key_str == "ruby_version" {
                    let line = find_line_for_key(raw_text, key_str);
                    match val {
                        serde_yml::Value::Sequence(seq) => {
                            for item in seq {
                                if let Some(v) = yaml_value_to_string(item) {
                                    let version = parse_version(&v);
                                    results.push(
                                        ConfigAssertion::new(
                                            SemanticConcept::ruby_version(),
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
                                    SemanticConcept::ruby_version(),
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
                    find_ruby_version_recursive(val, path, raw_text, extractor_id, results);
                }
            }
        }
        serde_yml::Value::Sequence(seq) => {
            for item in seq {
                find_ruby_version_recursive(item, path, raw_text, extractor_id, results);
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
