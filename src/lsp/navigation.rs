use std::path::Path;

use tower_lsp::lsp_types::*;

use crate::model::{ConceptResult, ConfigAssertion, ScanResult, SourceSpan};

use super::diagnostics::make_range;

/// Find the assertion at the given cursor position within the scan result.
fn find_assertion_at_position<'a>(
    scan_result: &'a ScanResult,
    file_path: &Path,
    position: &Position,
) -> Option<&'a ConfigAssertion> {
    scan_result
        .concept_results
        .iter()
        .flat_map(|cr| &cr.assertions)
        .filter(|assertion| assertion.source.file == file_path)
        .find(|assertion| {
            span_contains_position(assertion.span.as_ref(), &assertion.source, position)
        })
}

/// Find the ConceptResult that contains an assertion at the given position.
fn find_concept_at_position<'a>(
    scan_result: &'a ScanResult,
    file_path: &Path,
    position: &Position,
) -> Option<&'a ConceptResult> {
    scan_result.concept_results.iter().find(|cr| {
        cr.assertions
            .iter()
            .filter(|assertion| assertion.source.file == file_path)
            .any(|assertion| {
                span_contains_position(assertion.span.as_ref(), &assertion.source, position)
            })
    })
}

/// Check whether a cursor position falls within an assertion's span.
fn span_contains_position(
    span: Option<&SourceSpan>,
    source: &crate::model::SourceLocation,
    position: &Position,
) -> bool {
    let line = position.line as usize;
    let character = position.character as usize;

    if let Some(span) = span {
        let start_line = span.line.saturating_sub(1);
        let start_col = span.column.saturating_sub(1);
        let end_line = span.end_line.saturating_sub(1);
        let end_col = span.end_column.saturating_sub(1);

        if line < start_line || line > end_line {
            return false;
        }
        if line == start_line && character < start_col {
            return false;
        }
        if line == end_line && character >= end_col {
            return false;
        }
        true
    } else {
        // No span — match the entire line
        let source_line = source.line.saturating_sub(1);
        line == source_line
    }
}

/// Build hover content for an assertion, showing its concept, authority, peer assertions,
/// and contradiction status.
pub(super) fn build_hover(
    scan_result: &ScanResult,
    file_path: &Path,
    position: &Position,
) -> Option<Hover> {
    let matched_assertion = find_assertion_at_position(scan_result, file_path, position)?;
    let concept_result = scan_result
        .concept_results
        .iter()
        .find(|cr| cr.concept.id == matched_assertion.concept.id)?;

    let has_contradictions = !concept_result.findings.is_empty();

    let mut lines = Vec::new();

    // Header
    lines.push(format!("**{}**", concept_result.concept.display_name));
    lines.push(String::new());

    // Current value
    lines.push(format!(
        "Value: `{}` ({})",
        matched_assertion.raw_value, matched_assertion.authority
    ));

    // Status
    if has_contradictions {
        lines.push(String::new());
        lines.push(format!(
            "Status: {} contradiction(s) detected",
            concept_result.findings.len()
        ));
    } else {
        lines.push(String::new());
        lines.push("Status: consistent".to_string());
    }

    // Peer assertions
    let peers: Vec<&ConfigAssertion> = concept_result
        .assertions
        .iter()
        .filter(|a| {
            a.source.file != file_path
                || !spans_equal(a.span.as_ref(), matched_assertion.span.as_ref())
        })
        .collect();

    if !peers.is_empty() {
        lines.push(String::new());
        lines.push("---".to_string());
        lines.push(String::new());
        lines.push("**Other declarations:**".to_string());
        for peer in &peers {
            let filename = peer
                .source
                .file
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            lines.push(format!(
                "- `{}` in {} ({})",
                peer.raw_value, filename, peer.authority
            ));
        }
    }

    let range = make_range(&matched_assertion.source, matched_assertion.span.as_ref());

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n"),
        }),
        range: Some(range),
    })
}

/// Build reference locations for the concept at the given position.
pub(super) fn build_references(
    scan_result: &ScanResult,
    file_path: &Path,
    position: &Position,
    include_declaration: bool,
) -> Vec<Location> {
    let Some(concept_result) = find_concept_at_position(scan_result, file_path, position) else {
        return Vec::new();
    };

    concept_result
        .assertions
        .iter()
        .filter(|assertion| {
            if include_declaration {
                true
            } else {
                // Exclude the declaration at the cursor
                assertion.source.file != file_path
                    || !span_contains_position(assertion.span.as_ref(), &assertion.source, position)
            }
        })
        .filter_map(|assertion| {
            let uri = Url::from_file_path(&assertion.source.file).ok()?;
            let range = make_range(&assertion.source, assertion.span.as_ref());
            Some(Location { uri, range })
        })
        .collect()
}

