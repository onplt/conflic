use crate::model::*;
use serde::Serialize;

pub fn render(result: &ScanResult) -> String {
    let output = JsonOutput::from(result);
    serde_json::to_string_pretty(&output).unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e))
}

#[derive(Serialize)]
struct JsonOutput {
    version: String,
    concepts: Vec<JsonConcept>,
    parse_diagnostics: Vec<JsonParseDiagnostic>,
    summary: JsonSummary,
}

#[derive(Serialize)]
struct JsonConcept {
    id: String,
    display_name: String,
    assertions: Vec<JsonAssertion>,
    findings: Vec<JsonFinding>,
}

#[derive(Serialize)]
struct JsonAssertion {
    file: String,
    line: usize,
    key_path: String,
    raw_value: String,
    authority: String,
    extractor: String,
}

#[derive(Serialize)]
struct JsonFinding {
    severity: String,
    rule_id: String,
    left: JsonFindingRef,
    right: JsonFindingRef,
    explanation: String,
}

#[derive(Serialize)]
struct JsonFindingRef {
    file: String,
    line: usize,
    value: String,
}

#[derive(Serialize)]
struct JsonParseDiagnostic {
    severity: String,
    rule_id: String,
    file: String,
    message: String,
}

#[derive(Serialize)]
struct JsonSummary {
    concepts_checked: usize,
    errors: usize,
    warnings: usize,
    info: usize,
}

impl From<&ScanResult> for JsonOutput {
    fn from(result: &ScanResult) -> Self {
        let concepts: Vec<JsonConcept> = result
            .concept_results
            .iter()
            .map(|cr| JsonConcept {
                id: cr.concept.id.clone(),
                display_name: cr.concept.display_name.clone(),
                assertions: cr
                    .assertions
                    .iter()
                    .map(|a| JsonAssertion {
                        file: crate::pathing::strip_windows_extended_length_prefix(&a.source.file)
                            .to_string_lossy()
                            .to_string(),
                        line: a.source.line,
                        key_path: a.source.key_path.clone(),
                        raw_value: a.raw_value.clone(),
                        authority: a.authority.to_string(),
                        extractor: a.extractor_id.to_string(),
                    })
                    .collect(),
                findings: cr
                    .findings
                    .iter()
                    .map(|f| JsonFinding {
                        severity: f.severity.to_string().to_lowercase(),
                        rule_id: f.rule_id.clone(),
                        left: JsonFindingRef {
                            file: crate::pathing::strip_windows_extended_length_prefix(
                                &f.left.source.file,
                            )
                            .to_string_lossy()
                            .to_string(),
                            line: f.left.source.line,
                            value: f.left.raw_value.clone(),
                        },
                        right: JsonFindingRef {
                            file: crate::pathing::strip_windows_extended_length_prefix(
                                &f.right.source.file,
                            )
                            .to_string_lossy()
                            .to_string(),
                            line: f.right.source.line,
                            value: f.right.raw_value.clone(),
                        },
                        explanation: f.explanation.clone(),
                    })
                    .collect(),
            })
            .collect();

        JsonOutput {
            version: env!("CARGO_PKG_VERSION").to_string(),
            concepts,
            parse_diagnostics: result
                .parse_diagnostics
                .iter()
                .map(|d| JsonParseDiagnostic {
                    severity: d.severity.to_string().to_lowercase(),
                    rule_id: d.rule_id.clone(),
                    file: crate::pathing::strip_windows_extended_length_prefix(&d.file)
                        .to_string_lossy()
                        .to_string(),
                    message: d.message.clone(),
                })
                .collect(),
            summary: JsonSummary {
                concepts_checked: result.concept_results.len(),
                errors: result.error_count(),
                warnings: result.warning_count(),
                info: result.info_count(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::assertion::{Authority, ConfigAssertion, SourceLocation};
    use crate::model::concept::SemanticConcept;

    #[test]
    fn test_json_output_strips_windows_extended_length_prefix_from_file_paths() {
        let assertion = ConfigAssertion::new(
            SemanticConcept::node_version(),
            crate::model::SemanticType::Version(crate::model::parse_version("20")),
            "20".into(),
            SourceLocation {
                file: std::path::PathBuf::from(r"\\?\C:\workspace\.nvmrc"),
                line: 1,
                column: 0,
                key_path: String::new(),
            },
            Authority::Advisory,
            "node-version-nvmrc",
        );

        let result = ScanResult {
            concept_results: vec![ConceptResult {
                concept: SemanticConcept::node_version(),
                assertions: vec![assertion],
                findings: vec![],
            }],
            parse_diagnostics: vec![],
        };

        let output = render(&result);
        assert!(
            !output.contains(r"\\?\C:\workspace\.nvmrc"),
            "json output should strip the extended-length prefix: {}",
            output
        );
        assert!(
            output.contains(r"C:\\workspace\\.nvmrc"),
            "json output should keep the sanitized Windows path: {}",
            output
        );
    }
}
