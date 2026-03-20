mod common;

use common::{TestWorkspace, conflic_cmd, conflic_cmd_in};
use predicates::prelude::*;
#[cfg(feature = "lsp")]
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
#[cfg(feature = "lsp")]
use std::process::{Command as ProcessCommand, Stdio};
#[cfg(feature = "lsp")]
use std::sync::mpsc::{self, Receiver};
#[cfg(feature = "lsp")]
use std::thread;
#[cfg(feature = "lsp")]
use std::time::Duration;
#[cfg(feature = "lsp")]
use tower_lsp::lsp_types::Url;

fn all_line_endings_are_crlf(bytes: &[u8]) -> bool {
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' && (index == 0 || bytes[index - 1] != b'\r') {
            return false;
        }
    }
    true
}

fn file_name(path: &str) -> Option<&str> {
    Path::new(path).file_name().and_then(|name| name.to_str())
}

#[cfg(feature = "lsp")]
fn lsp_uri_matches_path(uri: &str, path: &Path) -> bool {
    let Ok(url) = Url::parse(uri) else {
        return false;
    };
    let Ok(uri_path) = url.to_file_path() else {
        return false;
    };

    let normalized_uri_path = std::fs::canonicalize(&uri_path).unwrap_or(uri_path);
    let normalized_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    normalized_uri_path == normalized_path
}

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
fn test_cli_diff_mode_detects_new_contradiction() {
    let workspace = TestWorkspace::new();
    workspace.write_node_workspace("20", "20.0.0", "20-alpine");
    workspace.write("global.json", "{ invalid json");
    workspace.init_git_repo();
    workspace.git_add_and_commit("initial");

    workspace.write("package.json", r#"{"engines":{"node":"18.0.0"}}"#);

    conflic_cmd()
        .arg(workspace.root())
        .arg("--diff")
        .arg("HEAD")
        .arg("--format")
        .arg("json")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Node.js Version"))
        .stdout(predicate::str::contains("18.0.0"))
        .stdout(predicate::str::contains("PARSE001").not());
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
fn test_cli_fix_mode_mutates_workspace_and_resolves_contradiction() {
    let workspace = TestWorkspace::new();
    workspace.write_node_workspace("18", "18", "20-alpine");

    conflic_cmd()
        .arg(workspace.root())
        .arg("--fix")
        .arg("--yes")
        .arg("--no-color")
        .arg("--no-backup")
        .assert()
        .success()
        .stdout(predicate::str::contains("fix preview"));

    let nvmrc = workspace.read(".nvmrc");
    let package_json = workspace.read("package.json");

    assert_eq!(nvmrc, "20\n");
    assert!(
        package_json.contains(r#""node":"20""#),
        "expected package.json to be updated to the winning version, got:\n{}",
        package_json
    );

    conflic_cmd()
        .arg(workspace.root())
        .arg("--no-color")
        .assert()
        .success();
}

#[test]
fn test_cli_fix_preserves_jsonc_comments_and_crlf() {
    let workspace = TestWorkspace::new();
    workspace.write("Dockerfile", "FROM node:20-alpine\n");
    workspace.write(
        "package.json",
        "{\r\n  // keep this comment\r\n  \"engines\": {\r\n    \"node\": \"18\"\r\n  }\r\n}\r\n",
    );

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--fix")
        .arg("--yes")
        .assert()
        .success();

    let package_json_bytes = std::fs::read(workspace.path("package.json")).unwrap();
    let package_json = String::from_utf8(package_json_bytes.clone()).unwrap();

    assert!(
        package_json.contains("\"node\": \"20\""),
        "expected version to be fixed, got:\n{}",
        package_json
    );
    assert!(
        package_json.contains("// keep this comment"),
        "expected inline comment to be preserved, got:\n{}",
        package_json
    );
    assert!(
        all_line_endings_are_crlf(&package_json_bytes),
        "expected CRLF line endings to be preserved, got bytes: {:?}",
        package_json_bytes
    );
}

#[test]
fn test_cli_fix_uses_atomic_replace_and_keeps_backups() {
    let workspace = TestWorkspace::new();
    workspace.write_node_workspace("18", "18", "20-alpine");

    let original_nvmrc = workspace.read(".nvmrc");
    let original_package_json = workspace.read("package.json");

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--fix")
        .arg("--yes")
        .arg("--no-color")
        .assert()
        .success()
        .stdout(predicate::str::contains("fix preview"));

    assert_eq!(workspace.read(".nvmrc"), "20\n");
    assert!(
        workspace.read("package.json").contains(r#""node":"20""#),
        "expected package.json to be updated"
    );
    assert_eq!(workspace.read(".nvmrc.conflic.bak"), original_nvmrc);
    assert_eq!(
        workspace.read("package.json.conflic.bak"),
        original_package_json
    );

    let temp_files: Vec<_> = std::fs::read_dir(workspace.root())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.contains(".conflic.tmp."))
        .collect();

    assert!(
        temp_files.is_empty(),
        "atomic fix path should not leave temp files behind, got {:?}",
        temp_files
    );
}

#[test]
fn test_cli_fix_preserves_crlf_for_line_based_env_rewrites() {
    let workspace = TestWorkspace::new();
    workspace.write(".env", "HOST=localhost\r\nPORT=3000\r\nNAME=demo\r\n");
    workspace.write(
        "docker-compose.yml",
        "services:\n  app:\n    image: node:20-alpine\n    ports:\n      - \"8080:8080\"\n",
    );

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--fix")
        .arg("--yes")
        .arg("--no-backup")
        .arg("--no-color")
        .assert()
        .success()
        .stdout(predicate::str::contains("fix preview"));

    let env_bytes = std::fs::read(workspace.path(".env")).unwrap();
    let env_text = String::from_utf8(env_bytes.clone()).unwrap();

    assert!(
        env_text.contains("PORT=8080"),
        "expected line-based env fix to update the port, got:\n{}",
        env_text
    );
    assert!(
        all_line_endings_are_crlf(&env_bytes),
        "expected CRLF line endings to be preserved for line-based fixes, got bytes: {:?}",
        env_bytes
    );
}

#[test]
fn test_cli_fix_updates_multiline_pom_xml_without_touching_duplicate_values() {
    let workspace = TestWorkspace::new();
    workspace.write(
        "pom.xml",
        r#"<project>
  <properties>
    <java.version>
      17
    </java.version>
  </properties>
  <description>Java 17 docs</description>
</project>
"#,
    );
    workspace.write("Dockerfile", "FROM eclipse-temurin:21-jdk\n");

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--fix")
        .arg("--yes")
        .arg("--no-backup")
        .arg("--no-color")
        .assert()
        .success();

    let fixed = workspace.read("pom.xml");
    assert!(
        fixed.contains(
            r#"<java.version>
      21
    </java.version>"#
        ),
        "expected the multiline java.version value to be updated, got:\n{}",
        fixed
    );
    assert!(
        fixed.contains("<description>Java 17 docs</description>"),
        "duplicate non-target XML text should remain untouched, got:\n{}",
        fixed
    );

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--no-color")
        .assert()
        .success()
        .stdout(predicate::str::contains("Java Version").not());
}

#[test]
fn test_cli_fix_updates_multiline_csproj_without_touching_duplicate_values() {
    let workspace = TestWorkspace::new();
    workspace.write(
        "MyApp.csproj",
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>
      net8.0
    </TargetFramework>
    <Description>Targets net8.0 docs</Description>
  </PropertyGroup>
</Project>
"#,
    );
    workspace.write("global.json", r#"{"sdk":{"version":"9.0.100"}}"#);

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--fix")
        .arg("--yes")
        .arg("--no-backup")
        .arg("--no-color")
        .assert()
        .success();

    let fixed = workspace.read("MyApp.csproj");
    assert!(
        fixed.contains(
            r#"<TargetFramework>
      net9.0
    </TargetFramework>"#
        ),
        "expected the multiline TargetFramework value to be updated, got:\n{}",
        fixed
    );
    assert!(
        fixed.contains("<Description>Targets net8.0 docs</Description>"),
        "duplicate non-target XML text should remain untouched, got:\n{}",
        fixed
    );

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--no-color")
        .assert()
        .success()
        .stdout(predicate::str::contains(".NET Version").not());
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
fn test_cli_monorepo_per_package_scoping_avoids_cross_package_conflicts() {
    let workspace = TestWorkspace::new();
    workspace.write_monorepo_package("a", "18", "18");
    workspace.write_monorepo_package("b", "20", "20");
    workspace.write(
        ".conflic.toml",
        r#"[monorepo]
per_package = true
package_roots = ["packages/*"]
"#,
    );

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--no-color")
        .assert()
        .success();
}

#[test]
fn test_cli_monorepo_global_concepts_reintroduce_cross_package_conflicts() {
    let workspace = TestWorkspace::new();
    workspace.write_monorepo_package("a", "18", "18");
    workspace.write_monorepo_package("b", "20", "20");
    workspace.write(
        ".conflic.toml",
        r#"[monorepo]
per_package = true
package_roots = ["packages/*"]
global_concepts = ["node-version"]
"#,
    );

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--no-color")
        .assert()
        .code(2)
        .stdout(predicate::str::contains("Node.js Version"));
}

#[test]
fn test_cli_monorepo_root_level_contradiction_is_reported_once() {
    let workspace = TestWorkspace::new();
    workspace.write(".nvmrc", "18\n");
    workspace.write("Dockerfile", "FROM node:20-alpine\n");
    workspace.write_monorepo_package("a", "18", "18");
    workspace.write(
        ".conflic.toml",
        r#"[monorepo]
per_package = true
package_roots = ["packages/*"]
"#,
    );

    let assert = conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--format")
        .arg("json")
        .assert()
        .code(1);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let findings = parsed["concepts"][0]["findings"]
        .as_array()
        .expect("node-version findings should be an array");

    let root_pair_count = findings
        .iter()
        .filter(|finding| {
            let left = finding["left"]["file"].as_str().unwrap_or_default();
            let right = finding["right"]["file"].as_str().unwrap_or_default();

            matches!(
                (file_name(left), file_name(right)),
                (Some(".nvmrc"), Some("Dockerfile")) | (Some("Dockerfile"), Some(".nvmrc"))
            ) && !left.contains("packages")
                && !right.contains("packages")
        })
        .count();

    assert_eq!(
        root_pair_count, 1,
        "root-level .nvmrc vs Dockerfile contradiction should appear exactly once:\n{}",
        stdout
    );
}

#[test]
fn test_cli_baseline_workflow_suppresses_known_findings_and_catches_new_ones() {
    let workspace = TestWorkspace::new();
    workspace.write_node_workspace("18", "18", "20-alpine");

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--update-baseline")
        .arg(".conflic-baseline.json")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Baseline updated"));

    assert!(
        workspace.path(".conflic-baseline.json").exists(),
        "baseline file should be created"
    );

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--baseline")
        .arg(".conflic-baseline.json")
        .arg("--no-color")
        .assert()
        .success();

    workspace.write_port_workspace("3000", "8080");

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--baseline")
        .arg(".conflic-baseline.json")
        .arg("--no-color")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Application Port"))
        .stdout(predicate::str::contains("8080"))
        .stdout(predicate::str::contains("3000"))
        .stdout(predicate::str::contains("Node.js Version").not());
}

#[test]
fn test_cli_baseline_keeps_changed_node_values_visible() {
    let workspace = TestWorkspace::new();
    workspace.write_node_workspace("18", "18", "20-alpine");

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--update-baseline")
        .arg(".conflic-baseline.json")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Baseline updated"));

    workspace.write(".nvmrc", "16\n");
    workspace.write("package.json", r#"{"engines":{"node":"16"}}"#);

    let assert = conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--baseline")
        .arg(".conflic-baseline.json")
        .arg("--no-color")
        .assert()
        .code(1);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    assert!(
        stdout.contains("Node.js Version"),
        "changed value should still be reported after baseline filtering:\n{}",
        stdout
    );
    assert!(
        stdout.contains("16"),
        "expected the updated node version to remain visible after baseline filtering:\n{}",
        stdout
    );
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
fn test_cli_env_inline_comments_and_quotes_do_not_hide_port_conflicts() {
    let workspace = TestWorkspace::new();
    workspace.write(
        ".env",
        "PORT=8080 # app port\nAPP_PORT=\"8080\" # quoted app port\nSERVER_PORT='8080' # single quoted\nHOST=\"localhost # keep hash\"\n",
    );
    workspace.write("Dockerfile", "EXPOSE 3000\n");

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--no-color")
        .assert()
        .code(2)
        .stdout(predicate::str::contains("Application Port"))
        .stdout(predicate::str::contains("(PORT)"))
        .stdout(predicate::str::contains("(APP_PORT)"))
        .stdout(predicate::str::contains("(SERVER_PORT)"))
        .stdout(predicate::str::contains("8080"))
        .stdout(predicate::str::contains("3000"));
}

#[test]
fn test_cli_port_ranges_treat_inside_and_boundary_values_as_compatible() {
    let inside_range = TestWorkspace::new();
    inside_range.write(".env", "PORT=3001\n");
    inside_range.write("Dockerfile", "EXPOSE 3000-3005\n");

    conflic_cmd_in(inside_range.root())
        .arg(".")
        .arg("--no-color")
        .assert()
        .success()
        .stdout(predicate::str::contains("Application Port").not());

    let boundary_value = TestWorkspace::new();
    boundary_value.write(".env", "PORT=3000\n");
    boundary_value.write("Dockerfile", "EXPOSE 3000-3005\n");

    conflic_cmd_in(boundary_value.root())
        .arg(".")
        .arg("--no-color")
        .assert()
        .success()
        .stdout(predicate::str::contains("Application Port").not());
}

#[test]
fn test_cli_port_ranges_still_report_values_outside_the_range() {
    let workspace = TestWorkspace::new();
    workspace.write(".env", "PORT=3006\n");
    workspace.write("Dockerfile", "EXPOSE 3000-3005\n");

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--no-color")
        .assert()
        .code(2)
        .stdout(predicate::str::contains("Application Port"))
        .stdout(predicate::str::contains("3006"))
        .stdout(predicate::str::contains("3000-3005"));
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

#[test]
fn test_cli_fix_dry_run_does_not_modify_files() {
    let workspace = TestWorkspace::new();
    workspace.write_node_workspace("18", "18", "20-alpine");
    let before_nvmrc = workspace.read(".nvmrc");
    let before_package_json = workspace.read("package.json");

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--fix")
        .arg("--dry-run")
        .arg("--no-color")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("fix preview"));

    assert_eq!(workspace.read(".nvmrc"), before_nvmrc);
    assert_eq!(workspace.read("package.json"), before_package_json);
}

#[test]
fn test_cli_fix_no_backup_avoids_creating_backup_files() {
    let workspace = TestWorkspace::new();
    workspace.write_node_workspace("18", "18", "20-alpine");

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--fix")
        .arg("--yes")
        .arg("--no-backup")
        .arg("--no-color")
        .assert()
        .success();

    let backups: Vec<_> = std::fs::read_dir(workspace.root())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().contains(".conflic.bak"))
        .collect();

    assert!(
        backups.is_empty(),
        "no-backup should avoid creating backup files, got {:?}",
        backups
    );
}

#[test]
fn test_cli_fix_concept_only_updates_requested_concept() {
    let workspace = TestWorkspace::new();
    workspace.write_node_workspace("18", "18", "20-alpine");
    workspace.write_port_workspace("3000", "8080");

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--fix")
        .arg("--concept")
        .arg("port")
        .arg("--yes")
        .arg("--no-backup")
        .arg("--no-color")
        .assert()
        .success();

    assert_eq!(workspace.read(".env"), "PORT=8080\n");
    assert_eq!(workspace.read(".nvmrc"), "18\n");
    assert!(
        workspace.read("package.json").contains(r#""node":"18""#),
        "node-version files should remain untouched"
    );

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--no-color")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Node.js Version"));
}

#[cfg(feature = "lsp")]
const LSP_MESSAGE_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(feature = "lsp")]
struct LspMessageReader {
    receiver: Receiver<String>,
}

#[cfg(feature = "lsp")]
impl LspMessageReader {
    fn spawn(stdout: std::process::ChildStdout) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Some(message) = read_lsp_message_from_bufread(&mut reader) {
                if sender.send(message).is_err() {
                    break;
                }
            }
        });

        Self { receiver }
    }

    fn read_message(&mut self) -> Option<String> {
        self.receiver.recv_timeout(LSP_MESSAGE_TIMEOUT).ok()
    }
}

#[cfg(feature = "lsp")]
fn write_lsp_message(stdin: &mut impl Write, payload: &str) {
    write!(
        stdin,
        "Content-Length: {}\r\n\r\n{}",
        payload.len(),
        payload
    )
    .expect("lsp message should be written");
    stdin.flush().expect("lsp stdin should flush");
}

#[cfg(feature = "lsp")]
fn read_lsp_message_from_bufread(reader: &mut impl BufRead) -> Option<String> {
    let length = read_lsp_content_length(reader)?;
    let mut body = vec![0_u8; length];
    reader
        .read_exact(&mut body)
        .expect("lsp body should be readable");
    Some(String::from_utf8(body).expect("lsp body should be utf-8"))
}

#[cfg(feature = "lsp")]
fn read_lsp_message(reader: &mut LspMessageReader) -> Option<String> {
    reader.read_message()
}

#[cfg(feature = "lsp")]
fn read_lsp_message_matching(
    reader: &mut LspMessageReader,
    predicate: impl Fn(&str) -> bool,
) -> Option<String> {
    for _ in 0..48 {
        let message = read_lsp_message(reader)?;
        if predicate(&message) {
            return Some(message);
        }
    }

    None
}

#[cfg(feature = "lsp")]
fn read_lsp_json_message_matching(
    reader: &mut LspMessageReader,
    predicate: impl Fn(&serde_json::Value) -> bool,
) -> Option<serde_json::Value> {
    for _ in 0..48 {
        let message = read_lsp_message(reader)?;
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&message) else {
            continue;
        };
        if predicate(&json) {
            return Some(json);
        }
    }

    None
}

