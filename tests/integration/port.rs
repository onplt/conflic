use crate::common::integration_helpers::*;
use assert_cmd::Command as AssertCommand;
use conflic::config::ConflicConfig;
use conflic::fix::plan_fixes;
use conflic::model::Severity;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_port_mismatch_finds_contradiction() {
    let path = fixture_path("port_mismatch");
    let result = conflic::scan(&path, &ConflicConfig::default()).unwrap();
    let port = concept_result(&result, "app-port");

    assert!(
        port.assertions.len() >= 2,
        "Should have at least 2 port assertions"
    );
    assert!(
        !port.findings.is_empty(),
        "Should find port contradictions (8080 vs 3000)"
    );
}

#[test]
fn test_flat_eslint_config_ignores_export_default_mentions_in_comments() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"strict":true}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("eslint.config.js"),
        r#"// mention export default before the real statement
export default [
  {
    rules: {
      '@typescript-eslint/no-explicit-any': 'off'
    }
  }
];
"#,
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let ts = concept_result(&result, "ts-strict-mode");

    assert!(
        ts.assertions
            .iter()
            .any(|assertion| assertion.extractor_id == "ts-strict-eslint"),
        "flat eslint config should still produce an ESLint strict-mode assertion"
    );
    assert!(
        !ts.findings.is_empty(),
        "flat eslint config should still participate in contradiction detection"
    );
    assert!(
        result
            .parse_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.file != root.join("eslint.config.js")),
        "leading comments should not cause eslint.config.js parse failures: {:?}",
        result.parse_diagnostics
    );
}

#[test]
fn test_policy_port_blacklist() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join(".env"), "PORT=80\n").unwrap();

    std::fs::write(
        root.join(".conflic.toml"),
        r#"
[[policy]]
id = "POL002"
concept = "app-port"
rule = "!= 80, != 443"
severity = "warning"
message = "Privileged ports require root."
"#,
    )
    .unwrap();

    let config = ConflicConfig::load(root, None).unwrap();
    let result = conflic::scan(root, &config).unwrap();

    let port = concept_result(&result, "app-port");

    assert_eq!(port.findings.len(), 1, "Port 80 should violate policy");
    assert_eq!(port.findings[0].rule_id, "POL002");
    assert_eq!(port.findings[0].severity, Severity::Warning);
}
