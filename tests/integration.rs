use assert_cmd::Command as AssertCommand;
use conflic::config::ConflicConfig;
use conflic::fix::plan_fixes;
use conflic::model::Severity;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn run_git(repo: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "git {:?} failed\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn concept_result<'a>(
    result: &'a conflic::model::ScanResult,
    concept_id: &str,
) -> &'a conflic::model::ConceptResult {
    result
        .concept_results
        .iter()
        .find(|result| result.concept.id == concept_id)
        .unwrap_or_else(|| panic!("missing concept result for {}", concept_id))
}

#[test]
fn test_node_contradiction_finds_errors() {
    let path = fixture_path("node_contradiction");
    let config = ConflicConfig::default();
    let result = conflic::scan(&path, &config).unwrap();

    // Should find contradictions in node version
    let node_concept = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "node-version");

    assert!(node_concept.is_some(), "Should find node-version concept");
    let node = node_concept.unwrap();

    assert!(
        node.assertions.len() >= 2,
        "Should have at least 2 assertions for node-version, got {}",
        node.assertions.len()
    );

    assert!(
        !node.findings.is_empty(),
        "Should find contradictions in node version"
    );

    // Should have at least one error or warning
    assert!(
        result.has_findings_at_or_above(Severity::Warning),
        "Should have at least warning-level findings"
    );
}

#[test]
fn test_port_mismatch_finds_contradiction() {
    let path = fixture_path("port_mismatch");
    let config = ConflicConfig::default();
    let result = conflic::scan(&path, &config).unwrap();

    let port_concept = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "app-port");

    assert!(port_concept.is_some(), "Should find app-port concept");
    let port = port_concept.unwrap();

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
fn test_all_clean_no_contradictions() {
    let path = fixture_path("all_clean");
    let config = ConflicConfig::default();
    let result = conflic::scan(&path, &config).unwrap();

    let node_concept = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "node-version");

    assert!(node_concept.is_some(), "Should find node-version concept");
    let node = node_concept.unwrap();

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
fn test_ts_strict_conflict() {
    let path = fixture_path("ts_strict_conflict");
    let config = ConflicConfig::default();
    let result = conflic::scan(&path, &config).unwrap();

    let ts_concept = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "ts-strict-mode");

    assert!(ts_concept.is_some(), "Should find ts-strict-mode concept");
    let ts = ts_concept.unwrap();

    assert!(
        ts.assertions.len() >= 2,
        "Should have assertions from tsconfig and eslint"
    );

    assert!(
        !ts.findings.is_empty(),
        "Should find strict mode contradictions"
    );
}

#[test]
fn test_extensionless_eslintrc_is_parsed_for_strict_mode() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"strict":true}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join(".eslintrc"),
        r#"{"rules":{"@typescript-eslint/no-explicit-any":"off"}}"#,
    )
    .unwrap();

    let config = ConflicConfig::default();
    let result = conflic::scan(root, &config).unwrap();
    let ts = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "ts-strict-mode")
        .expect("ts-strict-mode concept should exist");

    assert!(
        ts.assertions
            .iter()
            .any(|assertion| assertion.extractor_id == "ts-strict-eslint"),
        "extensionless .eslintrc should still produce an ESLint strict-mode assertion"
    );
    assert!(
        !ts.findings.is_empty(),
        "extensionless .eslintrc should still participate in contradiction detection"
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

    let config = ConflicConfig::default();
    let result = conflic::scan(root, &config).unwrap();
    let ts = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "ts-strict-mode")
        .expect("ts-strict-mode concept should exist");

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
fn test_json_output_format() {
    let path = fixture_path("node_contradiction");
    let config = ConflicConfig::default();
    let result = conflic::scan(&path, &config).unwrap();

    let json_output = conflic::report::json::render(&result);
    let parsed: serde_json::Value = serde_json::from_str(&json_output).unwrap();

    assert!(parsed.get("version").is_some());
    assert!(parsed.get("concepts").is_some());
    assert!(parsed.get("summary").is_some());

    let summary = parsed.get("summary").unwrap();
    assert!(
        summary.get("errors").unwrap().as_u64().unwrap() > 0
            || summary.get("warnings").unwrap().as_u64().unwrap() > 0
    );
}

#[test]
fn test_terminal_output_contains_key_info() {
    let path = fixture_path("node_contradiction");
    let config = ConflicConfig::default();
    let result = conflic::scan(&path, &config).unwrap();

    let output = conflic::report::terminal::render(&result, true, false);

    assert!(
        output.contains("Node.js Version"),
        "Output should mention Node.js Version"
    );
    assert!(
        output.contains("ERROR") || output.contains("WARNING"),
        "Output should contain severity: {}",
        output
    );
}