#[cfg(feature = "lsp")]
fn read_lsp_content_length(reader: &mut impl BufRead) -> Option<usize> {
    let mut content_length = None;
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = reader
            .read_line(&mut line)
            .expect("lsp header should be readable");
        if bytes_read == 0 {
            return None;
        }

        if line == "\r\n" {
            return content_length;
        }

        if let Some(value) = line.strip_prefix("Content-Length:") {
            content_length = Some(value.trim().parse().expect("content length should parse"));
        }
    }
}

#[cfg(feature = "lsp")]
#[test]
fn test_cli_lsp_uses_unsaved_buffer_diagnostics_and_targeted_code_actions() {
    let workspace = TestWorkspace::new();
    workspace.write("Dockerfile", "FROM node:20-alpine\n");

    let saved_package =
        "{\r\n  // keep this comment\r\n  \"engines\": {\r\n    \"node\": \"20\"\r\n  }\r\n}\r\n";
    let unsaved_package =
        "{\r\n  // keep this comment\r\n  \"engines\": {\r\n    \"node\": \"18\"\r\n  }\r\n}\r\n";
    workspace.write("package.json", saved_package);

    let root_uri = Url::from_file_path(workspace.root()).unwrap().to_string();
    let package_path = workspace.path("package.json");
    let package_uri = Url::from_file_path(&package_path).unwrap().to_string();

    let mut child = ProcessCommand::new(env!("CARGO_BIN_EXE_conflic"))
        .arg("--lsp")
        .env("CONFLIC_LSP_SCAN_STATS", "1")
        .current_dir(workspace.root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("lsp process should spawn");

    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut reader = LspMessageReader::spawn(stdout);

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "processId": serde_json::Value::Null,
                    "rootUri": root_uri.clone(),
                    "capabilities": {}
                }
            })
            .to_string(),
        );
    }

    let initialize_response = read_lsp_json_message_matching(&mut reader, |json| {
        json.get("id").and_then(|id| id.as_i64()) == Some(1)
    })
    .expect("initialize response should exist");
    assert!(
        initialize_response.get("result").is_some(),
        "expected initialize response, got {}",
        initialize_response
    );
    assert_eq!(
        initialize_response["result"]["capabilities"]["textDocumentSync"]["change"].as_u64(),
        Some(2),
        "server should advertise incremental text sync"
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
        );
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": package_uri.clone(),
                        "languageId": "json",
                        "version": 1,
                        "text": saved_package
                    }
                }
            })
            .to_string(),
        );
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didChange",
                "params": {
                    "textDocument": {
                        "uri": package_uri.clone(),
                        "version": 2
                    },
                    "contentChanges": [
                        {
                            "text": unsaved_package
                        }
                    ]
                }
            })
            .to_string(),
        );
    }

    let diagnostics_message = read_lsp_json_message_matching(&mut reader, |json| {
        json.get("method").and_then(|method| method.as_str())
            == Some("textDocument/publishDiagnostics")
            && json
                .get("params")
                .and_then(|params| params.get("uri"))
                .and_then(|uri| uri.as_str())
                .is_some_and(|uri| lsp_uri_matches_path(uri, &package_path))
            && json
                .get("params")
                .and_then(|params| params.get("diagnostics"))
                .and_then(|diagnostics| diagnostics.as_array())
                .is_some_and(|diagnostics| !diagnostics.is_empty())
    })
    .expect("publishDiagnostics for unsaved package.json should exist");

    let diagnostics = diagnostics_message["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics should be an array");
    let diagnostic_message = diagnostics[0]["message"]
        .as_str()
        .expect("diagnostic message should be a string");
    assert!(
        diagnostic_message.contains("18"),
        "expected diagnostic to reflect unsaved content, got {}",
        diagnostic_message
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/codeAction",
                "params": {
                    "textDocument": { "uri": package_uri.clone() },
                    "range": {
                        "start": { "line": 3, "character": 0 },
                        "end": { "line": 3, "character": 100 }
                    },
                    "context": { "diagnostics": diagnostics }
                }
            })
            .to_string(),
        );
    }

    let code_action_response = read_lsp_json_message_matching(&mut reader, |json| {
        json.get("id").and_then(|id| id.as_i64()) == Some(2)
    })
    .expect("code action response should exist");

    let actions = code_action_response["result"]
        .as_array()
        .expect("code action result should be an array");
    assert!(!actions.is_empty(), "expected at least one code action");

    let edit = &actions[0]["edit"]["changes"][package_uri.as_str()][0];
    assert_eq!(edit["newText"].as_str(), Some("\"20\""));

    let start = &edit["range"]["start"];
    let end = &edit["range"]["end"];
    assert_eq!(start["line"].as_u64(), Some(3));
    assert_eq!(end["line"].as_u64(), Some(3));
    assert_ne!(start["character"].as_u64(), Some(0));

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","id":3,"method":"shutdown"}"#);
    }

    let shutdown_response =
        read_lsp_message_matching(&mut reader, |message| message.contains(r#""id":3"#))
            .expect("shutdown response should exist");
    assert!(
        shutdown_response.contains(r#""id":3"#) && shutdown_response.contains(r#""result":null"#),
        "expected shutdown response, got {}",
        shutdown_response
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","method":"exit","params":null}"#);
    }
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .expect("lsp process should exit cleanly");

    assert!(
        output.status.success(),
        "lsp process exited with {:?}",
        output.status.code()
    );
}

#[cfg(feature = "lsp")]
#[test]
fn test_cli_lsp_rejects_documents_outside_workspace_root() {
    let workspace = TestWorkspace::new();
    workspace.write("Dockerfile", "FROM node:20-alpine\n");
    workspace.write("package.json", r#"{"engines":{"node":"20"}}"#);

    let outside_dir = tempfile::tempdir().unwrap();
    let outside_path = outside_dir.path().join("package.json");
    std::fs::write(&outside_path, r#"{"engines":{"node":"18"}}"#).unwrap();

    let root_uri = Url::from_file_path(workspace.root()).unwrap().to_string();
    let outside_uri = Url::from_file_path(&outside_path).unwrap().to_string();
    let outside_text = std::fs::read_to_string(&outside_path).unwrap();

    let mut child = ProcessCommand::new(env!("CARGO_BIN_EXE_conflic"))
        .arg("--lsp")
        .env("CONFLIC_LSP_SCAN_STATS", "1")
        .current_dir(workspace.root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("lsp process should spawn");

    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut reader = LspMessageReader::spawn(stdout);

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "processId": serde_json::Value::Null,
                    "rootUri": root_uri.clone(),
                    "capabilities": {}
                }
            })
            .to_string(),
        );
    }

    read_lsp_json_message_matching(&mut reader, |json| {
        json.get("id").and_then(|id| id.as_i64()) == Some(1)
    })
    .expect("initialize response should exist");

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
        );
    }

    read_lsp_json_message_matching(&mut reader, |json| {
        json.get("method").and_then(|method| method.as_str()) == Some("window/logMessage")
            && json["params"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("kind=full"))
    })
    .expect("initial full scan stats should be logged");

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": outside_uri.clone(),
                        "languageId": "json",
                        "version": 1,
                        "text": outside_text
                    }
                }
            })
            .to_string(),
        );
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/codeAction",
                "params": {
                    "textDocument": { "uri": outside_uri.clone() },
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 100 }
                    },
                    "context": { "diagnostics": [] }
                }
            })
            .to_string(),
        );
    }

    let mut rejection_logged = false;
    let mut outside_diagnostics_seen = false;
    let mut code_action_response = None;

    for _ in 0..48 {
        let Some(message) = read_lsp_message(&mut reader) else {
            break;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&message) else {
            continue;
        };

        match json.get("method").and_then(|method| method.as_str()) {
            Some("window/logMessage") => {
                let log_message = json["params"]["message"].as_str().unwrap_or_default();
                if log_message.contains("outside workspace root") {
                    rejection_logged = true;
                }
            }
            Some("textDocument/publishDiagnostics") => {
                if json["params"]["uri"].as_str() == Some(outside_uri.as_str()) {
                    outside_diagnostics_seen = true;
                }
            }
            _ => {}
        }

        if json.get("id").and_then(|id| id.as_i64()) == Some(2) {
            code_action_response = Some(json);
        }

        if rejection_logged && code_action_response.is_some() {
            break;
        }
    }

    assert!(
        rejection_logged,
        "outside-workspace documents should be rejected with a warning log"
    );
    assert!(
        !outside_diagnostics_seen,
        "outside-workspace documents must not receive diagnostics"
    );

    let code_action_response = code_action_response.expect("code action response should exist");
    assert!(
        code_action_response["result"].is_null(),
        "outside-workspace code actions should be rejected, got {}",
        code_action_response
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","id":3,"method":"shutdown"}"#);
    }

    read_lsp_json_message_matching(&mut reader, |json| {
        json.get("id").and_then(|id| id.as_i64()) == Some(3)
    })
    .expect("shutdown response should exist");

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","method":"exit","params":null}"#);
    }
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .expect("lsp process should exit cleanly");

    assert!(
        output.status.success(),
        "lsp process exited with {:?}",
        output.status.code()
    );
}

