use crate::common::integration_helpers::*;
use assert_cmd::Command as AssertCommand;
use conflic::config::ConflicConfig;
use conflic::fix::plan_fixes;
use conflic::model::Severity;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_ruby_contradiction_finds_errors() {
    let path = fixture_path("ruby_contradiction");
    let result = conflic::scan(&path, &ConflicConfig::default()).unwrap();
    let ruby = concept_result(&result, "ruby-version");

    assert_eq!(
        ruby.assertions.len(),
        3,
        "Should have 3 ruby version assertions"
    );
    assert!(
        !ruby.findings.is_empty(),
        "Should find ruby version contradictions"
    );
    assert!(result.has_findings_at_or_above(Severity::Warning));
}