#[test]
fn test_skip_concept_config() {
    let path = fixture_path("node_contradiction");
    let mut config = ConflicConfig::default();
    config.conflic.skip_concepts.push("node-version".into());

    let result = conflic::scan(&path, &config).unwrap();

    let node_concept = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "node-version");

    assert!(node_concept.is_none(), "node-version should be skipped");
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

    let config = ConflicConfig::default();
    let result = conflic::scan(workspace.path(), &config).unwrap();

    let node_concept = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "node-version")
        .expect("node-version concept should exist");

    let package_assertion = node_concept
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
fn test_ruby_contradiction_finds_errors() {
    let path = fixture_path("ruby_contradiction");
    let config = ConflicConfig::default();
    let result = conflic::scan(&path, &config).unwrap();

    let ruby_concept = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "ruby-version");

    assert!(ruby_concept.is_some(), "Should find ruby-version concept");
    let ruby = ruby_concept.unwrap();

    assert_eq!(
        ruby.assertions.len(),
        3,
        "Should have 3 ruby version assertions"
    );
    assert!(
        !ruby.findings.is_empty(),
        "Should find ruby version contradictions"
    );
    assert!(
        result.has_findings_at_or_above(Severity::Warning),
        "Should have at least warning-level findings"
    );
}

#[test]
fn test_java_contradiction_finds_errors() {
    let path = fixture_path("java_contradiction");
    let config = ConflicConfig::default();
    let result = conflic::scan(&path, &config).unwrap();

    let java_concept = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "java-version");

    assert!(java_concept.is_some(), "Should find java-version concept");
    let java = java_concept.unwrap();

    assert!(
        java.assertions.len() >= 3,
        "Should have assertions from pom.xml, Dockerfile, and .sdkmanrc"
    );
    assert!(
        !java.findings.is_empty(),
        "Should find java version contradictions (17 vs 21)"
    );
    assert!(
        result.has_findings_at_or_above(Severity::Error),
        "Should have error-level findings"
    );
}

#[test]
fn test_tsconfig_extends_inherits_strict() {
    let path = fixture_path("ts_extends");
    let config = ConflicConfig::default();
    let result = conflic::scan(&path, &config).unwrap();

    let ts_concept = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "ts-strict-mode");

    assert!(ts_concept.is_some(), "Should find ts-strict-mode concept");
    let ts = ts_concept.unwrap();

    // Should have 3 assertions: tsconfig.base (direct), tsconfig (inherited), eslint (off)
    assert_eq!(
        ts.assertions.len(),
        3,
        "Should have 3 assertions (base, inherited, eslint)"
    );

    // The inherited value should show as true
    let inherited = ts
        .assertions
        .iter()
        .find(|a| a.source.key_path.contains("inherited"));
    assert!(inherited.is_some(), "Should have an inherited assertion");

    // Should find contradictions (strict:true vs eslint no-explicit-any:off)
    assert!(!ts.findings.is_empty(), "Should find contradictions");
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

    let config = ConflicConfig::default();
    let result = conflic::scan(root, &config).unwrap();
    let strict_mode = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "ts-strict-mode")
        .expect("ts strict mode concept should be present");

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
fn test_dotnet_contradiction_finds_errors() {
    let path = fixture_path("dotnet_contradiction");
    let config = ConflicConfig::default();
    let result = conflic::scan(&path, &config).unwrap();

    let dotnet_concept = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "dotnet-version");

    assert!(
        dotnet_concept.is_some(),
        "Should find dotnet-version concept"
    );
    let dotnet = dotnet_concept.unwrap();

    assert!(
        dotnet.assertions.len() >= 3,
        "Should have assertions from csproj, global.json, and Dockerfile"
    );
    assert!(
        !dotnet.findings.is_empty(),
        "Should find .NET version contradictions"
    );
    assert!(
        result.has_findings_at_or_above(Severity::Error),
        "Should have error-level findings"
    );
}

#[test]
fn test_custom_extractor_detects_redis_contradiction() {
    let path = fixture_path("custom_redis");
    let config = ConflicConfig::load(&path, None).unwrap();

    // Should have 1 custom extractor
    assert_eq!(config.custom_extractor.len(), 1);
    assert_eq!(config.custom_extractor[0].concept, "redis-version");

    let result = conflic::scan(&path, &config).unwrap();

    let redis_concept = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "redis-version");

    assert!(
        redis_concept.is_some(),
        "Should find redis-version custom concept"
    );
    let redis = redis_concept.unwrap();

    assert_eq!(
        redis.assertions.len(),
        2,
        "Should have 2 redis version assertions (docker-compose + .env)"
    );
    let docker_compose_assertion = redis
        .assertions
        .iter()
        .find(|assertion| assertion.source.file.ends_with("docker-compose.yml"))
        .expect("docker-compose assertion should exist");
    assert_eq!(
        docker_compose_assertion.source.line, 4,
        "custom YAML assertions should point at the matched scalar value"
    );

    // 7.2 vs 7.0 should conflict
    assert!(
        !redis.findings.is_empty(),
        "Should find redis version contradiction (7.2 vs 7.0)"
    );
}