#[cfg(feature = "lsp")]
#[test]
fn test_cli_lsp_reloads_config_after_config_buffer_change() {
    let workspace = TestWorkspace::new();
    workspace.write(".nvmrc", "18\n");
    workspace.write("Dockerfile", "FROM node:20-alpine\n");

    let saved_config = "[conflic]\nseverity = \"warning\"\n";
    let updated_config = "[conflic]\nseverity = \"warning\"\nskip_concepts = [\"node-version\"]\n";
    workspace.write(".conflic.toml", saved_config);

    let root_uri = Url::from_file_path(workspace.root()).unwrap().to_string();
    let nvmrc_path = workspace.path(".nvmrc");
    let nvmrc_uri = Url::from_file_path(&nvmrc_path).unwrap().to_string();
    let config_uri = Url::from_file_path(workspace.path(".conflic.toml"))
        .unwrap()
        .to_string();

    let mut child = ProcessCommand::new(env!("CARGO_BIN_EXE_conflic"))
        .arg("--lsp")
        .env("CONFLIC_LSP_SCAN_STATS", "1")
        .current_dir(workspace.root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("lsp process should spawn");

    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut reader = LspMessageReader::spawn(stdout);

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "processId": serde_json::Value::Null,
                    "rootUri": root_uri.clone(),
                    "capabilities": {}
                }
            })
            .to_string(),
        );
    }

    read_lsp_json_message_matching(&mut reader, |json| {
        json.get("id").and_then(|id| id.as_i64()) == Some(1)
    })
    .expect("initialize response should exist");

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
        );
    }

    let mut initial_diagnostics = None;
    let mut initial_full_scan_logged = false;
    let mut observed_initial_messages = Vec::new();

    for _ in 0..64 {
        let Some(message) = read_lsp_message(&mut reader) else {
            break;
        };
        if observed_initial_messages.len() < 12 {
            observed_initial_messages.push(message.clone());
        }
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&message) else {
            continue;
        };

        match json.get("method").and_then(|method| method.as_str()) {
            Some("textDocument/publishDiagnostics")
                if json["params"]["uri"]
                    .as_str()
                    .is_some_and(|uri| lsp_uri_matches_path(uri, &nvmrc_path))
                    && json["params"]["diagnostics"]
                        .as_array()
                        .is_some_and(|diagnostics| !diagnostics.is_empty()) =>
            {
                initial_diagnostics = Some(json);
            }
            Some("window/logMessage")
                if json["params"]["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("kind=full")) =>
            {
                initial_full_scan_logged = true;
            }
            _ => {}
        }

        if initial_diagnostics.is_some() && initial_full_scan_logged {
            break;
        }
    }

    let initial_diagnostics = initial_diagnostics.unwrap_or_else(|| {
        panic!(
            "initial diagnostics for .nvmrc should exist; observed messages: {:?}",
            observed_initial_messages
        )
    });

    let initial_message = initial_diagnostics["params"]["diagnostics"][0]["message"]
        .as_str()
        .unwrap_or_default();
    assert!(
        initial_message.contains("20"),
        "expected initial diagnostics to reflect the Dockerfile conflict, got {}",
        initial_message
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": config_uri.clone(),
                        "languageId": "toml",
                        "version": 1,
                        "text": saved_config
                    }
                }
            })
            .to_string(),
        );
    }

    read_lsp_json_message_matching(&mut reader, |json| {
        json.get("method").and_then(|method| method.as_str()) == Some("window/logMessage")
            && json["params"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("kind=full"))
    })
    .expect("opening the config buffer should trigger a full scan");

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didChange",
                "params": {
                    "textDocument": {
                        "uri": config_uri.clone(),
                        "version": 2
                    },
                    "contentChanges": [
                        {
                            "text": updated_config
                        }
                    ]
                }
            })
            .to_string(),
        );
    }

    let mut config_reload_scan_logged = false;
    let mut cleared_diagnostics = None;
    let mut observed_config_change_messages = Vec::new();

    for _ in 0..64 {
        let Some(message) = read_lsp_message(&mut reader) else {
            break;
        };
        if observed_config_change_messages.len() < 12 {
            observed_config_change_messages.push(message.clone());
        }
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&message) else {
            continue;
        };

        if json.get("method").and_then(|method| method.as_str()) == Some("window/logMessage") {
            let log_message = json["params"]["message"].as_str().unwrap_or_default();
            assert!(
                !log_message.contains("Failed to reload conflic config")
                    && !log_message.contains("Failed to refresh conflic config"),
                "config reload should not fail: {}",
                log_message
            );
            if log_message.contains("kind=full") {
                config_reload_scan_logged = true;
            }
        }

        if json.get("method").and_then(|method| method.as_str())
            == Some("textDocument/publishDiagnostics")
            && json["params"]["uri"]
                .as_str()
                .is_some_and(|uri| lsp_uri_matches_path(uri, &nvmrc_path))
            && json["params"]["diagnostics"]
                .as_array()
                .is_some_and(|diagnostics| diagnostics.is_empty())
        {
            cleared_diagnostics = Some(json);
        }

        if config_reload_scan_logged && cleared_diagnostics.is_some() {
            break;
        }
    }

    assert!(
        config_reload_scan_logged,
        "changing the config buffer should trigger a full scan; observed messages: {:?}",
        observed_config_change_messages
    );

    let cleared_diagnostics = cleared_diagnostics.unwrap_or_else(|| {
        panic!(
            "config change should clear stale node-version diagnostics; observed messages: {:?}",
            observed_config_change_messages
        )
    });

    assert_eq!(
        cleared_diagnostics["params"]["diagnostics"]
            .as_array()
            .map(Vec::len),
        Some(0),
        "reloaded config should suppress node-version diagnostics"
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/codeAction",
                "params": {
                    "textDocument": { "uri": nvmrc_uri.clone() },
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 100 }
                    },
                    "context": { "diagnostics": [] }
                }
            })
            .to_string(),
        );
    }

    let code_action_response = read_lsp_json_message_matching(&mut reader, |json| {
        json.get("id").and_then(|id| id.as_i64()) == Some(2)
    })
    .expect("code action response should exist");
    assert!(
        code_action_response["result"].is_null(),
        "reloaded config should also clear cached code actions, got {}",
        code_action_response
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","id":3,"method":"shutdown"}"#);
    }

    read_lsp_json_message_matching(&mut reader, |json| {
        json.get("id").and_then(|id| id.as_i64()) == Some(3)
    })
    .expect("shutdown response should exist");

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","method":"exit","params":null}"#);
    }
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .expect("lsp process should exit cleanly");

    assert!(
        output.status.success(),
        "lsp process exited with {:?}",
        output.status.code()
    );
}

