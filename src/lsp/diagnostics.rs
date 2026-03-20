use std::collections::HashMap;
use tower_lsp::lsp_types::*;

use crate::model::{Finding, ParseDiagnostic, ScanResult, Severity, SourceLocation, SourceSpan};

/// Convert scan results into LSP diagnostics grouped by file URI.
pub fn scan_result_to_diagnostics(result: &ScanResult) -> HashMap<Url, Vec<Diagnostic>> {
    let mut diagnostics: HashMap<Url, Vec<Diagnostic>> = HashMap::new();

    for cr in &result.concept_results {
        for finding in &cr.findings {
            add_finding_diagnostics(finding, &mut diagnostics);
        }
    }

    for diagnostic in &result.parse_diagnostics {
        add_parse_diagnostic(diagnostic, &mut diagnostics);
    }

    diagnostics
}

fn severity_to_lsp(severity: Severity) -> DiagnosticSeverity {
    match severity {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Info => DiagnosticSeverity::INFORMATION,
    }
}

fn add_finding_diagnostics(finding: &Finding, diagnostics: &mut HashMap<Url, Vec<Diagnostic>>) {
    let severity = severity_to_lsp(finding.severity);

    // Left side diagnostic
    if let Ok(left_uri) = Url::from_file_path(&finding.left.source.file) {
        let right_uri = Url::from_file_path(&finding.right.source.file).ok();

        let related = right_uri.map(|uri| {
            vec![DiagnosticRelatedInformation {
                location: Location {
                    uri,
                    range: make_range(&finding.right.source, finding.right.span.as_ref()),
                },
                message: format!(
                    "{} = {} ({})",
                    finding.right.concept.display_name,
                    finding.right.raw_value,
                    finding.right.authority
                ),
            }]
        });

        let diag = Diagnostic {
            range: make_range(&finding.left.source, finding.left.span.as_ref()),
            severity: Some(severity),
            code: Some(NumberOrString::String(finding.rule_id.clone())),
            source: Some("conflic".to_string()),
            message: format!(
                "{}: {} conflicts with {} in {}",
                finding.left.concept.display_name,
                finding.left.raw_value,
                finding.right.raw_value,
                finding
                    .right
                    .source
                    .file
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            ),
            related_information: related,
            ..Default::default()
        };

        diagnostics.entry(left_uri).or_default().push(diag);
    }

    // Right side diagnostic
    if let Ok(right_uri) = Url::from_file_path(&finding.right.source.file) {
        let left_uri = Url::from_file_path(&finding.left.source.file).ok();

        let related = left_uri.map(|uri| {
            vec![DiagnosticRelatedInformation {
                location: Location {
                    uri,
                    range: make_range(&finding.left.source, finding.left.span.as_ref()),
                },
                message: format!(
                    "{} = {} ({})",
                    finding.left.concept.display_name,
                    finding.left.raw_value,
                    finding.left.authority
                ),
            }]
        });

        let diag = Diagnostic {
            range: make_range(&finding.right.source, finding.right.span.as_ref()),
            severity: Some(severity),
            code: Some(NumberOrString::String(finding.rule_id.clone())),
            source: Some("conflic".to_string()),
            message: format!(
                "{}: {} conflicts with {} in {}",
                finding.right.concept.display_name,
                finding.right.raw_value,
                finding.left.raw_value,
                finding
                    .left
                    .source
                    .file
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            ),
            related_information: related,
            ..Default::default()
        };

        diagnostics.entry(right_uri).or_default().push(diag);
    }
}

fn add_parse_diagnostic(
    parse_diagnostic: &ParseDiagnostic,
    diagnostics: &mut HashMap<Url, Vec<Diagnostic>>,
) {
    if let Ok(uri) = Url::from_file_path(&parse_diagnostic.file) {
        diagnostics.entry(uri).or_default().push(Diagnostic {
            range: make_range(
                &SourceLocation {
                    file: parse_diagnostic.file.clone(),
                    line: 1,
                    column: 0,
                    key_path: String::new(),
                },
                None,
            ),
            severity: Some(severity_to_lsp(parse_diagnostic.severity)),
            code: Some(NumberOrString::String(parse_diagnostic.rule_id.clone())),
            source: Some("conflic".to_string()),
            message: parse_diagnostic.message.clone(),
            ..Default::default()
        });
    }
}

fn make_range(source: &SourceLocation, span: Option<&SourceSpan>) -> Range {
    if let Some(span) = span {
        return Range {
            start: Position {
                line: span.line.saturating_sub(1) as u32,
                character: span.column.saturating_sub(1) as u32,
            },
            end: Position {
                line: span.end_line.saturating_sub(1) as u32,
                character: span.end_column.saturating_sub(1) as u32,
            },
        };
    }

    let line = source.line.saturating_sub(1) as u32;
    Range {
        start: Position { line, character: 0 },
        end: Position {
            line,
            character: u32::MAX,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ParseDiagnostic, ScanResult, Severity};

    #[test]
    fn test_scan_result_includes_parse_diagnostics() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("package.json");
        std::fs::write(&file, "{ invalid json").unwrap();

        let result = ScanResult {
            concept_results: vec![],
            parse_diagnostics: vec![ParseDiagnostic {
                severity: Severity::Error,
                file: file.clone(),
                message: "Failed to parse JSON".into(),
                rule_id: "PARSE001".into(),
            }],
        };

        let diagnostics = scan_result_to_diagnostics(&result);
        let uri = Url::from_file_path(file).unwrap();
        let file_diagnostics = diagnostics.get(&uri).expect("diagnostic should be present");

        assert_eq!(file_diagnostics.len(), 1);
        assert_eq!(
            file_diagnostics[0].code,
            Some(NumberOrString::String("PARSE001".into()))
        );
        assert_eq!(
            file_diagnostics[0].severity,
            Some(DiagnosticSeverity::ERROR)
        );
        assert_eq!(file_diagnostics[0].range.start.line, 0);
    }
}
