use super::Extractor;
use crate::model::*;
use crate::parse::*;
use std::path::Path;

/// Extracts runtime versions from Kubernetes manifests (Deployment, StatefulSet, Job, CronJob, Pod).
///
/// Recognizes container `image:` fields like `node:20-alpine`, `python:3.12`, `golang:1.22`.
pub struct KubernetesVersionExtractor;

impl Extractor for KubernetesVersionExtractor {
    fn id(&self) -> &str {
        "iac-kubernetes-version"
    }
    fn description(&self) -> &str {
        "Runtime versions from Kubernetes manifests"
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
        vec!["deployment", "statefulset", "job", "cronjob", "pod"]
    }

    fn matches_file(&self, filename: &str) -> bool {
        let lower = filename.to_lowercase();
        is_k8s_manifest_name(&lower)
    }

    fn matches_path(&self, _filename: &str, path: &Path) -> bool {
        let path_str = path.to_string_lossy().to_lowercase();
        // Match common k8s manifest patterns
        if path_str.contains("k8s")
            || path_str.contains("kubernetes")
            || path_str.contains("manifests")
        {
            let lower = _filename.to_lowercase();
            return lower.ends_with(".yml") || lower.ends_with(".yaml");
        }
        let lower = _filename.to_lowercase();
        is_k8s_manifest_name(&lower)
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        let FileContent::Yaml(ref value) = file.content else {
            return vec![];
        };

        let mut results = Vec::new();
        extract_from_k8s_yaml(value, file, &mut results);
        results
    }
}

/// Extracts ports from Kubernetes Service and Deployment manifests.
pub struct KubernetesPortExtractor;

impl Extractor for KubernetesPortExtractor {
    fn id(&self) -> &str {
        "iac-kubernetes-port"
    }
    fn description(&self) -> &str {
        "Ports from Kubernetes manifests"
    }
    fn concept_ids(&self) -> Vec<String> {
        vec!["app-port".into()]
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec!["service", "deployment"]
    }

    fn matches_file(&self, filename: &str) -> bool {
        let lower = filename.to_lowercase();
        is_k8s_manifest_name(&lower)
    }

    fn matches_path(&self, _filename: &str, path: &Path) -> bool {
        let path_str = path.to_string_lossy().to_lowercase();
        if path_str.contains("k8s")
            || path_str.contains("kubernetes")
            || path_str.contains("manifests")
        {
            let lower = _filename.to_lowercase();
            return lower.ends_with(".yml") || lower.ends_with(".yaml");
        }
        let lower = _filename.to_lowercase();
        is_k8s_manifest_name(&lower)
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        let FileContent::Yaml(ref value) = file.content else {
            return vec![];
        };

        let mut results = Vec::new();
        extract_ports_from_k8s_yaml(value, file, &mut results);
        results
    }
}

fn is_k8s_manifest_name(lower: &str) -> bool {
    let k8s_names = [
        "deployment.yml",
        "deployment.yaml",
        "statefulset.yml",
        "statefulset.yaml",
        "service.yml",
        "service.yaml",
        "job.yml",
        "job.yaml",
        "cronjob.yml",
        "cronjob.yaml",
        "pod.yml",
        "pod.yaml",
    ];
    k8s_names.iter().any(|n| lower.ends_with(n))
}

fn extract_from_k8s_yaml(
    value: &serde_json::Value,
    file: &ParsedFile,
    results: &mut Vec<ConfigAssertion>,
) {
    // Check if this is a Kubernetes resource
    let kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let workload_kinds = [
        "Deployment",
        "StatefulSet",
        "Job",
        "CronJob",
        "Pod",
        "DaemonSet",
        "ReplicaSet",
    ];

    if !workload_kinds.contains(&kind) {
        return;
    }

    // Navigate to containers
    let containers = get_containers(value);
    for container in containers {
        if let Some(image) = container.get("image").and_then(|v| v.as_str())
            && let Some(assertion) = parse_container_image(image, file, kind)
        {
            results.push(assertion);
        }
    }
}

