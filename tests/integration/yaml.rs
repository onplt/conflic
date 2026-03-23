use crate::common::integration_helpers::*;
use conflic::config::ConflicConfig;

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

// ── Policy-as-Code integration tests ──────────────────────────────────────
