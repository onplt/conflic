use crate::common::integration_helpers::*;
use assert_cmd::Command as AssertCommand;
use conflic::config::ConflicConfig;
use conflic::fix::plan_fixes;
use conflic::model::Severity;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

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
    let node = concept_result(&result, "node-version");

    assert!(
        node.findings.is_empty(),
        "the most specific package root should isolate nested packages"
    );
}
