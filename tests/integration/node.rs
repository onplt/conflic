use crate::common::integration_helpers::*;
use conflic::config::ConflicConfig;
use conflic::model::Severity;

#[test]
fn test_node_contradiction_finds_errors() {
    let path = fixture_path("node_contradiction");
    let result = conflic::scan(&path, &ConflicConfig::default()).unwrap();
    let node = concept_result(&result, "node-version");

    assert!(
        node.assertions.len() >= 2,
        "Should have at least 2 assertions for node-version, got {}",
        node.assertions.len()
    );
    assert!(
        !node.findings.is_empty(),
        "Should find contradictions in node version"
    );
    assert!(
        result.has_findings_at_or_above(Severity::Warning),
        "Should have at least warning-level findings"
    );
}

#[test]
fn test_policy_detects_outdated_node_version() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join(".nvmrc"), "18\n").unwrap();

    std::fs::write(
        root.join(".conflic.toml"),
        r#"
[[policy]]
id = "POL001"
concept = "node-version"
rule = ">= 20"
severity = "error"
message = "Node 18 is EOL. Upgrade to Node 20+."
"#,
    )
    .unwrap();

    let config = ConflicConfig::load(root, None).unwrap();
    let result = conflic::scan(root, &config).unwrap();

    let node = concept_result(&result, "node-version");

    assert_eq!(node.findings.len(), 1, "Should have 1 policy violation");
    assert_eq!(node.findings[0].rule_id, "POL001");
    assert_eq!(node.findings[0].severity, Severity::Error);
    assert!(
        node.findings[0].explanation.contains("Node 18 is EOL"),
        "Explanation should include policy message: {}",
        node.findings[0].explanation
    );
}
