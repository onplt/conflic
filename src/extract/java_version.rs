use crate::model::*;
use crate::parse::*;
use crate::parse::source_location::*;
use super::Extractor;
use super::node_version::extract_docker_image_version;

// --- pom.xml extractor (regex-based, no XML dep) ---

pub struct PomXmlExtractor;

impl Extractor for PomXmlExtractor {
    fn id(&self) -> &str { "java-version-pom" }
    fn description(&self) -> &str { "Java version from pom.xml maven.compiler.source/target" }
    fn relevant_filenames(&self) -> Vec<&str> { vec!["pom.xml"] }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        // pom.xml is parsed as PlainText since we don't have an XML parser
        let raw = &file.raw_text;
        if raw.is_empty() {
            return vec![];
        }

        let mut results = Vec::new();

        // Look for <maven.compiler.source>17</maven.compiler.source>
        let source_re = regex::Regex::new(
            r"<maven\.compiler\.source>\s*(\d+[\d.]*)\s*</maven\.compiler\.source>"
        ).unwrap();
        if let Some(caps) = source_re.captures(raw) {
            let version_str = caps[1].to_string();
            let line = find_line_for_key(raw, "maven.compiler.source");
            results.push(ConfigAssertion::new(
                SemanticConcept::java_version(),
                SemanticType::Version(parse_version(&version_str)),
                version_str,
                SourceLocation {
                    file: file.path.clone(),
                    line,
                    column: 0,
                    key_path: "maven.compiler.source".into(),
                },
                Authority::Declared,
                self.id(),
            ));
        }

        // Look for <maven.compiler.target>17</maven.compiler.target>
        let target_re = regex::Regex::new(
            r"<maven\.compiler\.target>\s*(\d+[\d.]*)\s*</maven\.compiler\.target>"
        ).unwrap();
        if let Some(caps) = target_re.captures(raw) {
            let version_str = caps[1].to_string();
            let line = find_line_for_key(raw, "maven.compiler.target");
            results.push(ConfigAssertion::new(
                SemanticConcept::java_version(),
                SemanticType::Version(parse_version(&version_str)),
                version_str,
                SourceLocation {
                    file: file.path.clone(),
                    line,
                    column: 0,
                    key_path: "maven.compiler.target".into(),
                },
                Authority::Declared,
                self.id(),
            ));
        }

        // Also check <java.version>17</java.version> (Spring Boot convention)
        let java_ver_re = regex::Regex::new(
            r"<java\.version>\s*(\d+[\d.]*)\s*</java\.version>"
        ).unwrap();
        if let Some(caps) = java_ver_re.captures(raw) {
            let version_str = caps[1].to_string();
            let line = find_line_for_key(raw, "java.version");
            results.push(ConfigAssertion::new(
                SemanticConcept::java_version(),
                SemanticType::Version(parse_version(&version_str)),
                version_str,
                SourceLocation {
                    file: file.path.clone(),
                    line,
                    column: 0,
                    key_path: "java.version".into(),
                },
                Authority::Declared,
                self.id(),
            ));
        }

        // Check <release>17</release> (maven-compiler-plugin config)
        let release_re = regex::Regex::new(
            r"<release>\s*(\d+[\d.]*)\s*</release>"
        ).unwrap();
        if let Some(caps) = release_re.captures(raw) {
            let version_str = caps[1].to_string();
            let line = find_line_for_key(raw, "<release>");
            results.push(ConfigAssertion::new(
                SemanticConcept::java_version(),
                SemanticType::Version(parse_version(&version_str)),
                version_str,
                SourceLocation {
                    file: file.path.clone(),
                    line,
                    column: 0,
                    key_path: "release".into(),
                },
                Authority::Declared,
                self.id(),
            ));
        }

        results
    }
}

// --- Dockerfile FROM openjdk:* / eclipse-temurin:* ---

pub struct DockerfileJavaExtractor;

impl Extractor for DockerfileJavaExtractor {
    fn id(&self) -> &str { "java-version-dockerfile" }
    fn description(&self) -> &str { "Java version from Dockerfile FROM openjdk/temurin:*" }
    fn relevant_filenames(&self) -> Vec<&str> { vec!["Dockerfile"] }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::Dockerfile(ref instructions) = file.content {
            let mut results = Vec::new();
            let java_images = ["openjdk", "eclipse-temurin", "amazoncorretto", "ibm-semeru-runtimes"];

            for instr in instructions {
                if instr.instruction == "FROM" {
                    for image_name in &java_images {
                        if let Some(version) = extract_docker_image_version(&instr.arguments, image_name) {
                            let authority = if instr.is_final_stage {
                                Authority::Enforced
                            } else {
                                Authority::Advisory
                            };
                            results.push(ConfigAssertion::new(
                                SemanticConcept::java_version(),
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
                            break; // Only match one image per FROM
                        }
                    }
                }
            }
            return results;
        }
        vec![]
    }
}

// --- .sdkmanrc extractor ---

pub struct SdkmanrcExtractor;

impl Extractor for SdkmanrcExtractor {
    fn id(&self) -> &str { "java-version-sdkmanrc" }
    fn description(&self) -> &str { "Java version from .sdkmanrc" }
    fn relevant_filenames(&self) -> Vec<&str> { vec![".sdkmanrc"] }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::PlainText(_) = file.content {
            // .sdkmanrc format: java=17.0.9-tem
            let re = regex::Regex::new(r"^\s*java\s*=\s*(.+)").unwrap();
            for (line_num, line) in file.raw_text.lines().enumerate() {
                if let Some(caps) = re.captures(line) {
                    let raw_version = caps[1].trim().to_string();
                    // Extract the major version number from strings like "17.0.9-tem"
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
    fn id(&self) -> &str { "java-version-tool-versions" }
    fn description(&self) -> &str { "Java version from .tool-versions" }
    fn relevant_filenames(&self) -> Vec<&str> { vec![".tool-versions"] }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::PlainText(_) = file.content {
            for (line_num, line) in file.raw_text.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.starts_with("java ") {
                    let version_str = trimmed
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("");
                    if !version_str.is_empty() {
                        let version = parse_version(version_str);
                        return vec![ConfigAssertion::new(
                            SemanticConcept::java_version(),
                            SemanticType::Version(version),
                            version_str.to_string(),
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
        }
        vec![]
    }
}

// --- CI yaml java-version ---

pub struct CiJavaExtractor;

impl Extractor for CiJavaExtractor {
    fn id(&self) -> &str { "java-version-ci" }
    fn description(&self) -> &str { "Java version from CI config" }
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
            find_java_version_recursive(value, &file.path, &file.raw_text, self.id(), &mut results);
            return results;
        }
        vec![]
    }
}

fn find_java_version_recursive(
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
                if key_str == "java-version" || key_str == "java_version" {
                    let line = find_line_for_key(raw_text, key_str);
                    match val {
                        serde_yml::Value::Sequence(seq) => {
                            for item in seq {
                                if let Some(v) = yaml_value_to_string(item) {
                                    let version = parse_version(&v);
                                    results.push(
                                        ConfigAssertion::new(
                                            SemanticConcept::java_version(),
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
                                    SemanticConcept::java_version(),
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
                    find_java_version_recursive(val, path, raw_text, extractor_id, results);
                }
            }
        }
        serde_yml::Value::Sequence(seq) => {
            for item in seq {
                find_java_version_recursive(item, path, raw_text, extractor_id, results);
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
