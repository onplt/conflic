use crate::common::e2e_helpers::*;
use crate::common::{TestWorkspace, conflic_cmd, conflic_cmd_in};
use predicates::prelude::*;
use std::path::Path;
use std::process::{Command as ProcessCommand, Stdio};
#[cfg(feature = "lsp")]
use tower_lsp::lsp_types::Url;

#[test]
fn test_cli_version_output() {
    conflic_cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("conflic "));
}

#[test]
fn test_cli_help_output() {
    conflic_cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Detect semantic contradictions across config files",
        ))
        .stdout(predicate::str::contains("--diff"))
        .stdout(predicate::str::contains("--fix"))
        .stdout(predicate::str::contains("--explain").not());
}

#[test]
fn test_cli_json_output_is_valid_and_parsable() {
    let workspace = TestWorkspace::new();
    workspace.write_node_workspace("20", "18.0.0", "20-alpine");

    let assert = conflic_cmd()
        .arg(workspace.root())
        .arg("--format")
        .arg("json")
        .assert()
        .code(1);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert!(parsed.get("version").is_some(), "missing version field");
    assert!(parsed.get("concepts").is_some(), "missing concepts field");
    assert!(parsed.get("summary").is_some(), "missing summary field");
}

#[test]
fn test_cli_sarif_output_is_valid_and_parsable() {
    let workspace = TestWorkspace::new();
    workspace.write_node_workspace("20", "18.0.0", "20-alpine");

    let assert = conflic_cmd()
        .arg(workspace.root())
        .arg("--format")
        .arg("sarif")
        .assert()
        .code(1);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(parsed["version"], "2.1.0");
    assert!(
        parsed["runs"].is_array(),
        "expected SARIF runs array: {}",
        stdout
    );
}

#[test]
fn test_cli_clean_workspace_exits_zero() {
    let workspace = TestWorkspace::new();
    workspace.write_node_workspace("20", "20", "20-alpine");
    workspace.write_docker_compose_service("app", "node:20-alpine");

    conflic_cmd()
        .arg(workspace.root())
        .arg("--no-color")
        .assert()
        .success();
}

#[test]
fn test_cli_contradiction_workspace_exits_one() {
    let workspace = TestWorkspace::new();
    workspace.write_node_workspace("18", "18.0.0", "20-alpine");

    conflic_cmd()
        .arg(workspace.root())
        .arg("--no-color")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Node.js Version"))
        .stdout(predicate::str::contains("18"))
        .stdout(predicate::str::contains("20"));
}

#[test]
fn test_cli_detects_node_version_in_github_workflow() {
    let workspace = TestWorkspace::new();
    workspace.write(".nvmrc", "18\n");
    workspace.write(
        ".github/workflows/ci.yml",
        r#"name: ci
on:
  push:
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/setup-node@v4
        with:
          node-version: 20
"#,
    );

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--no-color")
        .assert()
        .code(2)
        .stdout(predicate::str::contains("Node.js Version"))
        .stdout(predicate::str::contains("20"))
        .stdout(predicate::str::contains("18"));
}

#[test]
fn test_cli_check_filters_to_requested_concept() {
    let workspace = TestWorkspace::new();
    workspace.write(".nvmrc", "20\n");
    workspace.write("package.json", r#"{"engines":{"node":"20"}}"#);
    workspace.write_python_conflict_workspace("3.11", ">=3.12,<3.13");
    workspace.write("Dockerfile", "FROM python:3.12-slim\n");

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--check")
        .arg("node-version")
        .arg("--no-color")
        .assert()
        .success()
        .stdout(predicate::str::contains("Python Version").not());
}
