use crate::common::integration_helpers::*;
use conflic::config::ConflicConfig;
use conflic::fix::plan_fixes;

#[test]
fn test_docker_compose_yaml_variant_is_discovered() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join(".env"), "PORT=3000\n").unwrap();
    std::fs::write(
        root.join("docker-compose.override.yaml"),
        r#"services:
  app:
    image: node:20-alpine
    ports:
      - "8080:8080"
"#,
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let port = concept_result(&result, "app-port");

    assert!(
        port.assertions
            .iter()
            .any(|assertion| assertion.extractor_id == "port-docker-compose"),
        "docker-compose YAML variants should be discovered for port extraction"
    );
    assert!(
        !port.findings.is_empty(),
        "docker-compose YAML variants should participate in contradiction detection"
    );
}

#[test]
fn test_docker_from_variants_with_registry_ports_and_flags_are_detected() {
    for dockerfile in [
        "FROM --platform=linux/amd64 node:20-alpine\n",
        "FROM localhost:5000/node:20-alpine\n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::write(root.join(".nvmrc"), "18\n").unwrap();
        std::fs::write(root.join("Dockerfile"), dockerfile).unwrap();

        let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
        let node = concept_result(&result, "node-version");

        assert!(
            node.assertions
                .iter()
                .any(|assertion| assertion.extractor_id == "node-version-dockerfile"),
            "docker FROM variant should still produce a dockerfile assertion for input: {}",
            dockerfile.trim()
        );
        assert!(
            !node.findings.is_empty(),
            "docker FROM variant should participate in contradiction detection for input: {}",
            dockerfile.trim()
        );
    }
}

#[test]
fn test_docker_compose_host_ip_port_mapping_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join(".env"), "PORT=3000\n").unwrap();
    std::fs::write(
        root.join("docker-compose.yml"),
        r#"services:
  app:
    image: node:20-alpine
    ports:
      - "127.0.0.1:8080:8080"
"#,
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let port = concept_result(&result, "app-port");

    assert!(
        port.assertions
            .iter()
            .any(|assertion| assertion.extractor_id == "port-docker-compose"),
        "compose host-ip mappings should still produce an enforced port assertion"
    );
    assert!(
        !port.findings.is_empty(),
        "compose host-ip mappings should participate in contradiction detection"
    );
}

#[test]
fn test_docker_compose_long_form_port_mapping_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join(".env"), "PORT=3000\n").unwrap();
    std::fs::write(
        root.join("docker-compose.yml"),
        r#"services:
  app:
    image: node:20-alpine
    ports:
      - target: 8080
        published: 8080
        protocol: tcp
"#,
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let port = concept_result(&result, "app-port");

    assert!(
        port.assertions
            .iter()
            .any(|assertion| assertion.extractor_id == "port-docker-compose"),
        "compose long-form mappings should produce an enforced port assertion"
    );
    assert!(
        !port.findings.is_empty(),
        "compose long-form mappings should participate in contradiction detection"
    );
}

#[test]
fn test_docker_compose_long_form_published_range_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join(".env"), "PORT=4000\n").unwrap();
    std::fs::write(
        root.join("docker-compose.yml"),
        r#"services:
  app:
    image: node:20-alpine
    ports:
      - target: 3000
        published: "8080-8082"
        protocol: tcp
"#,
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let port = concept_result(&result, "app-port");

    assert!(
        port.assertions
            .iter()
            .any(|assertion| assertion.extractor_id == "port-docker-compose"),
        "compose long-form published ranges should still produce an enforced port assertion"
    );
    assert!(
        !port.findings.is_empty(),
        "compose long-form published ranges should participate in contradiction detection"
    );
}

#[test]
fn test_docker_compose_short_range_mapping_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join("Dockerfile"), "FROM node:20\nEXPOSE 3000\n").unwrap();
    std::fs::write(
        root.join("docker-compose.yml"),
        r#"services:
  app:
    image: node:20-alpine
    ports:
      - "9090-9091:8080-8081"
"#,
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let port = concept_result(&result, "app-port");

    assert!(
        port.assertions
            .iter()
            .any(|assertion| assertion.extractor_id == "port-docker-compose"),
        "compose short range mappings should still produce an enforced port assertion"
    );
    assert!(
        !port.findings.is_empty(),
        "compose short range mappings should participate in contradiction detection"
    );
}

#[test]
fn test_fix_plan_handles_dockerfile_platform_flags() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join("package.json"), r#"{"engines":{"node":"20"}}"#).unwrap();
    std::fs::write(
        root.join("Dockerfile"),
        "FROM --platform=linux/amd64 node:18-alpine AS build\nFROM nginx:1.27-alpine\n",
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let plan = plan_fixes(&result);

    assert!(
        plan.proposals.iter().any(|proposal| {
            proposal.file.ends_with("Dockerfile")
                && proposal
                    .proposed_raw
                    .starts_with("--platform=linux/amd64 node:20-alpine")
        }),
        "docker fix planning should preserve leading FROM flags: {:?}",
        plan.proposals
    );
}
