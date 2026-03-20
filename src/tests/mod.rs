use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

use crate::config::ConflicConfig;

fn default_config() -> ConflicConfig {
    ConflicConfig::default()
}

fn write_file(root: &std::path::Path, relative: &str, content: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

#[test]
fn missing_scan_root_cli_exits_with_code_one() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("missing-workspace");

    Command::cargo_bin("conflic")
        .unwrap()
        .arg(&missing)
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("Scan root does not exist"));
}

#[test]
fn gitlab_ci_directory_yaml_is_scanned_as_ci_input() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(root, ".nvmrc", "18\n");
    write_file(root, ".gitlab-ci/build.yml", "job:\n  node-version: 20\n");

    let result = crate::scan(root, &default_config()).unwrap();
    let node = result
        .concept_results
        .iter()
        .find(|concept| concept.concept.id == "node-version")
        .expect("node-version concept should be present");

    assert_eq!(node.assertions.len(), 2);
    assert_eq!(node.findings.len(), 1);
}

#[test]
fn nested_gitlab_ci_yml_is_not_treated_as_root_ci_config() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(root, ".nvmrc", "18\n");
    write_file(root, "subdir/.gitlab-ci.yml", "job:\n  node-version: 20\n");

    let report = crate::scan_doctor(root, &default_config()).unwrap();
    assert!(
        !report.discovered_files.contains_key(".gitlab-ci.yml"),
        "nested .gitlab-ci.yml should not be discovered as a CI config: {:?}",
        report.discovered_files
    );

    let node = report
        .scan_result
        .concept_results
        .iter()
        .find(|concept| concept.concept.id == "node-version")
        .expect("node-version concept should be present");
    assert_eq!(node.assertions.len(), 1);
    assert!(node.findings.is_empty());
}

#[test]
fn multiline_docker_from_produces_runtime_assertion() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(root, ".nvmrc", "18\n");
    write_file(
        root,
        "Dockerfile",
        "FROM --platform=linux/amd64 \\\n  node:20-alpine\n",
    );

    let result = crate::scan(root, &default_config()).unwrap();
    let node = result
        .concept_results
        .iter()
        .find(|concept| concept.concept.id == "node-version")
        .expect("node-version concept should be present");

    assert_eq!(node.assertions.len(), 2);
    assert_eq!(node.findings.len(), 1);
}

#[test]
fn same_file_assertions_do_not_generate_extra_contradictions() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(root, ".env", "PORT=3000\n");
    write_file(root, "Dockerfile", "EXPOSE 4000 3000\n");

    let result = crate::scan(root, &default_config()).unwrap();
    let port = result
        .concept_results
        .iter()
        .find(|concept| concept.concept.id == "app-port")
        .expect("app-port concept should be present");

    assert_eq!(port.assertions.len(), 3);
    assert_eq!(
        port.findings.len(),
        1,
        "only the cross-file mismatch should remain"
    );
}
