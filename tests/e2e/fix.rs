use crate::common::e2e_helpers::*;
use crate::common::{TestWorkspace, conflic_cmd, conflic_cmd_in};
use predicates::prelude::*;

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
fn test_cli_fix_updates_multiline_docker_from_without_leaving_continuation_line_behind() {
    let workspace = TestWorkspace::new();
    workspace.write("package.json", r#"{"engines":{"node":"20"}}"#);
    workspace.write(
        "Dockerfile",
        "FROM --platform=linux/amd64 \\\n  node:18-alpine AS build\nRUN npm ci\nFROM node:20-alpine\n",
    );

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--fix")
        .arg("--yes")
        .arg("--no-backup")
        .arg("--no-color")
        .assert()
        .success();

    let dockerfile = workspace.read("Dockerfile");
    assert_eq!(
        dockerfile,
        "FROM --platform=linux/amd64 node:20-alpine AS build\nRUN npm ci\nFROM node:20-alpine\n"
    );
}

#[test]
fn test_cli_fix_preserves_docker_image_digest() {
    let workspace = TestWorkspace::new();
    workspace.write("package.json", r#"{"engines":{"node":"20"}}"#);
    workspace.write(
        "Dockerfile",
        "FROM node:18-alpine@sha256:deadbeef AS build\nRUN npm ci\nFROM nginx:1.27\n",
    );

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--fix")
        .arg("--yes")
        .arg("--no-backup")
        .arg("--no-color")
        .assert()
        .success();

    let dockerfile = workspace.read("Dockerfile");
    assert_eq!(
        dockerfile,
        "FROM node:20-alpine@sha256:deadbeef AS build\nRUN npm ci\nFROM nginx:1.27\n"
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
fn test_cli_fix_preserves_env_quotes_and_inline_comments() {
    let workspace = TestWorkspace::new();
    workspace.write(
        ".env",
        "export PORT = \"3000\"  # keep comment\nNAME=demo\n",
    );
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
        .success();

    let env_text = workspace.read(".env");
    assert!(
        env_text.contains("export PORT = \"8080\"  # keep comment"),
        "expected env fix to preserve quotes and inline comments, got:\n{}",
        env_text
    );
}

#[test]
fn test_cli_fix_refuses_ambiguous_multi_token_docker_expose_line() {
    let workspace = TestWorkspace::new();
    workspace.write("Dockerfile", "EXPOSE 3000 5000\n");
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
        .stdout(predicate::str::contains(
            "Unfixable (manual resolution needed):",
        ))
        .stdout(predicate::str::contains(
            "Dockerfile EXPOSE line contains multiple port tokens",
        ));

    assert_eq!(
        workspace.read("Dockerfile"),
        "EXPOSE 3000 5000\n",
        "ambiguous EXPOSE lines must be left unchanged"
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
