use crate::common::integration_helpers::*;
use assert_cmd::Command as AssertCommand;
use conflic::config::ConflicConfig;
use std::process::Command;

#[test]
fn test_json_output_format() {
    let path = fixture_path("node_contradiction");
    let result = conflic::scan(&path, &ConflicConfig::default()).unwrap();
    let json_output = conflic::report::json::render(&result);
    let parsed: serde_json::Value = serde_json::from_str(&json_output).unwrap();

    assert!(parsed.get("version").is_some());
    assert!(parsed.get("concepts").is_some());
    assert!(parsed.get("summary").is_some());

    let summary = &parsed["summary"];
    assert!(summary["errors"].as_u64().unwrap() > 0 || summary["warnings"].as_u64().unwrap() > 0);
}

#[test]
fn test_terminal_output_contains_key_info() {
    let path = fixture_path("node_contradiction");
    let result = conflic::scan(&path, &ConflicConfig::default()).unwrap();
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
fn test_policy_json_output_includes_policy_findings() {
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
"#,
    )
    .unwrap();

    let output = AssertCommand::cargo_bin("conflic")
        .unwrap()
        .current_dir(root)
        .args(["--format", "json"])
        .output()
        .unwrap();

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("JSON output should be valid");

    let concepts = json["concepts"]
        .as_array()
        .expect("concepts should be array");
    let node = concepts
        .iter()
        .find(|c| c["id"] == "node-version")
        .expect("Should have node-version concept");

    let findings = node["findings"]
        .as_array()
        .expect("findings should be array");
    assert!(
        findings.iter().any(|f| f["rule_id"] == "POL001"),
        "JSON output should include policy finding POL001: {:?}",
        findings
    );
}