#[test]
fn test_custom_extractor_yaml_location_uses_full_key_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(
        root.join(".conflic.toml"),
        r#"[[custom_extractor]]
concept = "redis-version"
display_name = "Redis Version"
category = "runtime-version"
type = "version"

[[custom_extractor.source]]
file = "docker-compose.yml"
format = "yaml"
path = "services.redis.image"
pattern = "redis:(.*)"
authority = "enforced"

[[custom_extractor.source]]
file = ".env"
format = "env"
key = "REDIS_VERSION"
authority = "declared"
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("docker-compose.yml"),
        "services:\n  web:\n    image: redis:7.2\n  redis:\n    image: redis:7.2\n",
    )
    .unwrap();
    std::fs::write(root.join(".env"), "REDIS_VERSION=6.0\n").unwrap();

    let config = ConflicConfig::load(root, None).unwrap();
    let result = conflic::scan(root, &config).unwrap();
    let redis = concept_result(&result, "redis-version");
    let docker_compose_assertion = redis
        .assertions
        .iter()
        .find(|assertion| assertion.source.file.ends_with("docker-compose.yml"))
        .expect("docker-compose assertion should exist");

    assert_eq!(
        docker_compose_assertion.source.line, 5,
        "custom YAML assertions should follow the full key path instead of the first matching leaf"
    );
}

#[test]
fn test_custom_extractor_glob_source_runs_during_scan() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(
        root.join(".conflic.toml"),
        r#"[conflic]
severity = "warning"

[[custom_extractor]]
concept = "redis-version"
display_name = "Redis Version"
category = "runtime-version"
type = "version"

[[custom_extractor.source]]
file = "*.json"
format = "json"
path = "custom.redis"
authority = "declared"

[[custom_extractor.source]]
file = ".env"
format = "env"
key = "REDIS_VERSION"
authority = "enforced"
"#,
    )
    .unwrap();
    std::fs::create_dir_all(root.join("configs")).unwrap();
    std::fs::write(
        root.join("configs").join("service.json"),
        r#"{"custom":{"redis":"7.2"}}"#,
    )
    .unwrap();
    std::fs::write(root.join(".env"), "REDIS_VERSION=7.0\n").unwrap();

    let config = ConflicConfig::load(root, None).unwrap();
    let result = conflic::scan(root, &config).unwrap();

    let redis = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "redis-version")
        .expect("custom extractor concept should be scanned");

    assert_eq!(
        redis.assertions.len(),
        2,
        "glob and env sources should both run"
    );
    assert!(
        !redis.findings.is_empty(),
        "glob-backed custom source should participate in contradiction detection"
    );
}

#[test]
fn test_docker_compose_yaml_variant_is_discovered() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join(".env"), "PORT=3000\n").unwrap();
    std::fs::write(
        root.join("docker-compose.override.yaml"),
        r#"services:
  app:
    image: node:20-alpine
    ports:
      - "8080:8080"
"#,
    )
    .unwrap();

    let config = ConflicConfig::default();
    let result = conflic::scan(root, &config).unwrap();
    let port = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "app-port")
        .expect("app-port concept should be present");

    assert!(
        port.assertions
            .iter()
            .any(|assertion| assertion.extractor_id == "port-docker-compose"),
        "docker-compose YAML variants should be discovered for port extraction"
    );
    assert!(
        !port.findings.is_empty(),
        "docker-compose YAML variants should participate in contradiction detection"
    );
}

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
    let node = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "node-version")
        .expect("node-version concept should still exist for the Dockerfile assertion");

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
fn test_invalid_custom_extractor_patterns_surface_as_parse_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(
        root.join(".conflic.toml"),
        r#"[[custom_extractor]]
concept = "redis-version"
display_name = "Redis Version"
category = "runtime-version"
type = "version"

[[custom_extractor.source]]
file = "*.json"
format = "json"
path = "custom.redis"
pattern = "redis:("
authority = "declared"
"#,
    )
    .unwrap();
    std::fs::write(root.join("package.json"), r#"{"custom":{"redis":"7.2"}}"#).unwrap();

    let config = ConflicConfig::load(root, None).unwrap();
    let result = conflic::scan(root, &config).unwrap();

    assert!(
        result
            .parse_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.rule_id == "CONFIG001"),
        "expected invalid custom extractor patterns to surface as parse diagnostics, got {:?}",
        result.parse_diagnostics
    );
}

#[test]
fn test_invalid_custom_extractor_formats_surface_as_parse_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(
        root.join(".conflic.toml"),
        r#"[[custom_extractor]]
concept = "redis-version"
display_name = "Redis Version"
category = "runtime-version"
type = "version"

[[custom_extractor.source]]
file = "docker-compose.yml"
format = "yamll"
path = "services.redis.image"
pattern = "redis:(.*)"
authority = "declared"
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("docker-compose.yml"),
        "services:\n  redis:\n    image: redis:7.2\n",
    )
    .unwrap();

    let config = ConflicConfig::load(root, None).unwrap();
    let result = conflic::scan(root, &config).unwrap();

    assert!(
        result
            .parse_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.rule_id == "CONFIG001"
                && diagnostic.message.contains("invalid source format")),
        "expected invalid custom extractor formats to surface as parse diagnostics, got {:?}",
        result.parse_diagnostics
    );
}

