use std::collections::HashMap;
use tower_lsp::lsp_types::*;

use crate::model::{Finding, ScanResult, Severity};

/// Convert scan results into LSP diagnostics grouped by file URI.
pub fn scan_result_to_diagnostics(result: &ScanResult) -> HashMap<Url, Vec<Diagnostic>> {
    let mut diagnostics: HashMap<Url, Vec<Diagnostic>> = HashMap::new();

    for cr in &result.concept_results {
        for finding in &cr.findings {
            add_finding_diagnostics(finding, &mut diagnostics);
        }
    }

    diagnostics
}

fn add_finding_diagnostics(finding: &Finding, diagnostics: &mut HashMap<Url, Vec<Diagnostic>>) {
    let severity = match finding.severity {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Info => DiagnosticSeverity::INFORMATION,
    };

    // Left side diagnostic
    if let Ok(left_uri) = Url::from_file_path(&finding.left.source.file) {
        let right_uri = Url::from_file_path(&finding.right.source.file).ok();

        let related = right_uri.map(|uri| {
            vec![DiagnosticRelatedInformation {
                location: Location {
                    uri,
                    range: make_range(finding.right.source.line),
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
            range: make_range(finding.left.source.line),
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

        diagnostics
            .entry(left_uri)
            .or_default()
            .push(diag);
    }

    // Right side diagnostic
    if let Ok(right_uri) = Url::from_file_path(&finding.right.source.file) {
        let left_uri = Url::from_file_path(&finding.left.source.file).ok();

        let related = left_uri.map(|uri| {
            vec![DiagnosticRelatedInformation {
                location: Location {
                    uri,
                    range: make_range(finding.left.source.line),
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
            range: make_range(finding.right.source.line),
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

        diagnostics
            .entry(right_uri)
            .or_default()
            .push(diag);
    }
}

fn make_range(line: usize) -> Range {
    let line = if line > 0 { line - 1 } else { 0 } as u32;
    Range {
        start: Position {
            line,
            character: 0,
        },
        end: Position {
            line,
            character: u32::MAX,
        },
    }
}
