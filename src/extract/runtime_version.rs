use std::borrow::Cow;
use std::path::Path;

use crate::model::*;
use crate::parse::source_location::find_line_for_key_value;
use crate::parse::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeVersionKind {
    Node,
    Python,
    Go,
    Java,
    Ruby,
}

impl RuntimeVersionKind {
    fn concept(self) -> SemanticConcept {
        match self {
            Self::Node => SemanticConcept::node_version(),
            Self::Python => SemanticConcept::python_version(),
            Self::Go => SemanticConcept::go_version(),
            Self::Java => SemanticConcept::java_version(),
            Self::Ruby => SemanticConcept::ruby_version(),
        }
    }
}

pub(crate) trait VersionExtractionStrategy {
    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlainTextNormalizer {
    Identity,
    StripPrefixChar(char),
    StripPrefix(&'static str),
}

impl PlainTextNormalizer {
    fn normalize<'a>(self, raw: &'a str) -> Cow<'a, str> {
        match self {
            Self::Identity => Cow::Borrowed(raw),
            Self::StripPrefixChar(prefix) => raw
                .strip_prefix(prefix)
                .map(Cow::Borrowed)
                .unwrap_or_else(|| Cow::Borrowed(raw)),
            Self::StripPrefix(prefix) => raw
                .strip_prefix(prefix)
                .map(Cow::Borrowed)
                .unwrap_or_else(|| Cow::Borrowed(raw)),
        }
    }
}

pub(crate) struct PlainTextVersionStrategy<'a> {
    pub(crate) runtime: RuntimeVersionKind,
    pub(crate) authority: Authority,
    pub(crate) extractor_id: &'a str,
    pub(crate) normalizer: PlainTextNormalizer,
}

impl VersionExtractionStrategy for PlainTextVersionStrategy<'_> {
    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        let FileContent::PlainText(text) = &file.content else {
            return Vec::new();
        };

        if text.is_empty() {
            return Vec::new();
        }

        let normalized = self.normalizer.normalize(text);
        vec![ConfigAssertion::new(
            self.runtime.concept(),
            SemanticType::Version(parse_version(normalized.as_ref())),
            text.clone(),
            SourceLocation {
                file: file.path.clone(),
                line: 1,
                column: 0,
                key_path: String::new(),
            },
            self.authority,
            self.extractor_id,
        )]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DockerKeyPathStrategy {
    From,
    StageAwareFrom,
}

impl DockerKeyPathStrategy {
    fn key_path(self, instruction: &DockerInstruction) -> String {
        match self {
            Self::From => "FROM".to_string(),
            Self::StageAwareFrom => instruction
                .stage_name
                .as_ref()
                .map(|stage_name| format!("FROM (stage: {})", stage_name))
                .unwrap_or_else(|| "FROM".to_string()),
        }
    }
}

pub(crate) struct DockerRuntimeVersionStrategy<'a> {
    pub(crate) runtime: RuntimeVersionKind,
    pub(crate) extractor_id: &'a str,
    pub(crate) image_names: &'static [&'static str],
    pub(crate) key_path: DockerKeyPathStrategy,
}

impl VersionExtractionStrategy for DockerRuntimeVersionStrategy<'_> {
    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        let FileContent::Dockerfile(instructions) = &file.content else {
            return Vec::new();
        };

        instructions
            .iter()
            .filter(|instruction| instruction.instruction == "FROM")
            .filter_map(|instruction| {
                extract_matching_docker_image_version(&instruction.arguments, self.image_names).map(
                    |version| {
                        ConfigAssertion::new(
                            self.runtime.concept(),
                            SemanticType::Version(parse_version(&version)),
                            instruction.arguments.clone(),
                            SourceLocation {
                                file: file.path.clone(),
                                line: instruction.line,
                                column: 0,
                                key_path: self.key_path.key_path(instruction),
                            },
                            docker_authority(instruction),
                            self.extractor_id,
                        )
                    },
                )
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum YamlScalarStrategy {
    StringOrNumber,
    StringNumberOrBool,
}

pub(crate) struct CiYamlVersionStrategy<'a> {
    pub(crate) runtime: RuntimeVersionKind,
    pub(crate) extractor_id: &'a str,
    pub(crate) keys: &'static [&'static str],
    pub(crate) scalar_strategy: YamlScalarStrategy,
    pub(crate) scan_root_keys: bool,
}

struct CiYamlCollectionContext<'a> {
    path: &'a Path,
    raw_text: &'a str,
    runtime: RuntimeVersionKind,
    extractor_id: &'a str,
    keys: &'a [&'a str],
    scalar_strategy: YamlScalarStrategy,
}