#[cfg(feature = "lsp")]
#[test]
fn test_cli_lsp_rapid_typing_uses_incremental_targeted_rescans() {
    let workspace = TestWorkspace::new();
    workspace.write("Dockerfile", "FROM node:20-alpine\n");

    let saved_package = "{\"engines\":{\"node\":\"20\"}}\n";
    workspace.write("package.json", saved_package);

    let root_uri = Url::from_file_path(workspace.root()).unwrap().to_string();
    let package_path = workspace.path("package.json");
    let package_uri = Url::from_file_path(&package_path).unwrap().to_string();
    let global_json_path = workspace.path("global.json");

    let mut child = ProcessCommand::new(env!("CARGO_BIN_EXE_conflic"))
        .arg("--lsp")
        .env("CONFLIC_LSP_SCAN_STATS", "1")
        .current_dir(workspace.root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("lsp process should spawn");

    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut reader = LspMessageReader::spawn(stdout);

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "processId": serde_json::Value::Null,
                    "rootUri": root_uri.clone(),
                    "capabilities": {}
                }
            })
            .to_string(),
        );
    }

    let initialize_response = read_lsp_json_message_matching(&mut reader, |json| {
        json.get("id").and_then(|id| id.as_i64()) == Some(1)
    })
    .expect("initialize response should exist");
    assert_eq!(
        initialize_response["result"]["capabilities"]["textDocumentSync"]["change"].as_u64(),
        Some(2),
        "server should advertise incremental text sync"
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
        );
    }

    let initial_full_scan = read_lsp_json_message_matching(&mut reader, |json| {
        json.get("method").and_then(|method| method.as_str()) == Some("window/logMessage")
            && json["params"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("kind=full"))
    })
    .expect("initial full scan stats should be logged");
    assert!(
        initial_full_scan["params"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("parsed_files=2")),
        "expected initial full scan to include the two discovered files, got {}",
        initial_full_scan
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": package_uri.clone(),
                        "languageId": "json",
                        "version": 1,
                        "text": saved_package
                    }
                }
            })
            .to_string(),
        );
    }

    read_lsp_json_message_matching(&mut reader, |json| {
        json.get("method").and_then(|method| method.as_str()) == Some("window/logMessage")
            && json["params"]["message"].as_str().is_some_and(|message| {
                message.contains("kind=incremental")
                    && message.contains("changed_files=1")
                    && message.contains("peer_files=1")
            })
    })
    .expect("didOpen should trigger one targeted incremental scan");

    workspace.write("global.json", "{ invalid json");

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        for (version, replacement) in [(2, "18"), (3, "17"), (4, "16")] {
            write_lsp_message(
                stdin,
                &serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/didChange",
                    "params": {
                        "textDocument": {
                            "uri": package_uri.clone(),
                            "version": version
                        },
                        "contentChanges": [
                            {
                                "range": {
                                    "start": { "line": 0, "character": 20 },
                                    "end": { "line": 0, "character": 22 }
                                },
                                "text": replacement
                            }
                        ]
                    }
                })
                .to_string(),
            );
        }
    }

    let mut package_diagnostics = None;
    let mut incremental_scan_logs = Vec::new();
    let mut observed_incremental_messages = Vec::new();

    for _ in 0..64 {
        let Some(message) = read_lsp_message(&mut reader) else {
            break;
        };
        if observed_incremental_messages.len() < 12 {
            observed_incremental_messages.push(message.clone());
        }
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&message) else {
            continue;
        };

        match json.get("method").and_then(|method| method.as_str()) {
            Some("textDocument/publishDiagnostics") => {
                let uri = json["params"]["uri"].as_str().unwrap_or_default();
                let diagnostics = json["params"]["diagnostics"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();

                assert!(
                    !lsp_uri_matches_path(uri, &global_json_path),
                    "rapid incremental edits should not trigger diagnostics for untouched global.json: {}",
                    json
                );

                if lsp_uri_matches_path(uri, &package_path) && !diagnostics.is_empty() {
                    let message = diagnostics[0]["message"].as_str().unwrap_or_default();
                    if message.contains("16") {
                        package_diagnostics = Some(json.clone());
                    }
                }
            }
            Some("window/logMessage") => {
                let message = json["params"]["message"].as_str().unwrap_or_default();
                if message.contains("kind=incremental") {
                    incremental_scan_logs.push(message.to_string());
                }
            }
            _ => {}
        }

        if package_diagnostics.is_some() && !incremental_scan_logs.is_empty() {
            break;
        }
    }

    let diagnostics_message = package_diagnostics.unwrap_or_else(|| {
        panic!(
            "latest rapid-typing diagnostics should be published; observed messages: {:?}",
            observed_incremental_messages
        )
    });
    let diagnostics = diagnostics_message["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics should be an array");
    let diagnostic_message = diagnostics[0]["message"]
        .as_str()
        .expect("diagnostic message should be a string");
    assert!(
        diagnostic_message.contains("16"),
        "expected diagnostics to reflect the final incremental edit, got {}",
        diagnostic_message
    );

    assert_eq!(
        incremental_scan_logs.len(),
        1,
        "rapid typing should coalesce into one debounced incremental scan, got {:?}",
        incremental_scan_logs
    );
    assert!(
        incremental_scan_logs[0].contains("parsed_files=2")
            && incremental_scan_logs[0].contains("changed_files=1")
            && incremental_scan_logs[0].contains("peer_files=1"),
        "incremental scan should rescan only the changed file plus its single concept peer, got {:?}",
        incremental_scan_logs
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#);
    }

    read_lsp_json_message_matching(&mut reader, |json| {
        json.get("id").and_then(|id| id.as_i64()) == Some(2)
    })
    .expect("shutdown response should exist");

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","method":"exit","params":null}"#);
    }
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .expect("lsp process should exit cleanly");

    assert!(
        output.status.success(),
        "lsp process exited with {:?}",
        output.status.code()
    );
}

