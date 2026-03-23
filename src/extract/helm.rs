use super::Extractor;
use crate::model::*;
use crate::parse::*;

/// Extracts runtime versions from Helm `values.yaml` files.
///
/// Looks for common Helm chart image patterns:
/// - `image.tag: "20-alpine"`
/// - `image.repository: "node"` + `image.tag: "20"`
/// - Nested service images: `service.image.tag`
pub struct HelmValuesVersionExtractor;

impl Extractor for HelmValuesVersionExtractor {
    fn id(&self) -> &str {
        "iac-helm-version"
    }
    fn description(&self) -> &str {
        "Runtime versions from Helm values.yaml"
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
        vec!["values.yaml", "values.yml"]
    }

    fn matches_file(&self, filename: &str) -> bool {
        let lower = filename.to_lowercase();
        lower == "values.yaml"
            || lower == "values.yml"
            || lower.starts_with("values-")
            || lower.starts_with("values.")
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        let FileContent::Yaml(ref value) = file.content else {
            return vec![];
        };

        let mut results = Vec::new();
        extract_helm_images(value, file, &mut results, "");
        results
    }
}

/// Extracts ports from Helm `values.yaml` files.
pub struct HelmValuesPortExtractor;

impl Extractor for HelmValuesPortExtractor {
    fn id(&self) -> &str {
        "iac-helm-port"
    }
    fn description(&self) -> &str {
        "Ports from Helm values.yaml"
    }
    fn concept_ids(&self) -> Vec<String> {
        vec!["app-port".into()]
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec!["values.yaml", "values.yml"]
    }

    fn matches_file(&self, filename: &str) -> bool {
        let lower = filename.to_lowercase();
        lower == "values.yaml"
            || lower == "values.yml"
            || lower.starts_with("values-")
            || lower.starts_with("values.")
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        let FileContent::Yaml(ref value) = file.content else {
            return vec![];
        };

        let mut results = Vec::new();
        extract_helm_ports(value, file, &mut results, "");
        results
    }
}

/// Recursively search for image definitions in Helm values.
fn extract_helm_images(
    value: &serde_json::Value,
    file: &ParsedFile,
    results: &mut Vec<ConfigAssertion>,
    path_prefix: &str,
) {
    let Some(obj) = value.as_object() else {
        return;
    };

    // Check for image.repository + image.tag pattern
    if let Some(image_obj) = obj.get("image").and_then(|v| v.as_object()) {
        let repository = image_obj
            .get("repository")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let tag = image_obj.get("tag").and_then(|v| v.as_str());

        if let Some(tag) = tag {
            let key_path = if path_prefix.is_empty() {
                "image.tag".to_string()
            } else {
                format!("{}.image.tag", path_prefix)
            };

            if let Some(assertion) = make_helm_image_assertion(repository, tag, file, &key_path) {
                results.push(assertion);
            }
        }
    }

    // Recurse into nested objects (for multi-service charts)
    for (key, child) in obj {
        if key == "image" {
            continue; // Already handled
        }
        if child.is_object() {
            let next_prefix = if path_prefix.is_empty() {
                key.clone()
            } else {
                format!("{}.{}", path_prefix, key)
            };
            extract_helm_images(child, file, results, &next_prefix);
        }
    }
}

/// Recursively search for port definitions in Helm values.
fn extract_helm_ports(
    value: &serde_json::Value,
    file: &ParsedFile,
    results: &mut Vec<ConfigAssertion>,
    path_prefix: &str,
) {
    let Some(obj) = value.as_object() else {
        return;
    };

    // Check for service.port, containerPort, port
    for key in &["port", "containerPort", "targetPort", "servicePort"] {
        if let Some(port_val) = obj.get(*key) {
            let port_num = port_val
                .as_u64()
                .or_else(|| port_val.as_str().and_then(|s| s.parse::<u64>().ok()));
            if let Some(p) = port_num
                && let Ok(port) = u16::try_from(p)
            {
                let key_path = if path_prefix.is_empty() {
                    key.to_string()
                } else {
                    format!("{}.{}", path_prefix, key)
                };
                let line = find_line_in_text(&file.raw_text, &p.to_string());
                results.push(ConfigAssertion::new(
                    SemanticConcept::app_port(),
                    SemanticType::Port(PortSpec::Single(port)),
                    port.to_string(),
                    SourceLocation {
                        file: file.path.clone(),
                        line,
                        column: 0,
                        key_path,
                    },
                    Authority::Declared,
                    "iac-helm-port",
                ));
            }
        }
    }

    // Recurse into nested objects
    for (key, child) in obj {
        if child.is_object() {
            let next_prefix = if path_prefix.is_empty() {
                key.clone()
            } else {
                format!("{}.{}", path_prefix, key)
            };
            extract_helm_ports(child, file, results, &next_prefix);
        }
    }
}

