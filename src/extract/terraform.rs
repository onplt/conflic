use super::Extractor;
use crate::model::*;
use crate::parse::*;
use regex::Regex;
use std::sync::LazyLock;

static TF_RUNTIME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)runtime\s*=\s*"([^"]+)""#).unwrap());

static TF_IMAGE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)image\s*=\s*"([^"]+)""#).unwrap());

static TF_CONTAINER_PORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)container_port\s*=\s*(\d+)"#).unwrap());

static TF_PORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)(?:host_port|port)\s*=\s*(\d+)"#).unwrap());

/// Extracts runtime versions from Terraform (`.tf`) files.
///
/// Recognizes patterns like:
/// - `runtime = "python3.12"` (AWS Lambda)
/// - `runtime = "nodejs20.x"` (AWS Lambda)
/// - `image = "node:20"` (container resources)
/// - `engine_version = "8.0"` (database resources)
pub struct TerraformVersionExtractor;

impl Extractor for TerraformVersionExtractor {
    fn id(&self) -> &str {
        "iac-terraform-version"
    }
    fn description(&self) -> &str {
        "Runtime versions from Terraform .tf files"
    }
    fn concept_ids(&self) -> Vec<String> {
        vec![
            "node-version".into(),
            "python-version".into(),
            "go-version".into(),
            "java-version".into(),
            "ruby-version".into(),
            "dotnet-version".into(),
        ]
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec![".tf"]
    }

    fn matches_file(&self, filename: &str) -> bool {
        filename.ends_with(".tf")
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        // Terraform files are parsed as plain text — always use raw_text for full content
        let text = &file.raw_text;

        let mut results = Vec::new();

        // Extract from runtime = "..." (Lambda, Cloud Functions)
        for cap in TF_RUNTIME_RE.captures_iter(text) {
            let runtime_str = &cap[1];
            let line = find_line_in_text(text, &cap[0]);
            if let Some(assertion) = parse_lambda_runtime(runtime_str, file, line) {
                results.push(assertion);
            }
        }

        // Extract from image = "..." (container resources)
        for cap in TF_IMAGE_RE.captures_iter(text) {
            let image_str = &cap[1];
            let line = find_line_in_text(text, &cap[0]);
            if let Some(assertion) = parse_container_image_tf(image_str, file, line) {
                results.push(assertion);
            }
        }

        results
    }
}

/// Extracts ports from Terraform files.
pub struct TerraformPortExtractor;

impl Extractor for TerraformPortExtractor {
    fn id(&self) -> &str {
        "iac-terraform-port"
    }
    fn description(&self) -> &str {
        "Ports from Terraform .tf files"
    }
    fn concept_ids(&self) -> Vec<String> {
        vec!["app-port".into()]
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec![".tf"]
    }

    fn matches_file(&self, filename: &str) -> bool {
        filename.ends_with(".tf")
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        let text = &file.raw_text;

        let mut results = Vec::new();

        for cap in TF_CONTAINER_PORT_RE.captures_iter(text) {
            if let Ok(port) = cap[1].parse::<u16>() {
                let line = find_line_in_text(text, &cap[0]);
                results.push(ConfigAssertion::new(
                    SemanticConcept::app_port(),
                    SemanticType::Port(PortSpec::Single(port)),
                    port.to_string(),
                    SourceLocation {
                        file: file.path.clone(),
                        line,
                        column: 0,
                        key_path: "container_port".into(),
                    },
                    Authority::Enforced,
                    self.id(),
                ));
            }
        }

        // Also match host_port and port assignments (less common in IaC)
        for cap in TF_PORT_RE.captures_iter(text) {
            if let Ok(port) = cap[1].parse::<u16>() {
                let line = find_line_in_text(text, &cap[0]);
                // Avoid duplicates with container_port
                if !results.iter().any(|a| a.source.line == line) {
                    results.push(ConfigAssertion::new(
                        SemanticConcept::app_port(),
                        SemanticType::Port(PortSpec::Single(port)),
                        port.to_string(),
                        SourceLocation {
                            file: file.path.clone(),
                            line,
                            column: 0,
                            key_path: "port".into(),
                        },
                        Authority::Declared,
                        self.id(),
                    ));
                }
            }
        }

        results
    }
}

/// Lambda/Cloud Functions runtime identifiers and their concept mappings.
struct LambdaRuntime {
    prefix: &'static str,
    concept_fn: fn() -> SemanticConcept,
    version_extractor: fn(&str) -> Option<String>,
}

