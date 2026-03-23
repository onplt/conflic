use crate::common::e2e_helpers::*;
use crate::common::{TestWorkspace, conflic_cmd_in};
use predicates::prelude::*;

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
