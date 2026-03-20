use super::Extractor;
use super::runtime_version::{
    CiYamlVersionStrategy, DockerKeyPathStrategy, DockerRuntimeVersionStrategy, RuntimeVersionKind,
    ToolVersionsVersionStrategy, VersionExtractionStrategy, YamlScalarStrategy,
};
use crate::model::*;
use crate::parse::source_location::*;
use crate::parse::*;
use regex::Regex;
use std::sync::LazyLock;

static SDKMANRC_JAVA_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*java\s*=\s*(.+)").unwrap());

// --- pom.xml extractor (regex-based, no XML dep) ---

pub struct PomXmlExtractor;

impl Extractor for PomXmlExtractor {
    fn id(&self) -> &str {
        "java-version-pom"
    }
    fn description(&self) -> &str {
        "Java version from pom.xml maven.compiler.source/target"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec!["pom.xml"]
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        let raw = &file.raw_text;
        if raw.is_empty() {
            return vec![];
        }

        let mut results = Vec::new();
        results.extend(xml_java_assertions(
            file,
            raw,
            "maven.compiler.source",
            self.id(),
        ));
        results.extend(xml_java_assertions(
            file,
            raw,
            "maven.compiler.target",
            self.id(),
        ));
        results.extend(xml_java_assertions(file, raw, "java.version", self.id()));
        results.extend(xml_java_assertions(file, raw, "release", self.id()));

        results
    }
}

// --- Dockerfile FROM openjdk:* / eclipse-temurin:* ---

pub struct DockerfileJavaExtractor;

impl Extractor for DockerfileJavaExtractor {
    fn id(&self) -> &str {
        "java-version-dockerfile"
    }
    fn description(&self) -> &str {
        "Java version from Dockerfile FROM openjdk/temurin:*"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec!["Dockerfile"]
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        DockerRuntimeVersionStrategy {
            runtime: RuntimeVersionKind::Java,
            extractor_id: self.id(),
            image_names: &[
                "openjdk",
                "eclipse-temurin",
                "amazoncorretto",
                "ibm-semeru-runtimes",
            ],
            key_path: DockerKeyPathStrategy::From,
        }
        .extract(file)
    }
}

// --- .sdkmanrc extractor ---

pub struct SdkmanrcExtractor;

impl Extractor for SdkmanrcExtractor {
    fn id(&self) -> &str {
        "java-version-sdkmanrc"
    }
    fn description(&self) -> &str {
        "Java version from .sdkmanrc"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec![".sdkmanrc"]
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::PlainText(_) = file.content {
            for (line_num, line) in file.raw_text.lines().enumerate() {
                if let Some(caps) = SDKMANRC_JAVA_RE.captures(line) {
                    let raw_version = caps[1].trim().to_string();
                    let version = parse_version(&raw_version);
                    return vec![ConfigAssertion::new(
                        SemanticConcept::java_version(),
                        SemanticType::Version(version),
                        raw_version,
                        SourceLocation {
                            file: file.path.clone(),
                            line: line_num + 1,
                            column: 0,
                            key_path: "java".into(),
                        },
                        Authority::Advisory,
                        self.id(),
                    )];
                }
            }
        }
        vec![]
    }
}

// --- .tool-versions java extractor ---

pub struct ToolVersionsJavaExtractor;

impl Extractor for ToolVersionsJavaExtractor {
    fn id(&self) -> &str {
        "java-version-tool-versions"
    }
    fn description(&self) -> &str {
        "Java version from .tool-versions"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec![".tool-versions"]
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        ToolVersionsVersionStrategy {
            runtime: RuntimeVersionKind::Java,
            extractor_id: self.id(),
            tool_names: &["java"],
            authority: Authority::Advisory,
            key_path: "java",
        }
        .extract(file)
    }
}

// --- CI yaml java-version ---

pub struct CiJavaExtractor;

impl Extractor for CiJavaExtractor {
    fn id(&self) -> &str {
        "java-version-ci"
    }
    fn description(&self) -> &str {
        "Java version from CI config"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec![]
    }

    fn matches_file(&self, filename: &str) -> bool {
        (filename.ends_with(".yml") || filename.ends_with(".yaml")) || filename == ".gitlab-ci.yml"
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        CiYamlVersionStrategy {
            runtime: RuntimeVersionKind::Java,
            extractor_id: self.id(),
            keys: &["java-version", "java_version"],
            scalar_strategy: YamlScalarStrategy::StringOrNumber,
            scan_root_keys: true,
        }
        .extract(file)
    }
}

fn xml_java_assertions(
    file: &ParsedFile,
    raw: &str,
    tag: &str,
    extractor_id: &str,
) -> Vec<ConfigAssertion> {
    crate::parse::xml::find_tag_value_matches(raw, tag)
        .unwrap_or_default()
        .into_iter()
        .map(|xml_match| {
            let location = location_from_span(&file.path, tag, xml_match.span);
            ConfigAssertion::new(
                SemanticConcept::java_version(),
                SemanticType::Version(parse_version(&xml_match.raw_value)),
                xml_match.raw_value,
                location,
                Authority::Declared,
                extractor_id,
            )
            .with_span(xml_match.span)
        })
        .collect()
}