const LAMBDA_RUNTIMES: &[LambdaRuntime] = &[
    LambdaRuntime {
        prefix: "nodejs",
        concept_fn: SemanticConcept::node_version,
        version_extractor: extract_nodejs_version,
    },
    LambdaRuntime {
        prefix: "python",
        concept_fn: SemanticConcept::python_version,
        version_extractor: extract_python_version,
    },
    LambdaRuntime {
        prefix: "java",
        concept_fn: SemanticConcept::java_version,
        version_extractor: extract_java_version,
    },
    LambdaRuntime {
        prefix: "ruby",
        concept_fn: SemanticConcept::ruby_version,
        version_extractor: extract_ruby_version,
    },
    LambdaRuntime {
        prefix: "go",
        concept_fn: SemanticConcept::go_version,
        version_extractor: extract_go_version,
    },
    LambdaRuntime {
        prefix: "dotnet",
        concept_fn: SemanticConcept::dotnet_version,
        version_extractor: extract_dotnet_version,
    },
];

fn extract_nodejs_version(runtime: &str) -> Option<String> {
    // "nodejs20.x" -> "20", "nodejs18.x" -> "18"
    runtime
        .strip_prefix("nodejs")
        .map(|v| v.trim_end_matches(".x").to_string())
}

fn extract_python_version(runtime: &str) -> Option<String> {
    // "python3.12" -> "3.12"
    runtime.strip_prefix("python").map(String::from)
}

fn extract_java_version(runtime: &str) -> Option<String> {
    // "java21" -> "21"
    runtime.strip_prefix("java").map(String::from)
}

fn extract_ruby_version(runtime: &str) -> Option<String> {
    // "ruby3.3" -> "3.3"
    runtime.strip_prefix("ruby").map(String::from)
}

fn extract_go_version(runtime: &str) -> Option<String> {
    // "go1.x" -> "1", "provided.al2023" is not Go
    let version = runtime.strip_prefix("go")?;
    if version.starts_with(|c: char| c.is_ascii_digit()) {
        Some(version.trim_end_matches(".x").to_string())
    } else {
        None
    }
}

fn extract_dotnet_version(runtime: &str) -> Option<String> {
    // "dotnet8" -> "8", "dotnetcore3.1" -> "3.1"
    let version = runtime
        .strip_prefix("dotnetcore")
        .or_else(|| runtime.strip_prefix("dotnet"))?;
    if version.starts_with(|c: char| c.is_ascii_digit()) {
        Some(version.to_string())
    } else {
        None
    }
}

fn parse_lambda_runtime(runtime: &str, file: &ParsedFile, line: usize) -> Option<ConfigAssertion> {
    for lr in LAMBDA_RUNTIMES {
        if runtime.starts_with(lr.prefix)
            && let Some(version_str) = (lr.version_extractor)(runtime)
        {
            let version = parse_version(&version_str);
            return Some(ConfigAssertion::new(
                (lr.concept_fn)(),
                SemanticType::Version(version),
                version_str,
                SourceLocation {
                    file: file.path.clone(),
                    line,
                    column: 0,
                    key_path: "runtime".into(),
                },
                Authority::Enforced,
                "iac-terraform-version",
            ));
        }
    }
    None
}

/// Known container image names (same as Kubernetes extractor).
#[allow(clippy::type_complexity)]
const TF_IMAGE_MAP: &[(&str, fn() -> SemanticConcept)] = &[
    ("node", SemanticConcept::node_version),
    ("python", SemanticConcept::python_version),
    ("golang", SemanticConcept::go_version),
    ("openjdk", SemanticConcept::java_version),
    ("eclipse-temurin", SemanticConcept::java_version),
    ("ruby", SemanticConcept::ruby_version),
];

fn parse_container_image_tf(
    image: &str,
    file: &ParsedFile,
    line: usize,
) -> Option<ConfigAssertion> {
    let (name, tag) = super::kubernetes::split_image_tag(image)?;

    for (prefix, concept_fn) in TF_IMAGE_MAP {
        if name == *prefix || name.ends_with(&format!("/{}", prefix)) {
            let version = parse_version(tag);
            return Some(ConfigAssertion::new(
                concept_fn(),
                SemanticType::Version(version),
                tag.to_string(),
                SourceLocation {
                    file: file.path.clone(),
                    line,
                    column: 0,
                    key_path: "image".into(),
                },
                Authority::Enforced,
                "iac-terraform-version",
            ));
        }
    }
    None
}

