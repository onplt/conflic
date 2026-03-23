use crate::common::integration_helpers::*;
use conflic::config::ConflicConfig;
use std::process::Command;

#[test]
fn test_ts_strict_conflict() {
    let path = fixture_path("ts_strict_conflict");
    let result = conflic::scan(&path, &ConflicConfig::default()).unwrap();
    let ts = concept_result(&result, "ts-strict-mode");

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

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let ts = concept_result(&result, "ts-strict-mode");

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
fn test_tsconfig_extends_inherits_strict() {
    let path = fixture_path("ts_extends");
    let result = conflic::scan(&path, &ConflicConfig::default()).unwrap();
    let ts = concept_result(&result, "ts-strict-mode");

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
fn test_custom_extractor_detects_redis_contradiction() {
    let path = fixture_path("custom_redis");
    let config = ConflicConfig::load(&path, None).unwrap();

    // Should have 1 custom extractor
    assert_eq!(config.custom_extractor.len(), 1);
    assert_eq!(config.custom_extractor[0].concept, "redis-version");

    let result = conflic::scan(&path, &config).unwrap();
    let redis = concept_result(&result, "redis-version");

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
    let node = concept_result(&result, "node-version");

    assert_eq!(
        node.findings.len(),
        1,
        "only contradictions within the same package should be reported"
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

    init_git_repo(repo);

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

    init_git_repo(repo);

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
fn test_prerelease_exact_version_conflicts_with_stable_range() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join(".nvmrc"), "20.0.0-rc.1\n").unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"engines":{"node":">=20.0.0"}}"#,
    )
    .unwrap();

    let result = conflic::scan(root, &ConflicConfig::default()).unwrap();
    let node = concept_result(&result, "node-version");

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
    let strict_mode = concept_result(&result, "ts-strict-mode");

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
fn test_policy_coexists_with_contradiction_detection() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Two files with different node versions — contradiction
    std::fs::write(root.join(".nvmrc"), "18\n").unwrap();
    std::fs::write(root.join("package.json"), r#"{"engines": {"node": "20"}}"#).unwrap();

    // Policy requiring >= 22 — both files violate
    std::fs::write(
        root.join(".conflic.toml"),
        r#"
[[policy]]
id = "POL001"
concept = "node-version"
rule = ">= 22"
severity = "error"
"#,
    )
    .unwrap();

    let config = ConflicConfig::load(root, None).unwrap();
    let result = conflic::scan(root, &config).unwrap();

    let node = concept_result(&result, "node-version");

    // Should have both: contradiction finding (18 vs 20) + policy violations
    let contradiction_findings: Vec<_> = node
        .findings
        .iter()
        .filter(|f| f.rule_id.starts_with("VER"))
        .collect();
    let policy_findings: Vec<_> = node
        .findings
        .iter()
        .filter(|f| f.rule_id == "POL001")
        .collect();

    assert!(
        !contradiction_findings.is_empty(),
        "Should detect version contradiction between files"
    );
    assert!(
        !policy_findings.is_empty(),
        "Should detect policy violations"
    );
}