#[test]
fn test_monorepo_package_roots_apply_to_absolute_scan_paths() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::create_dir_all(root.join("packages").join("a")).unwrap();
    std::fs::create_dir_all(root.join("packages").join("b")).unwrap();

    std::fs::write(root.join("packages").join("a").join(".nvmrc"), "20\n").unwrap();
    std::fs::write(
        root.join("packages").join("a").join("package.json"),
        r#"{"engines":{"node":"18.0.0"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("packages").join("b").join("package.json"),
        r#"{"engines":{"node":"22.0.0"}}"#,
    )
    .unwrap();

    let mut config = ConflicConfig::default();
    config.monorepo.per_package = true;
    config.monorepo.package_roots.push("packages/*".into());

    let result = conflic::scan(root, &config).unwrap();
    let node = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "node-version")
        .expect("node version concept should be present");

    assert_eq!(
        node.findings.len(),
        1,
        "only contradictions within the same package should be reported"
    );
}

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

#[test]
fn test_cli_diff_detects_dirty_worktree_changes() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    std::fs::write(repo.join(".nvmrc"), "20\n").unwrap();
    std::fs::write(
        repo.join("package.json"),
        r#"{"engines":{"node":"20.0.0"}}"#,
    )
    .unwrap();
    std::fs::write(
        repo.join("Dockerfile"),
        "FROM node:20-alpine\nWORKDIR /app\n",
    )
    .unwrap();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "codex@example.com"]);
    run_git(repo, &["config", "user.name", "Codex"]);
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "initial"]);

    std::fs::write(
        repo.join("package.json"),
        r#"{"engines":{"node":"18.0.0"}}"#,
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
        Some(1),
        "expected an error-level contradiction\nstdout: {}\nstderr: {}",
        stdout,
        stderr
    );
    assert!(
        stdout.contains("Node.js Version"),
        "expected diff scan output to include the contradiction\nstdout: {}",
        stdout
    );
    assert!(
        stdout.contains("18.0.0"),
        "expected changed file assertion to be part of the diff scan\nstdout: {}",
        stdout
    );
    assert!(
        !stderr.contains("No files changed since HEAD"),
        "diff mode incorrectly reported no changes\nstderr: {}",
        stderr
    );
}

#[test]
fn test_cli_diff_detects_untracked_new_files() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    std::fs::write(repo.join(".nvmrc"), "20\n").unwrap();
    std::fs::write(
        repo.join("Dockerfile"),
        "FROM node:20-alpine\nWORKDIR /app\n",
    )
    .unwrap();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "codex@example.com"]);
    run_git(repo, &["config", "user.name", "Codex"]);
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "initial"]);

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
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        output.status.code(),
        Some(1),
        "untracked files should still participate in diff scans\nstdout: {}\nstderr: {}",
        stdout,
        stderr
    );
    assert!(
        stdout.contains("\"Node.js Version\""),
        "expected diff scan output to include the contradiction\nstdout: {}",
        stdout
    );
    assert!(
        stdout.contains("\"18.0.0\""),
        "expected untracked file assertion to be part of the diff scan\nstdout: {}",
        stdout
    );
    assert!(
        !stderr.contains("No files changed since HEAD"),
        "diff mode incorrectly skipped untracked files\nstderr: {}",
        stderr
    );
}

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

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "codex@example.com"]);
    run_git(repo, &["config", "user.name", "Codex"]);
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "initial"]);

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

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "codex@example.com"]);
    run_git(repo, &["config", "user.name", "Codex"]);
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "initial"]);

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
fn test_docker_from_variants_with_registry_ports_and_flags_are_detected() {
    for dockerfile in [
        "FROM --platform=linux/amd64 node:20-alpine\n",
        "FROM localhost:5000/node:20-alpine\n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::write(root.join(".nvmrc"), "18\n").unwrap();
        std::fs::write(root.join("Dockerfile"), dockerfile).unwrap();

        let config = ConflicConfig::default();
        let result = conflic::scan(root, &config).unwrap();
        let node = result
            .concept_results
            .iter()
            .find(|cr| cr.concept.id == "node-version")
            .expect("node version concept should be present");

        assert!(
            node.assertions
                .iter()
                .any(|assertion| assertion.extractor_id == "node-version-dockerfile"),
            "docker FROM variant should still produce a dockerfile assertion for input: {}",
            dockerfile.trim()
        );
        assert!(
            !node.findings.is_empty(),
            "docker FROM variant should participate in contradiction detection for input: {}",
            dockerfile.trim()
        );
    }
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
fn test_json_output_includes_parse_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join("package.json"), "{ invalid json").unwrap();

    let config = ConflicConfig::default();
    let result = conflic::scan(root, &config).unwrap();
    let output = conflic::report::json::render(&result);
    let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

    let diagnostics = parsed
        .get("parse_diagnostics")
        .and_then(|v| v.as_array())
        .expect("json output should include parse diagnostics");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["rule_id"], "PARSE001");
    assert_eq!(diagnostics[0]["severity"], "error");
    assert!(
        parsed["summary"]["errors"].as_u64().unwrap() >= 1,
        "parse diagnostics should affect summary counts: {}",
        output
    );
}

