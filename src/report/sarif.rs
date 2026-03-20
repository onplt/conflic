use crate::model::*;
use serde::Serialize;
use std::path::{Path, PathBuf};

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

fn path_to_uri(path: &Path) -> String {
    let sanitized = sanitize_path(path);

    if let Ok(cwd) = std::env::current_dir() {
        let sanitized_cwd = sanitize_path(&cwd);
        if let Ok(rel) = sanitized.strip_prefix(&sanitized_cwd) {
            return rel.to_string_lossy().replace('\\', "/");
        }
    }

    absolute_path_to_file_uri(&sanitized)
}

fn sanitize_path(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();

    if let Some(stripped) = raw.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{}", stripped));
    }

    if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        return PathBuf::from(stripped);
    }

    path.to_path_buf()
}

fn absolute_path_to_file_uri(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let encoded = encode_uri_component(&normalized);

    if normalized.starts_with("//") {
        format!("file:{}", encoded)
    } else if normalized.starts_with('/') {
        format!("file://{}", encoded)
    } else if normalized.as_bytes().get(1) == Some(&b':') {
        format!("file:///{}", encoded)
    } else {
        encoded
    }
}

fn encode_uri_component(value: &str) -> String {
    let mut encoded = String::new();

    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{:02X}", byte)),
        }
    }

    encoded
}

fn parse_rule_metadata(rule_id: &str) -> (&'static str, &'static str) {
    match rule_id {
        "PARSE001" => (
            "Configuration parse failure",
            "A configuration file could not be read or parsed.",
        ),
        "PARSE002" => (
            "Unsafe extends resolution",
            "A JSON extends reference could not be resolved safely inside the scan root.",
        ),
        "CONFIG001" => (
            "Invalid custom extractor configuration",
            "A custom extractor pattern could not be compiled or validated.",
        ),
        _ => (
            "Configuration diagnostic",
            "A configuration diagnostic was reported.",
        ),
    }
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

        for diagnostic in &result.parse_diagnostics {
            if seen_rules.insert(diagnostic.rule_id.clone()) {
                let (name, description) = parse_rule_metadata(&diagnostic.rule_id);
                rules.push(SarifRule {
                    id: diagnostic.rule_id.clone(),
                    name: name.into(),
                    short_description: SarifMessage {
                        text: description.into(),
                    },
                });
            }

            results.push(SarifResult {
                rule_id: diagnostic.rule_id.clone(),
                level: severity_to_sarif_level(diagnostic.severity),
                message: SarifMessage {
                    text: diagnostic.message.clone(),
                },
                locations: vec![SarifLocation {
                    physical_location: SarifPhysicalLocation {
                        artifact_location: SarifArtifactLocation {
                            uri: path_to_uri(&diagnostic.file),
                        },
                        region: SarifRegion {
                            start_line: 1,
                            start_column: 1,
                        },
                    },
                }],
                related_locations: vec![],
            });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_to_uri_strips_windows_extended_length_prefix() {
        let uri = path_to_uri(Path::new(r"\\?\C:\workspace\package.json"));
        assert_eq!(uri, "file:///C:/workspace/package.json");
    }
}
