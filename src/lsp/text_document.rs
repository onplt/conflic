use tower_lsp::lsp_types::{Position, Range, TextDocumentContentChangeEvent};

pub(super) fn apply_content_changes(
    document: &mut String,
    content_changes: Vec<TextDocumentContentChangeEvent>,
) {
    for change in content_changes {
        match change.range {
            Some(range) => {
                if let Some((start, end)) = range_to_offsets(document, range) {
                    document.replace_range(start..end, &change.text);
                } else {
                    *document = change.text;
                }
            }
            None => *document = change.text,
        }
    }
}

fn range_to_offsets(document: &str, range: Range) -> Option<(usize, usize)> {
    let start = position_to_offset(document, range.start)?;
    let end = position_to_offset(document, range.end)?;
    (start <= end).then_some((start, end))
}

fn position_to_offset(document: &str, position: Position) -> Option<usize> {
    if position.line == 0 && position.character == 0 {
        return Some(0);
    }

    let mut current_line = 0_u32;
    let mut current_character = 0_u32;

    for (byte_index, character) in document.char_indices() {
        if current_line == position.line && current_character == position.character {
            return Some(byte_index);
        }

        if character == '\n' {
            current_line += 1;
            current_character = 0;
            continue;
        }

        if current_line == position.line {
            current_character += character.len_utf16() as u32;
        }
    }

    if current_line == position.line && current_character == position.character {
        Some(document.len())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_content_changes_supports_incremental_ranges() {
        let mut document = String::from("{\n  \"name\": \"demo\",\n  \"version\": \"1.0.0\"\n}\n");
        apply_content_changes(
            &mut document,
            vec![TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: 2,
                        character: 13,
                    },
                    end: Position {
                        line: 2,
                        character: 20,
                    },
                }),
                range_length: None,
                text: "\"2.0.0\"".to_string(),
            }],
        );

        assert!(document.contains("\"version\": \"2.0.0\""));
    }

    #[test]
    fn test_position_to_offset_handles_line_boundaries() {
        let document = "first\nsecond\n";
        assert_eq!(
            position_to_offset(
                document,
                Position {
                    line: 1,
                    character: 3,
                }
            ),
            Some(9)
        );
    }

    #[test]
    fn test_position_to_offset_supports_multibyte_characters() {
        let document = "naive\ncaf\u{00e9}\n";
        assert_eq!(
            position_to_offset(
                document,
                Position {
                    line: 1,
                    character: 4,
                }
            ),
            Some(11)
        );
    }
}
