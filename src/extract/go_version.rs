use crate::model::*;
use crate::parse::*;
use super::Extractor;
use super::node_version::extract_docker_image_version;

// --- go.mod extractor ---

pub struct GoModExtractor;

impl Extractor for GoModExtractor {
    fn id(&self) -> &str { "go-version-gomod" }
    fn description(&self) -> &str { "Go version from go.mod" }
    fn relevant_filenames(&self) -> Vec<&str> { vec!["go.mod"] }

    fn matches_file(&self, filename: &str) -> bool {
        filename == "go.mod"
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        // go.mod is a plain text format, not TOML/JSON
        // We read the raw text regardless of how it was parsed
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
    fn id(&self) -> &str { "go-version-dockerfile" }
    fn description(&self) -> &str { "Go version from Dockerfile FROM golang:*" }
    fn relevant_filenames(&self) -> Vec<&str> { vec!["Dockerfile"] }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::Dockerfile(ref instructions) = file.content {
            let mut results = Vec::new();
            for instr in instructions {
                if instr.instruction == "FROM" {
                    if let Some(version) = extract_docker_image_version(&instr.arguments, "golang") {
                        let authority = if instr.is_final_stage {
                            Authority::Enforced
                        } else {
                            Authority::Advisory
                        };
                        results.push(ConfigAssertion::new(
                            SemanticConcept::go_version(),
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