/// Build document symbols for all assertions in the given file.
pub(super) fn build_document_symbols(
    scan_result: &ScanResult,
    file_path: &Path,
) -> Vec<DocumentSymbol> {
    let mut concept_groups: Vec<(&ConceptResult, Vec<&ConfigAssertion>)> = Vec::new();

    for cr in &scan_result.concept_results {
        let file_assertions: Vec<&ConfigAssertion> = cr
            .assertions
            .iter()
            .filter(|a| a.source.file == file_path)
            .collect();

        if !file_assertions.is_empty() {
            concept_groups.push((cr, file_assertions));
        }
    }

    concept_groups.sort_by(|a, b| a.0.concept.display_name.cmp(&b.0.concept.display_name));

    concept_groups
        .into_iter()
        .map(|(cr, assertions)| {
            let children: Vec<DocumentSymbol> = assertions
                .iter()
                .map(|assertion| {
                    let range = make_range(&assertion.source, assertion.span.as_ref());
                    let detail = format!("{} ({})", assertion.raw_value, assertion.authority);

                    #[allow(deprecated)]
                    DocumentSymbol {
                        name: assertion.raw_value.clone(),
                        detail: Some(detail),
                        kind: SymbolKind::CONSTANT,
                        tags: None,
                        deprecated: None,
                        range,
                        selection_range: range,
                        children: None,
                    }
                })
                .collect();

            // Parent range spans all children
            let parent_range = children
                .iter()
                .fold(None::<Range>, |acc, child| {
                    Some(match acc {
                        None => child.range,
                        Some(r) => Range {
                            start: if child.range.start.line < r.start.line
                                || (child.range.start.line == r.start.line
                                    && child.range.start.character < r.start.character)
                            {
                                child.range.start
                            } else {
                                r.start
                            },
                            end: if child.range.end.line > r.end.line
                                || (child.range.end.line == r.end.line
                                    && child.range.end.character > r.end.character)
                            {
                                child.range.end
                            } else {
                                r.end
                            },
                        },
                    })
                })
                .unwrap_or_default();

            let has_contradictions = !cr.findings.is_empty();
            let status = if has_contradictions {
                format!("{} assertion(s), conflicts detected", children.len())
            } else {
                format!("{} assertion(s), consistent", children.len())
            };

            #[allow(deprecated)]
            DocumentSymbol {
                name: cr.concept.display_name.clone(),
                detail: Some(status),
                kind: SymbolKind::NAMESPACE,
                tags: None,
                deprecated: None,
                range: parent_range,
                selection_range: parent_range,
                children: Some(children),
            }
        })
        .collect()
}