#[cfg(feature = "lsp")]
#[test]
fn test_cli_lsp_smoke_initialize_and_exit() {
    let workspace = TestWorkspace::new();

    let mut child = ProcessCommand::new(env!("CARGO_BIN_EXE_conflic"))
        .arg("--lsp")
        .current_dir(workspace.root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("lsp process should spawn");

    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut reader = LspMessageReader::spawn(stdout);

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"rootUri":null,"capabilities":{}}}"#,
        );
    }

    let initialize_response =
        read_lsp_message_matching(&mut reader, |message| message.contains(r#""id":1"#))
            .expect("initialize response should exist");
    assert!(
        initialize_response.contains(r#""id":1"#) && initialize_response.contains(r#""result""#),
        "expected initialize response, got {}",
        initialize_response
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
        );
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#);
    }

    let shutdown_response =
        read_lsp_message_matching(&mut reader, |message| message.contains(r#""id":2"#))
            .expect("shutdown response should exist");
    assert!(
        shutdown_response.contains(r#""id":2"#) && shutdown_response.contains(r#""result":null"#),
        "expected shutdown response, got {}",
        shutdown_response
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","method":"exit","params":null}"#);
    }
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .expect("lsp process should exit cleanly");

    assert!(
        output.status.success(),
        "lsp process exited with {:?}",
        output.status.code()
    );
}
