use crate::common::e2e_helpers::*;
use crate::common::{TestWorkspace, conflic_cmd, conflic_cmd_in};
use predicates::prelude::*;
use std::path::Path;
use std::process::{Command as ProcessCommand, Stdio};
#[cfg(feature = "lsp")]
use tower_lsp::lsp_types::Url;

#[test]
fn test_cli_init_creates_template_config() {
    let workspace = TestWorkspace::new();

    conflic_cmd()
        .arg(workspace.root())
        .arg("--init")
        .assert()
        .success()
        .stdout(predicate::str::contains("Created"));

    let config = workspace.read(".conflic.toml");
    assert!(
        config.contains("[conflic]"),
        "expected init to create the template config, got:\n{}",
        config
    );
    assert!(
        config.contains("Custom extractors - define your own concepts without writing Rust"),
        "expected init template to contain the ASCII custom extractor comment, got:\n{}",
        config
    );
}

#[test]
fn test_cli_init_fails_when_config_already_exists() {
    let workspace = TestWorkspace::new();
    workspace.write(".conflic.toml", "[conflic]\nseverity = \"warning\"\n");

    conflic_cmd()
        .arg(workspace.root())
        .arg("--init")
        .assert()
        .code(3)
        .stderr(predicate::str::contains(".conflic.toml already exists"));
}

#[test]
fn test_cli_config_format_json_is_used_when_flag_missing() {
    let workspace = TestWorkspace::new();
    workspace.write("package.json", r#"{"engines":{"node":"18"}}"#);
    workspace.write("Dockerfile", "FROM node:20-alpine\n");
    workspace.write(
        ".conflic.toml",
        r#"[conflic]
format = "json"
"#,
    );

    let assert = conflic_cmd_in(workspace.root()).arg(".").assert().code(1);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert!(
        parsed.get("concepts").is_some(),
        "expected JSON output: {stdout}"
    );
}

#[test]
fn test_cli_config_severity_is_used_when_flag_missing() {
    let workspace = TestWorkspace::new();
    workspace.write(".nvmrc", "18\n");
    workspace.write("Dockerfile", "FROM node:20-alpine\n");
    workspace.write(
        ".conflic.toml",
        r#"[conflic]
severity = "error"
"#,
    );

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--no-color")
        .assert()
        .success()
        .stdout(predicate::str::contains("Node.js Version"));
}

#[test]
fn test_cli_severity_flag_overrides_config_file() {
    let workspace = TestWorkspace::new();
    workspace.write(".nvmrc", "18\n");
    workspace.write("Dockerfile", "FROM node:20-alpine\n");
    workspace.write(
        ".conflic.toml",
        r#"[conflic]
severity = "warning"
"#,
    );

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--severity")
        .arg("error")
        .arg("--no-color")
        .assert()
        .success()
        .stdout(predicate::str::contains("Node.js Version"));
}

#[test]
fn test_cli_ignore_rules_suppress_targeted_findings() {
    let workspace = TestWorkspace::new();
    workspace.write(".nvmrc", "18\n");
    workspace.write("Dockerfile", "FROM node:20-alpine\n");
    workspace.write(
        ".conflic.toml",
        r#"[[ignore]]
rule = "VER001"
files = [".nvmrc", "Dockerfile"]
reason = "Intentional drift"
"#,
    );

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--no-color")
        .assert()
        .success();
}

#[test]
fn test_cli_custom_extractors_detect_contradictions_from_config() {
    let workspace = TestWorkspace::new();
    workspace.write(
        ".conflic.toml",
        r#"[[custom_extractor]]
concept = "redis-version"
display_name = "Redis Version"
category = "runtime-version"
type = "version"

[[custom_extractor.source]]
file = "docker-compose.yml"
format = "yaml"
path = "services.redis.image"
pattern = "redis:(.*)"
authority = "enforced"

[[custom_extractor.source]]
file = ".env"
format = "env"
key = "REDIS_VERSION"
authority = "declared"
"#,
    );
    workspace.write(
        "docker-compose.yml",
        "services:\n  redis:\n    image: redis:7.2\n",
    );
    workspace.write(".env", "REDIS_VERSION=7.0\n");

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--no-color")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Redis Version"));
}

#[test]
fn test_cli_skip_concepts_ignores_port_conflicts() {
    let workspace = TestWorkspace::new();
    workspace.write_port_workspace("3000", "8080");
    workspace.write(
        ".conflic.toml",
        r#"[conflic]
skip_concepts = ["port"]
"#,
    );

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--no-color")
        .assert()
        .success();
}

#[test]
fn test_cli_severity_error_ignores_warning_only_findings() {
    let workspace = TestWorkspace::new();
    workspace.write(".nvmrc", "18\n");
    workspace.write("Dockerfile", "FROM node:20-alpine\n");

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--severity")
        .arg("error")
        .arg("--no-color")
        .assert()
        .success();
}

#[test]
fn test_cli_quiet_is_silent_for_clean_workspaces() {
    let workspace = TestWorkspace::new();
    workspace.write_node_workspace("20", "20", "20-alpine");

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--quiet")
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn test_cli_quiet_still_prints_when_findings_exist() {
    let workspace = TestWorkspace::new();
    workspace.write_node_workspace("18", "18", "20-alpine");

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--quiet")
        .arg("--no-color")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Node.js Version"));
}