/// Check whether two optional spans refer to the same location.
fn spans_equal(a: Option<&SourceSpan>, b: Option<&SourceSpan>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.start == b.start && a.end == b.end,
        (None, None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::semantic_type::SemanticType;
    use crate::model::{
        Authority, ConceptResult, ConfigAssertion, ScanResult, SemanticConcept, SourceLocation,
        SourceSpan,
    };

    fn make_assertion(
        file: &Path,
        raw_value: &str,
        line: usize,
        column: usize,
        end_column: usize,
        authority: Authority,
    ) -> ConfigAssertion {
        ConfigAssertion {
            concept: SemanticConcept::node_version(),
            value: SemanticType::StringValue(raw_value.to_string()),
            raw_value: raw_value.to_string(),
            source: SourceLocation {
                file: file.to_path_buf(),
                line,
                column,
                key_path: String::new(),
            },
            span: Some(SourceSpan {
                start: 0,
                end: 10,
                line,
                column,
                end_line: line,
                end_column,
            }),
            authority,
            extractor_id: "test".to_string(),
            is_matrix: false,
        }
    }

    fn make_scan_result(assertions: Vec<ConfigAssertion>) -> ScanResult {
        ScanResult {
            concept_results: vec![ConceptResult {
                concept: SemanticConcept::node_version(),
                assertions,
                findings: vec![],
            }],
            parse_diagnostics: vec![],
        }
    }

    #[test]
    fn test_hover_shows_concept_and_value() {
        let file = std::path::PathBuf::from("/project/package.json");
        let assertion = make_assertion(&file, "20.0.0", 5, 10, 17, Authority::Declared);
        let scan_result = make_scan_result(vec![assertion]);

        let position = Position {
            line: 4,
            character: 12,
        };
        let hover = build_hover(&scan_result, &file, &position);

        assert!(hover.is_some());
        let hover = hover.unwrap();
        if let HoverContents::Markup(markup) = &hover.contents {
            assert!(markup.value.contains("Node.js Version"));
            assert!(markup.value.contains("20.0.0"));
            assert!(markup.value.contains("declared"));
            assert!(markup.value.contains("consistent"));
        } else {
            panic!("expected markup content");
        }
    }

    #[test]
    fn test_hover_shows_peer_assertions() {
        let file1 = std::path::PathBuf::from("/project/package.json");
        let file2 = std::path::PathBuf::from("/project/.nvmrc");
        let a1 = make_assertion(&file1, "20.0.0", 5, 10, 17, Authority::Declared);
        let a2 = make_assertion(&file2, "18.0.0", 1, 1, 7, Authority::Advisory);
        let scan_result = make_scan_result(vec![a1, a2]);

        let position = Position {
            line: 4,
            character: 12,
        };
        let hover = build_hover(&scan_result, &file1, &position);

        assert!(hover.is_some());
        if let HoverContents::Markup(markup) = &hover.unwrap().contents {
            assert!(markup.value.contains("Other declarations"));
            assert!(markup.value.contains("18.0.0"));
            assert!(markup.value.contains(".nvmrc"));
        } else {
            panic!("expected markup content");
        }
    }

    #[test]
    fn test_hover_returns_none_outside_assertion() {
        let file = std::path::PathBuf::from("/project/package.json");
        let assertion = make_assertion(&file, "20.0.0", 5, 10, 17, Authority::Declared);
        let scan_result = make_scan_result(vec![assertion]);

        let position = Position {
            line: 0,
            character: 0,
        };
        assert!(build_hover(&scan_result, &file, &position).is_none());
    }

    #[test]
    fn test_references_returns_all_concept_locations() {
        let dir = tempfile::tempdir().unwrap();
        let file1 = dir.path().join("package.json");
        let file2 = dir.path().join(".nvmrc");
        let a1 = make_assertion(&file1, "20.0.0", 5, 10, 17, Authority::Declared);
        let a2 = make_assertion(&file2, "18.0.0", 1, 1, 7, Authority::Advisory);
        let scan_result = make_scan_result(vec![a1, a2]);

        let position = Position {
            line: 4,
            character: 12,
        };
        let refs = build_references(&scan_result, &file1, &position, true);

        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn test_references_excludes_declaration_when_requested() {
        let dir = tempfile::tempdir().unwrap();
        let file1 = dir.path().join("package.json");
        let file2 = dir.path().join(".nvmrc");
        let a1 = make_assertion(&file1, "20.0.0", 5, 10, 17, Authority::Declared);
        let a2 = make_assertion(&file2, "18.0.0", 1, 1, 7, Authority::Advisory);
        let scan_result = make_scan_result(vec![a1, a2]);

        let position = Position {
            line: 4,
            character: 12,
        };
        let refs = build_references(&scan_result, &file1, &position, false);

        assert_eq!(refs.len(), 1);
    }

    #[test]
    fn test_document_symbols_groups_by_concept() {
        let file = std::path::PathBuf::from("/project/package.json");
        let assertion = make_assertion(&file, "20.0.0", 5, 10, 17, Authority::Declared);
        let scan_result = make_scan_result(vec![assertion]);

        let symbols = build_document_symbols(&scan_result, &file);

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Node.js Version");
        assert!(symbols[0].children.is_some());
        let children = symbols[0].children.as_ref().unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "20.0.0");
    }

    #[test]
    fn test_document_symbols_empty_for_unrelated_file() {
        let file1 = std::path::PathBuf::from("/project/package.json");
        let file2 = std::path::PathBuf::from("/project/other.json");
        let assertion = make_assertion(&file1, "20.0.0", 5, 10, 17, Authority::Declared);
        let scan_result = make_scan_result(vec![assertion]);

        let symbols = build_document_symbols(&scan_result, &file2);
        assert!(symbols.is_empty());
    }
}
