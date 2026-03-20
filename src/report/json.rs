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
                        file: a.source.file.to_string_lossy().to_string(),
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
                            file: f.left.source.file.to_string_lossy().to_string(),
                            line: f.left.source.line,
                            value: f.left.raw_value.clone(),
                        },
                        right: JsonFindingRef {
                            file: f.right.source.file.to_string_lossy().to_string(),
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
            summary: JsonSummary {
                concepts_checked: result.concept_results.len(),
                errors: result.error_count(),
                warnings: result.warning_count(),
                info: result.info_count(),
            },
        }
    }
}
