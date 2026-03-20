use crate::model::*;
use serde::Serialize;

pub fn render(result: &ScanResult) -> String {
    let sarif = SarifLog::from(result);
    serde_json::to_string_pretty(&sarif).unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e))
}

#[derive(Serialize)]
struct SarifLog {
    #[serde(rename = "$schema")]
    schema: String,
    version: String,
    runs: Vec<SarifRun>,
}

#[derive(Serialize)]
struct SarifRun {
    tool: SarifTool,
    results: Vec<SarifResult>,
}

#[derive(Serialize)]
struct SarifTool {
    driver: SarifDriver,
}

#[derive(Serialize)]
struct SarifDriver {
    name: String,
    #[serde(rename = "informationUri")]
    information_uri: String,
    version: String,
    rules: Vec<SarifRule>,
}

#[derive(Serialize)]
struct SarifRule {
    id: String,
    name: String,
    #[serde(rename = "shortDescription")]
    short_description: SarifMessage,
}

#[derive(Serialize)]
struct SarifResult {
    #[serde(rename = "ruleId")]
    rule_id: String,
    level: String,
    message: SarifMessage,
    locations: Vec<SarifLocation>,
    #[serde(rename = "relatedLocations")]
    related_locations: Vec<SarifLocation>,
}

#[derive(Serialize)]
struct SarifLocation {
    #[serde(rename = "physicalLocation")]
    physical_location: SarifPhysicalLocation,
}

#[derive(Serialize)]
struct SarifPhysicalLocation {
    #[serde(rename = "artifactLocation")]
    artifact_location: SarifArtifactLocation,
    region: SarifRegion,
}

#[derive(Serialize)]
struct SarifArtifactLocation {
    uri: String,
}

#[derive(Serialize)]
struct SarifRegion {
    #[serde(rename = "startLine")]
    start_line: usize,
    #[serde(rename = "startColumn")]
    start_column: usize,
}

#[derive(Serialize)]
struct SarifMessage {
    text: String,
}

fn severity_to_sarif_level(severity: Severity) -> String {
    match severity {
        Severity::Error => "error".into(),
        Severity::Warning => "warning".into(),
        Severity::Info => "note".into(),
    }
}

fn path_to_uri(path: &std::path::Path) -> String {
    // Convert to relative path if possible, use forward slashes
    let display = if let Ok(cwd) = std::env::current_dir() {
        if let Ok(rel) = path.strip_prefix(&cwd) {
            rel.to_string_lossy().to_string()
        } else {
            path.to_string_lossy().to_string()
        }
    } else {
        path.to_string_lossy().to_string()
    };
    display.replace('\\', "/")
}

impl From<&ScanResult> for SarifLog {
    fn from(result: &ScanResult) -> Self {
        let mut rules = Vec::new();
        let mut seen_rules = std::collections::HashSet::new();
        let mut results = Vec::new();

        for cr in &result.concept_results {
            for finding in &cr.findings {
                // Collect unique rules
                if seen_rules.insert(finding.rule_id.clone()) {
                    rules.push(SarifRule {
                        id: finding.rule_id.clone(),
                        name: format!("{} mismatch", cr.concept.display_name),
                        short_description: SarifMessage {
                            text: format!(
                                "Semantic contradiction detected in {}",
                                cr.concept.display_name
                            ),
                        },
                    });
                }

                // Primary location (left assertion)
                let primary = SarifLocation {
                    physical_location: SarifPhysicalLocation {
                        artifact_location: SarifArtifactLocation {
                            uri: path_to_uri(&finding.left.source.file),
                        },
                        region: SarifRegion {
                            start_line: finding.left.source.line.max(1),
                            start_column: finding.left.source.column.max(1),
                        },
                    },
                };

                // Related location (right assertion)
                let related = SarifLocation {
                    physical_location: SarifPhysicalLocation {
                        artifact_location: SarifArtifactLocation {
                            uri: path_to_uri(&finding.right.source.file),
                        },
                        region: SarifRegion {
                            start_line: finding.right.source.line.max(1),
                            start_column: finding.right.source.column.max(1),
                        },
                    },
                };

                results.push(SarifResult {
                    rule_id: finding.rule_id.clone(),
                    level: severity_to_sarif_level(finding.severity),
                    message: SarifMessage {
                        text: format!(
                            "{}: {} (in {}) vs {} (in {})",
                            finding.explanation,
                            finding.left.raw_value,
                            path_to_uri(&finding.left.source.file),
                            finding.right.raw_value,
                            path_to_uri(&finding.right.source.file),
                        ),
                    },
                    locations: vec![primary],
                    related_locations: vec![related],
                });
            }
        }

        SarifLog {
            schema: "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json".into(),
            version: "2.1.0".into(),
            runs: vec![SarifRun {
                tool: SarifTool {
                    driver: SarifDriver {
                        name: "conflic".into(),
                        information_uri: "https://github.com/conflic/conflic".into(),
                        version: env!("CARGO_PKG_VERSION").into(),
                        rules,
                    },
                },
                results,
            }],
        }
    }
}