impl CiYamlCollectionContext<'_> {
    fn collect_versions(&self, value: &YamlValue, assertions: &mut Vec<ConfigAssertion>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, nested) in map {
                    if self.keys.iter().any(|candidate| candidate == key) {
                        assertions.extend(self.assertions_for_key(nested, key));
                    } else {
                        self.collect_versions(nested, assertions);
                    }
                }
            }
            serde_json::Value::Array(sequence) => {
                for item in sequence {
                    self.collect_versions(item, assertions);
                }
            }
            _ => {}
        }
    }

    fn assertions_for_key(&self, value: &YamlValue, key: &str) -> Vec<ConfigAssertion> {
        match value {
            serde_json::Value::Array(sequence) => sequence
                .iter()
                .filter_map(|item| yaml_scalar_to_string(item, self.scalar_strategy))
                .map(|raw_value| {
                    let line = find_line_for_key_value(self.raw_text, key, &raw_value);
                    ConfigAssertion::new(
                        self.runtime.concept(),
                        SemanticType::Version(parse_version(&raw_value)),
                        raw_value,
                        SourceLocation {
                            file: self.path.to_path_buf(),
                            line,
                            column: 0,
                            key_path: format!("matrix.{}", key),
                        },
                        Authority::Enforced,
                        self.extractor_id,
                    )
                    .with_matrix(true)
                })
                .collect(),
            _ => yaml_scalar_to_string(value, self.scalar_strategy)
                .map(|raw_value| {
                    let line = find_line_for_key_value(self.raw_text, key, &raw_value);
                    vec![ConfigAssertion::new(
                        self.runtime.concept(),
                        SemanticType::Version(parse_version(&raw_value)),
                        raw_value,
                        SourceLocation {
                            file: self.path.to_path_buf(),
                            line,
                            column: 0,
                            key_path: key.to_string(),
                        },
                        Authority::Enforced,
                        self.extractor_id,
                    )]
                })
                .unwrap_or_default(),
        }
    }
}

impl VersionExtractionStrategy for CiYamlVersionStrategy<'_> {
    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if !is_ci_config_path(&file.path) {
            return Vec::new();
        }

        let FileContent::Yaml(value) = &file.content else {
            return Vec::new();
        };

        let context = CiYamlCollectionContext {
            path: &file.path,
            raw_text: &file.raw_text,
            runtime: self.runtime,
            extractor_id: self.extractor_id,
            keys: self.keys,
            scalar_strategy: self.scalar_strategy,
        };

        let mut assertions = Vec::new();
        if self.scan_root_keys {
            context.collect_versions(value, &mut assertions);
        } else if let Some(mapping) = value.as_object() {
            for nested in mapping.values() {
                context.collect_versions(nested, &mut assertions);
            }
        }

        assertions
    }
}

pub(crate) struct ToolVersionsVersionStrategy<'a> {
    pub(crate) runtime: RuntimeVersionKind,
    pub(crate) extractor_id: &'a str,
    pub(crate) tool_names: &'static [&'static str],
    pub(crate) authority: Authority,
    pub(crate) key_path: &'a str,
}

impl VersionExtractionStrategy for ToolVersionsVersionStrategy<'_> {
    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        let FileContent::PlainText(_) = &file.content else {
            return Vec::new();
        };

        let Some((line, version)) =
            file.raw_text
                .lines()
                .enumerate()
                .find_map(|(index, raw_line)| {
                    line_value_for_tool(raw_line, self.tool_names)
                        .map(|value| (index + 1, value.to_string()))
                })
        else {
            return Vec::new();
        };

        vec![ConfigAssertion::new(
            self.runtime.concept(),
            SemanticType::Version(parse_version(&version)),
            version,
            SourceLocation {
                file: file.path.clone(),
                line,
                column: 0,
                key_path: self.key_path.to_string(),
            },
            self.authority,
            self.extractor_id,
        )]
    }
}

pub(crate) fn extract_docker_image_version(from_args: &str, image_name: &str) -> Option<String> {
    let reference = crate::parse::dockerfile::docker_from_image_reference(from_args)?;
    let image_basename = reference.image.rsplit('/').next()?;

    (image_basename == image_name).then(|| reference.tag.to_string())
}

fn yaml_scalar_to_string(value: &YamlValue, strategy: YamlScalarStrategy) -> Option<String> {
    match value {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Bool(value)
            if matches!(strategy, YamlScalarStrategy::StringNumberOrBool) =>
        {
            Some(value.to_string())
        }
        _ => None,
    }
}

fn is_ci_config_path(path: &Path) -> bool {
    if path
        .file_name()
        .is_some_and(|name| name == ".gitlab-ci.yml")
    {
        return true;
    }

    let mut saw_dot_github = false;
    for component in path.components() {
        let Some(component) = component.as_os_str().to_str() else {
            saw_dot_github = false;
            continue;
        };

        if saw_dot_github && component == "workflows" {
            return true;
        }

        if component == ".circleci" {
            return true;
        }

        saw_dot_github = component == ".github";
    }

    false
}

fn line_value_for_tool<'a>(line: &'a str, tool_names: &[&str]) -> Option<&'a str> {
    let mut parts = line.split_whitespace();
    let tool = parts.next()?;
    let value = parts.next()?;
    tool_names.contains(&tool).then_some(value)
}

fn docker_authority(instruction: &DockerInstruction) -> Authority {
    if instruction.is_final_stage {
        Authority::Enforced
    } else {
        Authority::Advisory
    }
}

fn extract_matching_docker_image_version(from_args: &str, image_names: &[&str]) -> Option<String> {
    image_names
        .iter()
        .find_map(|image_name| extract_docker_image_version(from_args, image_name))
}