#[test]
fn test_sarif_output_includes_parse_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join("package.json"), "{ invalid json").unwrap();

    let config = ConflicConfig::default();
    let result = conflic::scan(root, &config).unwrap();
    let output = conflic::report::sarif::render(&result);
    let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
    let results = parsed["runs"][0]["results"]
        .as_array()
        .expect("sarif output should include results");

    assert!(
        results.iter().any(|r| r["ruleId"] == "PARSE001"),
        "parse diagnostics should be emitted as SARIF results: {}",
        output
    );
}

#[test]
fn test_cli_parse_diagnostics_set_exit_code_and_json_output() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join("package.json"), "{ invalid json").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_conflic"))
        .arg(root)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        output.status.code(),
        Some(1),
        "parse diagnostics should produce an error exit code\nstdout: {}\nstderr: {}",
        stdout,
        stderr
    );
    assert!(
        stdout.contains("\"parse_diagnostics\""),
        "json output should include parse diagnostics\nstdout: {}",
        stdout
    );
    assert!(
        stdout.contains("\"PARSE001\""),
        "json output should expose the parse rule id\nstdout: {}",
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

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "codex@example.com"]);
    run_git(repo, &["config", "user.name", "Codex"]);
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "initial"]);

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

#[test]
fn test_sarif_output_uses_valid_file_uri_for_absolute_paths() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join(".nvmrc"), "20\n").unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"engines":{"node":"18.0.0"}}"#,
    )
    .unwrap();

    let config = ConflicConfig::default();
    let result = conflic::scan(root, &config).unwrap();
    let output = conflic::report::sarif::render(&result);
    let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
    let uri = parsed["runs"][0]["results"][0]["locations"][0]["physicalLocation"]
        ["artifactLocation"]["uri"]
        .as_str()
        .expect("sarif result should include an artifact uri");

    assert!(
        !uri.starts_with("//?/"),
        "sarif uri should not include the Windows extended-length prefix: {}",
        uri
    );
    assert!(
        uri.starts_with("file:///"),
        "absolute paths should be emitted as file URIs: {}",
        uri
    );
}

#[test]
fn test_prerelease_exact_version_conflicts_with_stable_range() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join(".nvmrc"), "20.0.0-rc.1\n").unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"engines":{"node":">=20.0.0"}}"#,
    )
    .unwrap();

    let config = ConflicConfig::default();
    let result = conflic::scan(root, &config).unwrap();
    let node = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "node-version")
        .expect("node version concept should be present");

    assert!(
        !node.findings.is_empty(),
        "prerelease exact versions should not satisfy stable ranges"
    );
}

#[test]
fn test_eslint_extends_array_inherits_local_strict_rules() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"strict":true}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join(".eslintrc.json"),
        r#"{"extends":["./eslint.base.json"]}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("eslint.base.json"),
        r#"{"rules":{"@typescript-eslint/no-explicit-any":"off"}}"#,
    )
    .unwrap();

    let config = ConflicConfig::default();
    let result = conflic::scan(root, &config).unwrap();
    let strict_mode = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "ts-strict-mode")
        .expect("ts strict mode concept should be present");

    assert!(
        strict_mode
            .assertions
            .iter()
            .any(|assertion| assertion.extractor_id == "ts-strict-eslint"),
        "eslint array extends should contribute inherited strict-mode assertions"
    );
    assert!(
        !strict_mode.findings.is_empty(),
        "inherited eslint rules should still conflict with tsconfig strict=true"
    );
}

#[test]
fn test_docker_compose_host_ip_port_mapping_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join(".env"), "PORT=3000\n").unwrap();
    std::fs::write(
        root.join("docker-compose.yml"),
        r#"services:
  app:
    image: node:20-alpine
    ports:
      - "127.0.0.1:8080:8080"
"#,
    )
    .unwrap();

    let config = ConflicConfig::default();
    let result = conflic::scan(root, &config).unwrap();
    let port = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "app-port")
        .expect("app-port concept should be present");

    assert!(
        port.assertions
            .iter()
            .any(|assertion| assertion.extractor_id == "port-docker-compose"),
        "compose host-ip mappings should still produce an enforced port assertion"
    );
    assert!(
        !port.findings.is_empty(),
        "compose host-ip mappings should participate in contradiction detection"
    );
}

