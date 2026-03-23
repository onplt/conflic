use crate::common::integration_helpers::*;
use conflic::config::ConflicConfig;
use conflic::model::Severity;
use std::collections::HashMap;

#[test]
fn test_incremental_workspace_ignores_paths_outside_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join("Dockerfile"), "FROM node:20-alpine\n").unwrap();
    std::fs::write(root.join("package.json"), r#"{"engines":{"node":"20"}}"#).unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    let outside_file = outside_dir.path().join("package.json");
    std::fs::write(&outside_file, r#"{"engines":{"node":"18"}}"#).unwrap();

    let config = ConflicConfig::default();
    let mut workspace = conflic::IncrementalWorkspace::new(root, &config);

    let initial = workspace.full_scan(&HashMap::new());
    assert!(
        !initial.has_findings_at_or_above(Severity::Warning),
        "clean workspace should not report findings before the outside path is introduced: {:?}",
        initial.concept_results
    );

    let result = workspace.scan_incremental(&[outside_file], &HashMap::new());
    let stats = workspace.last_stats();

    assert_eq!(
        stats.changed_files, 0,
        "outside-workspace files should be rejected before they reach incremental planning"
    );
    assert_eq!(
        stats.parsed_files, 0,
        "outside-workspace files should not be parsed during incremental scans"
    );
    assert!(
        !result.has_findings_at_or_above(Severity::Warning),
        "outside-workspace files must not affect incremental scan results: {:?}",
        result.concept_results
    );
}

#[test]
fn test_incremental_workspace_uses_override_content_for_extended_peers() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(
        root.join("tsconfig.base.json"),
        r#"{"compilerOptions":{"strict":false}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"extends":"./tsconfig.base.json"}"#,
    )
    .unwrap();

    let config = ConflicConfig::default();
    let mut workspace = conflic::IncrementalWorkspace::new(root, &config);
    let initial = workspace.full_scan(&HashMap::new());
    let initial_ts = concept_result(&initial, "ts-strict-mode");
    assert!(
        initial_ts
            .assertions
            .iter()
            .all(|assertion| assertion.raw_value == "false"),
        "expected the initial on-disk assertions to stay false: {:?}",
        initial_ts.assertions
    );

    let mut overrides = HashMap::new();
    overrides.insert(
        root.join("tsconfig.base.json"),
        "{\n  \"compilerOptions\": {\n    \"strict\": true\n  }\n}\n".to_string(),
    );

    let result = workspace.scan_incremental(&[root.join("tsconfig.base.json")], &overrides);
    let stats = workspace.last_stats();
    let ts = concept_result(&result, "ts-strict-mode");

    assert!(
        ts.findings.is_empty(),
        "incremental scans should re-resolve inherited peers against override content: {:?}",
        ts.findings
    );
    assert_eq!(stats.changed_files, 1);
    assert_eq!(stats.peer_files, 1);
    assert!(
        ts.assertions
            .iter()
            .all(|assertion| assertion.raw_value == "true"),
        "both assertions should reflect the override after the incremental rescan: {:?}",
        ts.assertions
    );
}
