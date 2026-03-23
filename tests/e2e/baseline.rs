use crate::common::e2e_helpers::*;
use crate::common::{TestWorkspace, conflic_cmd, conflic_cmd_in};
use predicates::prelude::*;
use std::path::Path;
use std::process::{Command as ProcessCommand, Stdio};
#[cfg(feature = "lsp")]
use tower_lsp::lsp_types::Url;

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