#[test]
fn test_docker_compose_long_form_port_mapping_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join(".env"), "PORT=3000\n").unwrap();
    std::fs::write(
        root.join("docker-compose.yml"),
        r#"services:
  app:
    image: node:20-alpine
    ports:
      - target: 8080
        published: 8080
        protocol: tcp
"#,
    )
    .unwrap();

    let config = ConflicConfig::default();
    let result = conflic::scan(root, &config).unwrap();
    let port = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "app-port")
        .expect("app-port concept should be present");

    assert!(
        port.assertions
            .iter()
            .any(|assertion| assertion.extractor_id == "port-docker-compose"),
        "compose long-form mappings should produce an enforced port assertion"
    );
    assert!(
        !port.findings.is_empty(),
        "compose long-form mappings should participate in contradiction detection"
    );
}

#[test]
fn test_docker_compose_long_form_published_range_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join(".env"), "PORT=4000\n").unwrap();
    std::fs::write(
        root.join("docker-compose.yml"),
        r#"services:
  app:
    image: node:20-alpine
    ports:
      - target: 3000
        published: "8080-8082"
        protocol: tcp
"#,
    )
    .unwrap();

    let config = ConflicConfig::default();
    let result = conflic::scan(root, &config).unwrap();
    let port = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "app-port")
        .expect("app-port concept should be present");

    assert!(
        port.assertions
            .iter()
            .any(|assertion| assertion.extractor_id == "port-docker-compose"),
        "compose long-form published ranges should still produce an enforced port assertion"
    );
    assert!(
        !port.findings.is_empty(),
        "compose long-form published ranges should participate in contradiction detection"
    );
}

#[test]
fn test_docker_compose_short_range_mapping_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join("Dockerfile"), "FROM node:20\nEXPOSE 3000\n").unwrap();
    std::fs::write(
        root.join("docker-compose.yml"),
        r#"services:
  app:
    image: node:20-alpine
    ports:
      - "9090-9091:8080-8081"
"#,
    )
    .unwrap();

    let config = ConflicConfig::default();
    let result = conflic::scan(root, &config).unwrap();
    let port = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "app-port")
        .expect("app-port concept should be present");

    assert!(
        port.assertions
            .iter()
            .any(|assertion| assertion.extractor_id == "port-docker-compose"),
        "compose short range mappings should still produce an enforced port assertion"
    );
    assert!(
        !port.findings.is_empty(),
        "compose short range mappings should participate in contradiction detection"
    );
}

