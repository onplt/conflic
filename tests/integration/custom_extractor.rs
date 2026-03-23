use crate::common::integration_helpers::*;
use conflic::config::ConflicConfig;

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
    let redis = concept_result(&result, "redis-version");

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
