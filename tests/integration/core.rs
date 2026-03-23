use crate::common::integration_helpers::*;
use conflic::config::ConflicConfig;
use std::collections::HashMap;

#[test]
fn test_all_clean_no_contradictions() {
    let path = fixture_path("all_clean");
    let result = conflic::scan(&path, &ConflicConfig::default()).unwrap();
    let node = concept_result(&result, "node-version");

    assert!(
        node.findings.is_empty(),
        "Should have no contradictions when everything is consistent, but found: {:?}",
        node.findings
            .iter()
            .map(|f| &f.explanation)
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_skip_concept_config() {
    let path = fixture_path("node_contradiction");
    let mut config = ConflicConfig::default();
    config.conflic.skip_concepts.push("node-version".into());

    let result = conflic::scan(&path, &config).unwrap();
    assert!(
        find_concept(&result, "node-version").is_none(),
        "node-version should be skipped"
    );
}

#[test]
fn test_json_source_locations_use_exact_value_spans() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(
        workspace.path().join("package.json"),
        "{\n  // \"node\": \"99\"\n  \"name\": \"demo\",\n  \"engines\": {\n    \"node\": \"18\"\n  }\n}\n",
    )
    .unwrap();
    std::fs::write(workspace.path().join("Dockerfile"), "FROM node:20-alpine\n").unwrap();

    let result = conflic::scan(workspace.path(), &ConflicConfig::default()).unwrap();
    let node = concept_result(&result, "node-version");

    let package_assertion = node
        .assertions
        .iter()
        .find(|assertion| assertion.extractor_id == "node-version-package-json")
        .expect("package.json assertion should exist");

    assert_eq!(
        package_assertion.source.line, 5,
        "package.json diagnostics should point at the actual engines.node value"
    );
    assert_eq!(package_assertion.source.column, 13);
    assert!(
        package_assertion.span.is_some(),
        "package.json assertions should preserve exact source spans"
    );
}

#[test]
fn test_tsconfig_variant_is_discovered() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(
        root.join("tsconfig.app.json"),
        r#"{"compilerOptions":{"strict":true}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join(".eslintrc.json"),
        r#"{"rules":{"@typescript-eslint/no-explicit-any":"off"}}"#,
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let strict_mode = concept_result(&result, "ts-strict-mode");

    assert!(
        strict_mode
            .assertions
            .iter()
            .any(|assertion| assertion.extractor_id == "ts-strict-tsconfig"),
        "tsconfig.app.json should be discovered and extracted"
    );
    assert!(
        !strict_mode.findings.is_empty(),
        "tsconfig.app.json strict mode should still participate in contradiction detection"
    );
}

#[test]
fn test_scan_blocks_extends_path_outside_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("workspace");
    let external = dir.path().join("external");

    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&external).unwrap();

    std::fs::write(
        external.join("tsconfig.base.json"),
        r#"{"compilerOptions":{"strict":true}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"extends":"../external/tsconfig.base.json"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join(".eslintrc.json"),
        r#"{"rules":{"@typescript-eslint/no-explicit-any":"off"}}"#,
    )
    .unwrap();

    let config = ConflicConfig::default();
    let result = conflic::scan(&root, &config).unwrap();

    assert!(
        result
            .parse_diagnostics
            .iter()
            .any(|d| d.rule_id == "PARSE002" && d.message.contains("outside scan root")),
        "expected blocked extends diagnostic, got {:?}",
        result.parse_diagnostics
    );

    let ts_assertions: Vec<_> = result
        .concept_results
        .iter()
        .filter(|cr| cr.concept.id == "ts-strict-mode")
        .flat_map(|cr| cr.assertions.iter())
        .map(|a| a.raw_value.clone())
        .collect();
    assert!(
        !ts_assertions.iter().any(|value| value == "true"),
        "outside-root tsconfig should not contribute inherited assertions: {:?}",
        ts_assertions
    );
}

#[test]
fn test_eslint_flat_config_js_participates_in_strict_mode_comparison() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(
        root.join("tsconfig.json"),
        "{\n  \"compilerOptions\": {\n    \"strict\": true\n  }\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("eslint.config.js"),
        "export default [\n  {\n    rules: {\n      \"@typescript-eslint/no-explicit-any\": \"off\"\n    }\n  }\n];\n",
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let strict_mode = concept_result(&result, "ts-strict-mode");

    assert!(
        strict_mode
            .assertions
            .iter()
            .any(|assertion| assertion.extractor_id == "ts-strict-eslint"),
        "eslint.config.js should emit an ESLint strict-mode assertion"
    );
    assert!(
        !strict_mode.findings.is_empty(),
        "eslint.config.js should participate in contradiction detection"
    );
}

#[test]
fn test_csproj_targetframework_with_attributes_participates_in_comparison() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(
        root.join("MyApp.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    <TargetFramework Condition=\"'$(Configuration)' == 'Debug'\">net8.0</TargetFramework>\n  </PropertyGroup>\n</Project>\n",
    )
    .unwrap();
    std::fs::write(
        root.join("Dockerfile"),
        "FROM mcr.microsoft.com/dotnet/sdk:9.0\n",
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let dotnet = concept_result(&result, "dotnet-version");

    assert!(
        dotnet
            .assertions
            .iter()
            .any(|assertion| assertion.extractor_id == "dotnet-version-csproj"),
        "TargetFramework tags with attributes should still produce .csproj assertions"
    );
    assert!(
        !dotnet.findings.is_empty(),
        "TargetFramework tags with attributes should participate in contradiction detection"
    );
}

#[test]
fn test_scan_with_overrides_resolves_inherited_tsconfig_using_override_content() {
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

    let mut overrides = HashMap::new();
    overrides.insert(
        root.join("tsconfig.base.json"),
        "{\n  \"compilerOptions\": {\n    \"strict\": true\n  }\n}\n".to_string(),
    );

    let result = conflic::scan_with_overrides(root, &ConflicConfig::default(), &overrides).unwrap();
    let ts = concept_result(&result, "ts-strict-mode");

    assert!(
        ts.findings.is_empty(),
        "override-aware extends resolution should not report stale contradictions: {:?}",
        ts.findings
    );
    assert_eq!(ts.assertions.len(), 2);
    assert!(
        ts.assertions
            .iter()
            .all(|assertion| assertion.raw_value == "true"),
        "both the parent and inherited child assertion should reflect the override: {:?}",
        ts.assertions
    );
}

#[test]
fn test_nested_github_workflow_directories_are_not_scanned_as_ci_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::create_dir_all(root.join(".github").join("workflows").join("nested")).unwrap();
    std::fs::write(root.join(".nvmrc"), "20\n").unwrap();
    std::fs::write(
        root.join(".github")
            .join("workflows")
            .join("nested")
            .join("ci.yml"),
        "jobs:\n  build:\n    steps:\n      - uses: actions/setup-node@v4\n        with:\n          node-version: 18\n",
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let node = concept_result(&result, "node-version");

    assert!(
        node.findings.is_empty(),
        "nested workflow files should be ignored during CI extraction: {:?}",
        node.findings
    );
    assert_eq!(node.assertions.len(), 1);
    assert!(
        node.assertions
            .iter()
            .all(|assertion| !assertion.source.file.ends_with("ci.yml")),
        "only the on-disk .nvmrc assertion should remain: {:?}",
        node.assertions
    );
}