#[test]
fn test_monorepo_prefers_more_specific_package_roots() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::create_dir_all(root.join("apps").join("web").join("packages").join("a")).unwrap();
    std::fs::create_dir_all(root.join("apps").join("web").join("packages").join("b")).unwrap();
    std::fs::write(
        root.join(".conflic.toml"),
        r#"[monorepo]
per_package = true
package_roots = ["apps/*", "apps/*/packages/*"]
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("apps")
            .join("web")
            .join("packages")
            .join("a")
            .join(".nvmrc"),
        "18\n",
    )
    .unwrap();
    std::fs::write(
        root.join("apps")
            .join("web")
            .join("packages")
            .join("a")
            .join("package.json"),
        r#"{"engines":{"node":"18"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("apps")
            .join("web")
            .join("packages")
            .join("b")
            .join(".nvmrc"),
        "20\n",
    )
    .unwrap();
    std::fs::write(
        root.join("apps")
            .join("web")
            .join("packages")
            .join("b")
            .join("package.json"),
        r#"{"engines":{"node":"20"}}"#,
    )
    .unwrap();

    let config = ConflicConfig::load(root, None).unwrap();
    let result = conflic::scan(root, &config).unwrap();
    let node = result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == "node-version")
        .expect("node version concept should be present");

    assert!(
        node.findings.is_empty(),
        "the most specific package root should isolate nested packages"
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
fn test_yaml_eslintrc_extends_inherits_strict_rules() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(
        root.join("tsconfig.json"),
        "{\n  \"compilerOptions\": {\n    \"strict\": true\n  }\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".eslintrc.base.yml"),
        "rules:\n  '@typescript-eslint/no-explicit-any': off\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".eslintrc.yml"),
        "extends: ./.eslintrc.base.yml\n",
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let strict_mode = concept_result(&result, "ts-strict-mode");

    assert!(
        strict_mode
            .assertions
            .iter()
            .any(|assertion| assertion.source.key_path.contains("inherited")),
        "yaml extends should contribute an inherited ESLint assertion"
    );
    assert!(
        !strict_mode.findings.is_empty(),
        "yaml extends should still conflict with tsconfig strict=true"
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
fn test_bare_missing_tsconfig_extends_reports_parse_diagnostic() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(
        root.join("tsconfig.json"),
        "{ \"extends\": \"tsconfig.base\" }\n",
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();

    assert!(
        result
            .parse_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.rule_id == "PARSE002"),
        "missing bare local tsconfig extends should produce PARSE002: {:?}",
        result.parse_diagnostics
    );
}

#[test]
fn test_fix_plan_handles_dockerfile_platform_flags() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join("package.json"), r#"{"engines":{"node":"20"}}"#).unwrap();
    std::fs::write(
        root.join("Dockerfile"),
        "FROM --platform=linux/amd64 node:18-alpine AS build\nFROM nginx:1.27-alpine\n",
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let plan = plan_fixes(&result);

    assert!(
        plan.proposals.iter().any(|proposal| {
            proposal.file.ends_with("Dockerfile")
                && proposal
                    .proposed_raw
                    .starts_with("--platform=linux/amd64 node:20-alpine")
        }),
        "docker fix planning should preserve leading FROM flags: {:?}",
        plan.proposals
    );
}

#[test]
fn test_yaml_key_locations_ignore_comment_lines() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::create_dir_all(root.join(".github").join("workflows")).unwrap();
    std::fs::write(root.join(".nvmrc"), "20\n").unwrap();
    std::fs::write(
        root.join(".github").join("workflows").join("ci.yml"),
        "# node-version: 99\njobs:\n  build:\n    steps:\n      - uses: actions/setup-node@v4\n        with:\n          node-version: 18\n",
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let node = concept_result(&result, "node-version");
    let ci_assertion = node
        .assertions
        .iter()
        .find(|assertion| assertion.source.file.ends_with("ci.yml"))
        .expect("CI workflow assertion should exist");

    assert_eq!(
        ci_assertion.source.line, 7,
        "workflow key locations should ignore leading comment lines"
    );
}

#[test]
fn test_yaml_key_locations_ignore_freeform_mentions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::create_dir_all(root.join(".github").join("workflows")).unwrap();
    std::fs::write(root.join(".nvmrc"), "20\n").unwrap();
    std::fs::write(
        root.join(".github").join("workflows").join("ci.yml"),
        "name: ci\njobs:\n  build:\n    steps:\n      - run: echo node-version\n      - uses: actions/setup-node@v4\n        with:\n          node-version: 18\n",
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let node = concept_result(&result, "node-version");
    let ci_assertion = node
        .assertions
        .iter()
        .find(|assertion| assertion.source.file.ends_with("ci.yml"))
        .expect("CI workflow assertion should exist");

    assert_eq!(
        ci_assertion.source.line, 8,
        "workflow key locations should ignore freeform mentions before the actual key"
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

#[test]
fn test_java_generic_release_tag_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(
        root.join("pom.xml"),
        r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>demo</groupId>
  <artifactId>demo</artifactId>
  <version>1.0.0</version>
  <properties>
    <release>2024.03</release>
  </properties>
</project>"#,
    )
    .unwrap();
    std::fs::write(root.join("Dockerfile"), "FROM eclipse-temurin:17\n").unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let java = concept_result(&result, "java-version");

    assert!(
        java.findings.is_empty(),
        "generic release metadata must not create Java-version contradictions: {:?}",
        java.findings
    );
    assert_eq!(java.assertions.len(), 1);
    assert!(
        java.assertions
            .iter()
            .all(|assertion| assertion.source.key_path != "release"),
        "generic <release> tags should not be treated as Java runtime assertions: {:?}",
        java.assertions
    );
}

#[test]
fn test_java_maven_compiler_plugin_release_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(
        root.join("pom.xml"),
        r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>demo</groupId>
  <artifactId>demo</artifactId>
  <version>1.0.0</version>
  <build>
    <plugins>
      <plugin>
        <artifactId>maven-compiler-plugin</artifactId>
        <configuration>
          <release>17</release>
        </configuration>
      </plugin>
    </plugins>
  </build>
</project>"#,
    )
    .unwrap();
    std::fs::write(root.join("Dockerfile"), "FROM eclipse-temurin:21\n").unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let java = concept_result(&result, "java-version");

    assert!(
        java.assertions
            .iter()
            .any(|assertion| assertion.source.key_path == "maven-compiler-plugin.release"),
        "maven-compiler-plugin release should still participate in Java-version extraction: {:?}",
        java.assertions
    );
    assert!(
        !java.findings.is_empty(),
        "compiler plugin release should still be compared against Docker Java versions"
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

#[test]
fn test_non_config_circleci_yaml_is_not_scanned_as_ci_input() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::create_dir_all(root.join(".circleci")).unwrap();
    std::fs::write(root.join("package.json"), r#"{"engines":{"node":"20"}}"#).unwrap();
    std::fs::write(
        root.join(".circleci").join("notes.yml"),
        "meta:\n  node-version: 18\n",
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let node = concept_result(&result, "node-version");

    assert!(
        node.findings.is_empty(),
        "only .circleci/config.yml should participate in CI extraction: {:?}",
        node.findings
    );
    assert!(
        node.assertions
            .iter()
            .all(|assertion| !assertion.source.file.ends_with("notes.yml")),
        "non-config CircleCI YAML files must be ignored: {:?}",
        node.assertions
    );
}

#[test]
fn test_custom_extractor_exact_relative_paths_do_not_match_suffix_neighbors() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(
        root.join(".conflic.toml"),
        r#"[[custom_extractor]]
concept = "redis-version"
display_name = "Redis Version"
category = "runtime-version"
type = "version"

[[custom_extractor.source]]
file = "configs/package.json"
format = "json"
path = "custom.redis"
authority = "declared"

[[custom_extractor.source]]
file = ".env"
format = "env"
key = "REDIS_VERSION"
authority = "declared"
"#,
    )
    .unwrap();
    std::fs::create_dir_all(root.join("otherconfigs")).unwrap();
    std::fs::create_dir_all(root.join("nested").join("configs")).unwrap();
    std::fs::write(
        root.join("otherconfigs").join("package.json"),
        r#"{"custom":{"redis":"7.2"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("nested").join("configs").join("package.json"),
        r#"{"custom":{"redis":"7.2"}}"#,
    )
    .unwrap();
    std::fs::write(root.join(".env"), "REDIS_VERSION=6.0\n").unwrap();

    let config = ConflicConfig::load(root, None).unwrap();
    let result = conflic::scan(root, &config).unwrap();
    let redis = concept_result(&result, "redis-version");

    assert!(
        redis.findings.is_empty(),
        "root-relative custom source paths must not match suffix neighbors: {:?}",
        redis.findings
    );
    assert_eq!(redis.assertions.len(), 1);
    assert!(
        redis.assertions[0].source.file.ends_with(".env"),
        "only the explicit .env source should match: {:?}",
        redis.assertions
    );
}

#[test]
fn test_python_ci_unquoted_decimal_preserves_literal_and_avoids_false_positive() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::create_dir_all(root.join(".github").join("workflows")).unwrap();
    std::fs::write(root.join(".python-version"), "3.10\n").unwrap();
    std::fs::write(
        root.join(".github").join("workflows").join("ci.yml"),
        "jobs:\n  test:\n    steps:\n      - uses: actions/setup-python@v5\n        with:\n          python-version: 3.10\n",
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let python = concept_result(&result, "python-version");

    assert!(
        python.findings.is_empty(),
        "unquoted YAML decimals should not be rounded into false contradictions: {:?}",
        python.findings
    );
    assert!(
        python
            .assertions
            .iter()
            .any(|assertion| assertion.extractor_id == "python-version-ci"
                && assertion.raw_value == "3.10"),
        "CI assertions should preserve the original YAML scalar text: {:?}",
        python.assertions
    );
}

#[test]
fn test_python_ci_matrix_decimal_preserves_literal_and_avoids_false_positive() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::create_dir_all(root.join(".github").join("workflows")).unwrap();
    std::fs::write(root.join(".python-version"), "3.10\n").unwrap();
    std::fs::write(
        root.join(".github").join("workflows").join("ci.yml"),
        "jobs:\n  test:\n    strategy:\n      matrix:\n        python-version: [3.10]\n    steps:\n      - uses: actions/setup-python@v5\n        with:\n          python-version: ${{ matrix.python-version }}\n",
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let python = concept_result(&result, "python-version");

    assert!(
        python.findings.is_empty(),
        "matrix YAML decimals should not be rounded into false contradictions: {:?}",
        python.findings
    );
    assert!(
        python
            .assertions
            .iter()
            .any(|assertion| assertion.extractor_id == "python-version-ci"
                && assertion.source.key_path == "matrix.python-version"
                && assertion.raw_value == "3.10"),
        "matrix assertions should preserve the original YAML scalar text: {:?}",
        python.assertions
    );
}

#[test]
fn test_custom_yaml_decimal_preserves_literal_and_avoids_false_positive() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(
        root.join(".conflic.toml"),
        r#"[[custom_extractor]]
concept = "redis-version"
display_name = "Redis Version"
category = "runtime-version"
type = "version"

[[custom_extractor.source]]
file = "settings.yml"
format = "yaml"
path = "custom.redis"
authority = "enforced"

[[custom_extractor.source]]
file = ".env"
format = "env"
key = "REDIS_VERSION"
authority = "declared"
"#,
    )
    .unwrap();
    std::fs::write(root.join("settings.yml"), "custom:\n  redis: 7.20\n").unwrap();
    std::fs::write(root.join(".env"), "REDIS_VERSION=7.20\n").unwrap();

    let config = ConflicConfig::load(root, None).unwrap();
    let result = conflic::scan(root, &config).unwrap();
    let redis = concept_result(&result, "redis-version");

    assert!(
        redis.findings.is_empty(),
        "custom YAML scalars should preserve decimal precision: {:?}",
        redis.findings
    );
    assert!(
        redis
            .assertions
            .iter()
            .any(|assertion| assertion.source.file.ends_with("settings.yml")
                && assertion.raw_value == "7.20"),
        "custom YAML assertions should keep the original scalar text: {:?}",
        redis.assertions
    );
}
