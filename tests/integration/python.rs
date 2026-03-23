use crate::common::integration_helpers::*;
use assert_cmd::Command as AssertCommand;
use conflic::config::ConflicConfig;
use conflic::fix::plan_fixes;
use conflic::model::Severity;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

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
