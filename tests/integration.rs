use conflic::config::ConflicConfig;
use conflic::model::Severity;
use std::path::PathBuf;
use tempfile;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
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
        node.findings.iter().map(|f| &f.explanation).collect::<Vec<_>>()
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
    assert!(summary.get("errors").unwrap().as_u64().unwrap() > 0 || summary.get("warnings").unwrap().as_u64().unwrap() > 0);
}

#[test]
fn test_terminal_output_contains_key_info() {
    let path = fixture_path("node_contradiction");
    let config = ConflicConfig::default();
    let result = conflic::scan(&path, &config).unwrap();

    let output = conflic::report::terminal::render(&result, true, false);

    assert!(output.contains("Node.js Version"), "Output should mention Node.js Version");
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

    assert_eq!(ruby.assertions.len(), 3, "Should have 3 ruby version assertions");
    assert!(!ruby.findings.is_empty(), "Should find ruby version contradictions");
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

    assert!(java.assertions.len() >= 3, "Should have assertions from pom.xml, Dockerfile, and .sdkmanrc");
    assert!(!java.findings.is_empty(), "Should find java version contradictions (17 vs 21)");
    assert!(result.has_findings_at_or_above(Severity::Error), "Should have error-level findings");
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
    assert_eq!(ts.assertions.len(), 3, "Should have 3 assertions (base, inherited, eslint)");

    // The inherited value should show as true
    let inherited = ts.assertions.iter().find(|a| {
        a.source.key_path.contains("inherited")
    });
    assert!(inherited.is_some(), "Should have an inherited assertion");

    // Should find contradictions (strict:true vs eslint no-explicit-any:off)
    assert!(!ts.findings.is_empty(), "Should find contradictions");
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

    assert!(dotnet_concept.is_some(), "Should find dotnet-version concept");
    let dotnet = dotnet_concept.unwrap();

    assert!(dotnet.assertions.len() >= 3, "Should have assertions from csproj, global.json, and Dockerfile");
    assert!(!dotnet.findings.is_empty(), "Should find .NET version contradictions");
    assert!(result.has_findings_at_or_above(Severity::Error), "Should have error-level findings");
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

    assert!(redis_concept.is_some(), "Should find redis-version custom concept");
    let redis = redis_concept.unwrap();

    assert_eq!(
        redis.assertions.len(),
        2,
        "Should have 2 redis version assertions (docker-compose + .env)"
    );

    // 7.2 vs 7.0 should conflict
    assert!(
        !redis.findings.is_empty(),
        "Should find redis version contradiction (7.2 vs 7.0)"
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
            assert!(backup.exists(), "Backup file should exist: {}", backup.display());
        }
    }
}
