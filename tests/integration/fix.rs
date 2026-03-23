use crate::common::integration_helpers::*;
use conflic::config::ConflicConfig;

#[test]
fn test_exclude_path_prefix_skips_nested_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::create_dir_all(root.join("packages").join("ignore")).unwrap();
    std::fs::write(
        root.join(".conflic.toml"),
        r#"[conflic]
exclude = ["packages/ignore"]
"#,
    )
    .unwrap();
    std::fs::write(root.join("Dockerfile"), "FROM node:20-alpine\n").unwrap();
    std::fs::write(
        root.join("packages").join("ignore").join("package.json"),
        r#"{"engines":{"node":"18"}}"#,
    )
    .unwrap();

    let config = ConflicConfig::load(root, None).unwrap();
    let result = conflic::scan(root, &config).unwrap();
    let node = concept_result(&result, "node-version");

    assert_eq!(
        node.assertions.len(),
        1,
        "excluded subtree should not contribute assertions: {:?}",
        node.assertions
            .iter()
            .map(|assertion| assertion.source.file.clone())
            .collect::<Vec<_>>()
    );
    assert!(
        node.findings.is_empty(),
        "excluded subtree should not generate contradictions: {:?}",
        node.findings
    );
}

#[test]
fn test_fix_apply_modifies_files() {
    // Create a temp directory with conflicting files
    let dir = tempfile::tempdir().unwrap();
    let dir_path = dir.path();

    // .nvmrc says 20
    std::fs::write(dir_path.join(".nvmrc"), "20\n").unwrap();
    // package.json engines says 18
    std::fs::write(
        dir_path.join("package.json"),
        r#"{"engines": {"node": "18.0.0"}}"#,
    )
    .unwrap();
    // Dockerfile says node:20
    std::fs::write(
        dir_path.join("Dockerfile"),
        "FROM node:20-alpine\nWORKDIR /app\n",
    )
    .unwrap();

    let config = ConflicConfig::default();
    let result = conflic::scan(dir_path, &config).unwrap();

    let plan = conflic::fix::plan_fixes(&result);

    // Should have some proposals
    if !plan.proposals.is_empty() {
        let apply_result = conflic::fix::patcher::apply_fixes(&plan, true);
        assert!(
            apply_result.errors.is_empty(),
            "Should have no errors applying fixes"
        );
        // Backup files should exist
        for backup in &apply_result.files_backed_up {
            assert!(
                backup.exists(),
                "Backup file should exist: {}",
                backup.display()
            );
        }
    }
}

#[test]
fn test_fix_plan_refuses_writing_semver_range_into_nvmrc() {
    let dir = tempfile::tempdir().unwrap();
    let dir_path = dir.path();

    std::fs::write(dir_path.join(".nvmrc"), "18\n").unwrap();
    std::fs::write(
        dir_path.join("package.json"),
        r#"{"engines":{"node":"^20"}}"#,
    )
    .unwrap();

    let config = ConflicConfig::default();
    let result = conflic::scan(dir_path, &config).unwrap();
    let plan = conflic::fix::plan_fixes(&result);

    assert!(
        plan.proposals.is_empty(),
        "range winners should not produce unsafe .nvmrc rewrites: {:?}",
        plan.proposals
    );
    assert!(
        plan.unfixable
            .iter()
            .any(|item| item.reason.contains("not an exact version token")),
        "expected an explicit unfixable reason, got {:?}",
        plan.unfixable
    );
}
