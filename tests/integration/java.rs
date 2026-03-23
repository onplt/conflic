use crate::common::integration_helpers::*;
use assert_cmd::Command as AssertCommand;
use conflic::config::ConflicConfig;
use conflic::fix::plan_fixes;
use conflic::model::Severity;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_java_contradiction_finds_errors() {
    let path = fixture_path("java_contradiction");
    let result = conflic::scan(&path, &ConflicConfig::default()).unwrap();
    let java = concept_result(&result, "java-version");

    assert!(
        java.assertions.len() >= 3,
        "Should have assertions from pom.xml, Dockerfile, and .sdkmanrc"
    );
    assert!(
        !java.findings.is_empty(),
        "Should find java version contradictions (17 vs 21)"
    );
    assert!(result.has_findings_at_or_above(Severity::Error));
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