fn extract_ports_from_k8s_yaml(
    value: &serde_json::Value,
    file: &ParsedFile,
    results: &mut Vec<ConfigAssertion>,
) {
    let kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");

    // Extract containerPort from workloads
    let workload_kinds = [
        "Deployment",
        "StatefulSet",
        "Job",
        "CronJob",
        "Pod",
        "DaemonSet",
        "ReplicaSet",
    ];
    if workload_kinds.contains(&kind) {
        let containers = get_containers(value);
        for container in containers {
            if let Some(ports) = container.get("ports").and_then(|v| v.as_array()) {
                for port_obj in ports {
                    if let Some(cp) = port_obj.get("containerPort").and_then(|v| v.as_u64())
                        && let Ok(port) = u16::try_from(cp)
                    {
                        let line = find_line_in_text(&file.raw_text, &cp.to_string());
                        results.push(ConfigAssertion::new(
                            SemanticConcept::app_port(),
                            SemanticType::Port(PortSpec::Single(port)),
                            port.to_string(),
                            SourceLocation {
                                file: file.path.clone(),
                                line,
                                column: 0,
                                key_path: "spec.containers.ports.containerPort".into(),
                            },
                            Authority::Enforced,
                            "iac-kubernetes-port",
                        ));
                    }
                }
            }
        }
    }

    // Extract port/targetPort from Service
    if kind == "Service"
        && let Some(ports) = value
            .get("spec")
            .and_then(|v| v.get("ports"))
            .and_then(|v| v.as_array())
    {
        for port_obj in ports {
            if let Some(tp) = port_obj.get("targetPort").and_then(|v| v.as_u64())
                && let Ok(port) = u16::try_from(tp)
            {
                let line = find_line_in_text(&file.raw_text, &tp.to_string());
                results.push(ConfigAssertion::new(
                    SemanticConcept::app_port(),
                    SemanticType::Port(PortSpec::Single(port)),
                    port.to_string(),
                    SourceLocation {
                        file: file.path.clone(),
                        line,
                        column: 0,
                        key_path: "spec.ports.targetPort".into(),
                    },
                    Authority::Enforced,
                    "iac-kubernetes-port",
                ));
            }
        }
    }
}

fn get_containers(value: &serde_json::Value) -> Vec<&serde_json::Value> {
    let mut containers = Vec::new();

    // Pod spec: spec.containers
    if let Some(arr) = value
        .get("spec")
        .and_then(|v| v.get("containers"))
        .and_then(|v| v.as_array())
    {
        containers.extend(arr.iter());
    }

    // Deployment/StatefulSet: spec.template.spec.containers
    if let Some(arr) = value
        .get("spec")
        .and_then(|v| v.get("template"))
        .and_then(|v| v.get("spec"))
        .and_then(|v| v.get("containers"))
        .and_then(|v| v.as_array())
    {
        containers.extend(arr.iter());
    }

    // CronJob: spec.jobTemplate.spec.template.spec.containers
    if let Some(arr) = value
        .get("spec")
        .and_then(|v| v.get("jobTemplate"))
        .and_then(|v| v.get("spec"))
        .and_then(|v| v.get("template"))
        .and_then(|v| v.get("spec"))
        .and_then(|v| v.get("containers"))
        .and_then(|v| v.as_array())
    {
        containers.extend(arr.iter());
    }

    containers
}

