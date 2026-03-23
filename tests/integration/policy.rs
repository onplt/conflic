use crate::common::integration_helpers::*;
use assert_cmd::Command as AssertCommand;
use conflic::config::ConflicConfig;
use conflic::fix::plan_fixes;
use conflic::model::Severity;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_policy_passes_for_compliant_version() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join(".nvmrc"), "22\n").unwrap();

    std::fs::write(
        root.join(".conflic.toml"),
        r#"
[[policy]]
id = "POL001"
concept = "node-version"
rule = ">= 20"
severity = "error"
"#,
    )
    .unwrap();

    let config = ConflicConfig::load(root, None).unwrap();
    let result = conflic::scan(root, &config).unwrap();

    let node = concept_result(&result, "node-version");
    assert!(
        node.findings.is_empty(),
        "Node 22 satisfies >= 20, no policy violations expected"
    );
}

#[test]
fn test_policy_version_blacklist() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join(".python-version"), "3.8.19\n").unwrap();

    std::fs::write(
        root.join(".conflic.toml"),
        r#"
[[policy]]
id = "POL003"
concept = "python-version"
rule = "!= 3.8, != 3.9"
severity = "error"
message = "Python 3.8 and 3.9 are EOL."
"#,
    )
    .unwrap();

    let config = ConflicConfig::load(root, None).unwrap();
    let result = conflic::scan(root, &config).unwrap();

    let python = concept_result(&result, "python-version");

    assert_eq!(
        python.findings.len(),
        1,
        "Python 3.8.19 should match blacklisted 3.8"
    );
    assert_eq!(python.findings[0].rule_id, "POL003");
}
