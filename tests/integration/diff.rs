use crate::common::integration_helpers::*;
use assert_cmd::Command as AssertCommand;
use conflic::config::ConflicConfig;
use conflic::fix::plan_fixes;
use conflic::model::Severity;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_cli_diff_re_evaluates_when_conflic_config_changes() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    std::fs::write(repo.join(".nvmrc"), "18\n").unwrap();
    std::fs::write(repo.join("Dockerfile"), "FROM node:20-alpine\n").unwrap();
    std::fs::write(
        repo.join(".conflic.toml"),
        "[conflic]\nskip_concepts = [\"node-version\"]\n",
    )
    .unwrap();

    init_git_repo(repo);

    std::fs::write(
        repo.join(".conflic.toml"),
        "[conflic]\nseverity = \"warning\"\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_conflic"))
        .arg(repo)
        .arg("--diff")
        .arg("HEAD")
        .arg("--no-color")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        output.status.code(),
        Some(2),
        "config-only changes should trigger a full diff re-evaluation\nstdout: {}\nstderr: {}",
        stdout,
        stderr
    );
    assert!(
        stdout.contains("Node.js Version"),
        "expected the newly-unsuppressed node contradiction to be reported\nstdout: {}",
        stdout
    );
}

#[test]
fn test_cli_diff_re_evaluates_when_explicit_config_changes() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    std::fs::create_dir_all(repo.join("config")).unwrap();
    std::fs::write(repo.join("Dockerfile"), "FROM node:20-alpine\n").unwrap();
    std::fs::write(repo.join("package.json"), r#"{"engines":{"node":"18"}}"#).unwrap();
    std::fs::write(
        repo.join("config").join("custom.toml"),
        "[conflic]\nskip_concepts = [\"node-version\"]\n",
    )
    .unwrap();

    init_git_repo(repo);

    std::fs::write(
        repo.join("config").join("custom.toml"),
        "[conflic]\nskip_concepts = []\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_conflic"))
        .arg(repo)
        .arg("--config")
        .arg("config/custom.toml")
        .arg("--diff")
        .arg("HEAD")
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        output.status.code(),
        Some(1),
        "explicit config-only changes should trigger a full diff re-evaluation\nstdout: {}\nstderr: {}",
        stdout,
        stderr
    );
    assert!(
        stdout.contains("Node.js Version"),
        "expected the newly-unsuppressed node contradiction to be reported\nstdout: {}",
        stdout
    );
    assert!(
        stdout.contains("package.json") && stdout.contains("Dockerfile"),
        "expected diff scan to include both concept peers after an explicit config change\nstdout: {}",
        stdout
    );
}

#[test]
fn test_cli_diff_stdin_re_evaluates_when_external_explicit_config_changes() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("workspace");
    let config_dir = dir.path().join("external-config");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(repo.join("Dockerfile"), "FROM node:20-alpine\n").unwrap();
    std::fs::write(repo.join("package.json"), r#"{"engines":{"node":"18"}}"#).unwrap();
    let config_path = config_dir.join("outside.toml");
    std::fs::write(
        &config_path,
        "[conflic]\nskip_concepts = [\"node-version\"]\n",
    )
    .unwrap();

    let suppressed_config = ConflicConfig::load(&repo, Some(config_path.as_path())).unwrap();
    let suppressed = conflic::scan(&repo, &suppressed_config).unwrap();
    assert!(
        suppressed
            .concept_results
            .iter()
            .all(|result| result.concept.id != "node-version"),
        "precondition failed: the explicit external config should suppress node-version findings"
    );

    std::fs::write(&config_path, "[conflic]\nskip_concepts = []\n").unwrap();

    let assert = AssertCommand::cargo_bin("conflic")
        .unwrap()
        .arg(&repo)
        .arg("--config")
        .arg(&config_path)
        .arg("--diff-stdin")
        .arg("--format")
        .arg("json")
        .write_stdin(format!("{}\n", config_path.display()))
        .assert()
        .code(1);

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);

    assert!(
        stdout.contains("Node.js Version"),
        "external explicit config changes should trigger a full diff re-evaluation\nstdout: {}",
        stdout
    );
    assert!(
        stdout.contains("package.json") && stdout.contains("Dockerfile"),
        "expected diff scan to include both concept peers after an external config change\nstdout: {}",
        stdout
    );
}

#[test]
fn test_cli_diff_ignores_parse_diagnostics_from_untouched_files() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    std::fs::write(repo.join(".nvmrc"), "20\n").unwrap();
    std::fs::write(
        repo.join("package.json"),
        r#"{"engines":{"node":"20.0.0"}}"#,
    )
    .unwrap();
    std::fs::write(repo.join("global.json"), "{ invalid json").unwrap();

    init_git_repo(repo);

    std::fs::write(
        repo.join("package.json"),
        r#"{"engines":{"node":"18.0.0"}}"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_conflic"))
        .arg(repo)
        .arg("--diff")
        .arg("HEAD")
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let parse_diagnostics = parsed["parse_diagnostics"]
        .as_array()
        .expect("json output should include parse diagnostics array");

    assert_eq!(
        output.status.code(),
        Some(0),
        "untouched parse errors should not fail a diff scan\nstdout: {}",
        stdout
    );
    assert!(
        parse_diagnostics.is_empty(),
        "untouched files should not leak parse diagnostics into diff scans\nstdout: {}",
        stdout
    );
}
