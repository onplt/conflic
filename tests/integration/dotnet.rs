use crate::common::integration_helpers::*;
use conflic::config::ConflicConfig;
use conflic::model::Severity;

#[test]
fn test_dotnet_contradiction_finds_errors() {
    let path = fixture_path("dotnet_contradiction");
    let result = conflic::scan(&path, &ConflicConfig::default()).unwrap();
    let dotnet = concept_result(&result, "dotnet-version");

    assert!(
        dotnet.assertions.len() >= 3,
        "Should have assertions from csproj, global.json, and Dockerfile"
    );
    assert!(
        !dotnet.findings.is_empty(),
        "Should find .NET version contradictions"
    );
    assert!(result.has_findings_at_or_above(Severity::Error));
}