/// Known runtime image prefixes and their concept mappings.
#[allow(clippy::type_complexity)]
const IMAGE_RUNTIME_MAP: &[(&str, fn() -> SemanticConcept)] = &[
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

fn parse_container_image(image: &str, file: &ParsedFile, kind: &str) -> Option<ConfigAssertion> {
    // Parse image:tag format, handling registry prefixes
    let (image_name, tag) = split_image_tag(image)?;

    // Match against known runtimes
    for (prefix, concept_fn) in IMAGE_RUNTIME_MAP {
        if image_name == *prefix || image_name.ends_with(&format!("/{}", prefix)) {
            let version = parse_version(tag);
            let line = find_line_in_text(&file.raw_text, image);
            return Some(ConfigAssertion::new(
                concept_fn(),
                SemanticType::Version(version),
                tag.to_string(),
                SourceLocation {
                    file: file.path.clone(),
                    line,
                    column: 0,
                    key_path: format!("{}.containers.image", kind),
                },
                Authority::Enforced,
                "iac-kubernetes-version",
            ));
        }
    }

    None
}

pub(super) fn split_image_tag(image: &str) -> Option<(&str, &str)> {
    // Handle digest references (image@sha256:...)
    if image.contains('@') {
        return None;
    }

    // Find the last ':' that's not part of a port number in registry URL
    let colon_pos = image.rfind(':')?;
    let name = &image[..colon_pos];
    let tag = &image[colon_pos + 1..];

    // Tag should not be empty and should start with a digit or "latest"
    if tag.is_empty() {
        return None;
    }

    Some((name, tag))
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

    fn make_k8s_file(yaml_str: &str) -> ParsedFile {
        crate::parse::parse_file_with_content(
            &PathBuf::from("deployment.yaml"),
            &PathBuf::from("."),
            yaml_str.to_string(),
        )
        .unwrap()
    }

    #[test]
    fn test_extract_node_version_from_deployment() {
        let yaml = r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: web
spec:
  template:
    spec:
      containers:
        - name: app
          image: node:20-alpine
"#;
        let file = make_k8s_file(yaml);
        let extractor = KubernetesVersionExtractor;
        let assertions = extractor.extract(&file);

        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0].concept.id, "node-version");
        assert_eq!(assertions[0].raw_value, "20-alpine");
        assert_eq!(assertions[0].authority, Authority::Enforced);
    }

    #[test]
    fn test_extract_python_version_from_deployment() {
        let yaml = r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: ml
spec:
  template:
    spec:
      containers:
        - name: app
          image: python:3.12-slim
"#;
        let file = make_k8s_file(yaml);
        let extractor = KubernetesVersionExtractor;
        let assertions = extractor.extract(&file);

        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0].concept.id, "python-version");
        assert_eq!(assertions[0].raw_value, "3.12-slim");
    }

    #[test]
    fn test_extract_port_from_deployment() {
        let yaml = r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: web
spec:
  template:
    spec:
      containers:
        - name: app
          image: node:20
          ports:
            - containerPort: 3000
"#;
        let file = make_k8s_file(yaml);
        let extractor = KubernetesPortExtractor;
        let assertions = extractor.extract(&file);

        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0].concept.id, "app-port");
        assert_eq!(assertions[0].raw_value, "3000");
    }

    #[test]
    fn test_extract_port_from_service() {
        let yaml = r#"
apiVersion: v1
kind: Service
metadata:
  name: web-svc
spec:
  ports:
    - port: 80
      targetPort: 3000
"#;
        let file = make_k8s_file(yaml);
        let extractor = KubernetesPortExtractor;
        let assertions = extractor.extract(&file);

        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0].raw_value, "3000");
    }

    #[test]
    fn test_no_extraction_from_non_k8s_yaml() {
        let yaml = r#"
name: my-config
value: something
"#;
        let file = make_k8s_file(yaml);
        let extractor = KubernetesVersionExtractor;
        let assertions = extractor.extract(&file);
        assert!(assertions.is_empty());
    }

    #[test]
    fn test_image_without_tag_is_skipped() {
        let yaml = r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: web
spec:
  template:
    spec:
      containers:
        - name: app
          image: myregistry/myapp
"#;
        let file = make_k8s_file(yaml);
        let extractor = KubernetesVersionExtractor;
        let assertions = extractor.extract(&file);
        assert!(assertions.is_empty());
    }

    #[test]
    fn test_split_image_tag() {
        assert_eq!(split_image_tag("node:20"), Some(("node", "20")));
        assert_eq!(
            split_image_tag("python:3.12-slim"),
            Some(("python", "3.12-slim"))
        );
        assert_eq!(split_image_tag("myapp"), None);
        assert_eq!(split_image_tag("myapp@sha256:abc"), None);
    }
}