/// Known runtime image names for Helm charts.
#[allow(clippy::type_complexity)]
const HELM_IMAGE_MAP: &[(&str, fn() -> SemanticConcept)] = &[
    ("node", SemanticConcept::node_version),
    ("python", SemanticConcept::python_version),
    ("golang", SemanticConcept::go_version),
    ("openjdk", SemanticConcept::java_version),
    ("eclipse-temurin", SemanticConcept::java_version),
    ("amazoncorretto", SemanticConcept::java_version),
    ("ruby", SemanticConcept::ruby_version),
    (
        "mcr.microsoft.com/dotnet/sdk",
        SemanticConcept::dotnet_version,
    ),
    (
        "mcr.microsoft.com/dotnet/aspnet",
        SemanticConcept::dotnet_version,
    ),
    (
        "mcr.microsoft.com/dotnet/runtime",
        SemanticConcept::dotnet_version,
    ),
];

fn make_helm_image_assertion(
    repository: &str,
    tag: &str,
    file: &ParsedFile,
    key_path: &str,
) -> Option<ConfigAssertion> {
    for (name, concept_fn) in HELM_IMAGE_MAP {
        if repository == *name || repository.ends_with(&format!("/{}", name)) {
            let version = parse_version(tag);
            let line = find_line_in_text(&file.raw_text, tag);
            return Some(ConfigAssertion::new(
                concept_fn(),
                SemanticType::Version(version),
                tag.to_string(),
                SourceLocation {
                    file: file.path.clone(),
                    line,
                    column: 0,
                    key_path: key_path.to_string(),
                },
                Authority::Declared,
                "iac-helm-version",
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

    fn make_values_file(yaml_str: &str) -> ParsedFile {
        crate::parse::parse_file_with_content(
            &PathBuf::from("values.yaml"),
            &PathBuf::from("."),
            yaml_str.to_string(),
        )
        .unwrap()
    }

    #[test]
    fn test_extract_node_version_from_helm_values() {
        let yaml = r#"
image:
  repository: node
  tag: "20-alpine"
"#;
        let file = make_values_file(yaml);
        let extractor = HelmValuesVersionExtractor;
        let assertions = extractor.extract(&file);

        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0].concept.id, "node-version");
        assert_eq!(assertions[0].raw_value, "20-alpine");
    }

    #[test]
    fn test_extract_nested_service_image() {
        let yaml = r#"
api:
  image:
    repository: python
    tag: "3.12"
worker:
  image:
    repository: node
    tag: "20"
"#;
        let file = make_values_file(yaml);
        let extractor = HelmValuesVersionExtractor;
        let assertions = extractor.extract(&file);

        assert_eq!(assertions.len(), 2);
        let concepts: Vec<&str> = assertions.iter().map(|a| a.concept.id.as_str()).collect();
        assert!(concepts.contains(&"python-version"));
        assert!(concepts.contains(&"node-version"));
    }

    #[test]
    fn test_extract_port_from_helm_values() {
        let yaml = r#"
service:
  port: 8080
  targetPort: 3000
"#;
        let file = make_values_file(yaml);
        let extractor = HelmValuesPortExtractor;
        let assertions = extractor.extract(&file);

        assert_eq!(assertions.len(), 2);
        let ports: Vec<&str> = assertions.iter().map(|a| a.raw_value.as_str()).collect();
        assert!(ports.contains(&"8080"));
        assert!(ports.contains(&"3000"));
    }

    #[test]
    fn test_unknown_image_not_extracted() {
        let yaml = r#"
image:
  repository: mycompany/custom-app
  tag: "v2.3.1"
"#;
        let file = make_values_file(yaml);
        let extractor = HelmValuesVersionExtractor;
        let assertions = extractor.extract(&file);
        assert!(assertions.is_empty());
    }
}