fn find_line_in_text(raw_text: &str, needle: &str) -> usize {
    for (idx, line) in raw_text.lines().enumerate() {
        if line.contains(needle) {
            return idx + 1;
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_tf_file(content: &str) -> ParsedFile {
        crate::parse::parse_file_with_content(
            &PathBuf::from("main.tf"),
            &PathBuf::from("."),
            content.to_string(),
        )
        .unwrap()
    }

    #[test]
    fn test_extract_lambda_nodejs_runtime() {
        let tf = r#"
resource "aws_lambda_function" "handler" {
  function_name = "my-handler"
  runtime       = "nodejs20.x"
  handler       = "index.handler"
}
"#;
        let file = make_tf_file(tf);
        let extractor = TerraformVersionExtractor;
        let assertions = extractor.extract(&file);

        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0].concept.id, "node-version");
        assert_eq!(assertions[0].raw_value, "20");
        assert_eq!(assertions[0].authority, Authority::Enforced);
    }

    #[test]
    fn test_extract_lambda_python_runtime() {
        let tf = r#"
resource "aws_lambda_function" "processor" {
  runtime = "python3.12"
}
"#;
        let file = make_tf_file(tf);
        let extractor = TerraformVersionExtractor;
        let assertions = extractor.extract(&file);

        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0].concept.id, "python-version");
        assert_eq!(assertions[0].raw_value, "3.12");
    }

    #[test]
    fn test_extract_lambda_java_runtime() {
        let tf = r#"
resource "aws_lambda_function" "api" {
  runtime = "java21"
}
"#;
        let file = make_tf_file(tf);
        let extractor = TerraformVersionExtractor;
        let assertions = extractor.extract(&file);

        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0].concept.id, "java-version");
        assert_eq!(assertions[0].raw_value, "21");
    }

    #[test]
    fn test_extract_container_image_from_tf() {
        let tf = r#"
resource "aws_ecs_task_definition" "web" {
  container_definitions = jsonencode([{
    name  = "web"
    image = "node:20-alpine"
  }])
}
"#;
        let file = make_tf_file(tf);
        let extractor = TerraformVersionExtractor;
        let assertions = extractor.extract(&file);

        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0].concept.id, "node-version");
        assert_eq!(assertions[0].raw_value, "20-alpine");
    }

    #[test]
    fn test_extract_port_from_tf() {
        let tf = r#"
resource "aws_ecs_task_definition" "web" {
  container_definitions = jsonencode([{
    name  = "web"
    image = "node:20"
    portMappings = [{
      container_port = 3000
    }]
  }])
}
"#;
        let file = make_tf_file(tf);
        let extractor = TerraformPortExtractor;
        let assertions = extractor.extract(&file);

        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0].raw_value, "3000");
    }

    #[test]
    fn test_extract_dotnet_runtime() {
        let tf = r#"
resource "aws_lambda_function" "api" {
  runtime = "dotnet8"
}
"#;
        let file = make_tf_file(tf);
        let extractor = TerraformVersionExtractor;
        let assertions = extractor.extract(&file);

        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0].concept.id, "dotnet-version");
        assert_eq!(assertions[0].raw_value, "8");
    }

    #[test]
    fn test_unknown_runtime_skipped() {
        let tf = r#"
resource "aws_lambda_function" "custom" {
  runtime = "provided.al2023"
}
"#;
        let file = make_tf_file(tf);
        let extractor = TerraformVersionExtractor;
        let assertions = extractor.extract(&file);
        assert!(assertions.is_empty());
    }

    #[test]
    fn test_multiple_lambdas() {
        let tf = r#"
resource "aws_lambda_function" "a" {
  runtime = "nodejs20.x"
}
resource "aws_lambda_function" "b" {
  runtime = "python3.12"
}
"#;
        let file = make_tf_file(tf);
        let extractor = TerraformVersionExtractor;
        let assertions = extractor.extract(&file);

        assert_eq!(assertions.len(), 2);
        let concepts: Vec<&str> = assertions.iter().map(|a| a.concept.id.as_str()).collect();
        assert!(concepts.contains(&"node-version"));
        assert!(concepts.contains(&"python-version"));
    }
}
