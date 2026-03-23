use crate::common::e2e_helpers::*;
use crate::common::{TestWorkspace, conflic_cmd, conflic_cmd_in};
use predicates::prelude::*;
use std::path::Path;
use std::process::{Command as ProcessCommand, Stdio};
#[cfg(feature = "lsp")]
use tower_lsp::lsp_types::Url;

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
fn test_cli_diff_preserves_significant_whitespace_in_git_paths() {
    let workspace = TestWorkspace::new();
    workspace.write(
        ".conflic.toml",
        r#"[[custom_extractor]]
concept = "redis-version"
display_name = "Redis Version"
category = "runtime-version"
type = "version"

[[custom_extractor.source]]
file = " pkg/settings.json"
format = "json"
path = "redis"
authority = "enforced"

[[custom_extractor.source]]
file = ".env"
format = "env"
key = "REDIS_VERSION"
authority = "declared"
"#,
    );
    workspace.write(".env", "REDIS_VERSION=7.0\n");
    workspace.write(
        Path::new(" pkg").join("settings.json"),
        r#"{"redis":"7.0"}"#,
    );
    workspace.init_git_repo();
    workspace.git_add_and_commit("initial");

    workspace.write(
        Path::new(" pkg").join("settings.json"),
        r#"{"redis":"8.0"}"#,
    );

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--diff")
        .arg("HEAD")
        .arg("--format")
        .arg("json")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("redis-version"))
        .stdout(predicate::str::contains("\"value\": \"8.0\""))
        .stdout(predicate::str::contains("\"value\": \"7.0\""));
}

#[test]
fn test_cli_diff_stdin_preserves_significant_whitespace_in_paths() {
    let workspace = TestWorkspace::new();
    workspace.write(
        ".conflic.toml",
        r#"[[custom_extractor]]
concept = "redis-version"
display_name = "Redis Version"
category = "runtime-version"
type = "version"

[[custom_extractor.source]]
file = " pkg/settings.json"
format = "json"
path = "redis"
authority = "enforced"

[[custom_extractor.source]]
file = ".env"
format = "env"
key = "REDIS_VERSION"
authority = "declared"
"#,
    );
    workspace.write(".env", "REDIS_VERSION=7.0\n");
    workspace.write(
        Path::new(" pkg").join("settings.json"),
        r#"{"redis":"8.0"}"#,
    );

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--diff-stdin")
        .arg("--format")
        .arg("json")
        .write_stdin(" pkg/settings.json\n")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("redis-version"))
        .stdout(predicate::str::contains("\"value\": \"8.0\""))
        .stdout(predicate::str::contains("\"value\": \"7.0\""));
}

#[test]
fn test_cli_diff_rejects_option_like_git_ref() {
    let workspace = TestWorkspace::new();
    workspace.write(".nvmrc", "20\n");
    workspace.init_git_repo();
    workspace.git_add_and_commit("initial");

    let injected_output = workspace.path("owned-by-diff.txt");
    let diff_arg = format!("--diff=--output={}", injected_output.display());

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg(diff_arg)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("must not start with '-'"));

    assert!(
        !injected_output.exists(),
        "option-like diff refs must not create git-controlled output files"
    );
}
