use std::fmt;
use std::path::PathBuf;

use super::concept::SemanticConcept;
use super::semantic_type::SemanticType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

/// Where in the source tree a value was found.
#[derive(Debug, Clone)]
pub struct SourceLocation {
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
    pub key_path: String,
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let file = self.file.display();
        if self.column > 0 {
            write!(f, "{}:{}:{}", file, self.line, self.column)
        } else {
            write!(f, "{}:{}", file, self.line)
        }
    }
}

/// How binding/authoritative is this assertion?
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Authority {
    /// Informational / advisory (.nvmrc, .tool-versions)
    Advisory,
    /// Declared preference — should match but not mechanically enforced (package.json engines)
    Declared,
    /// Hard constraint — build will break if violated (Dockerfile FROM, CI matrix)
    Enforced,
}

impl fmt::Display for Authority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Authority::Advisory => write!(f, "advisory"),
            Authority::Declared => write!(f, "declared"),
            Authority::Enforced => write!(f, "enforced"),
        }
    }
}

/// A single extracted config assertion.
#[derive(Debug, Clone)]
pub struct ConfigAssertion {
    pub concept: SemanticConcept,
    pub value: SemanticType,
    pub raw_value: String,
    pub source: SourceLocation,
    pub span: Option<SourceSpan>,
    pub authority: Authority,
    pub extractor_id: String,
    pub is_matrix: bool,
}

impl ConfigAssertion {
    pub fn new(
        concept: SemanticConcept,
        value: SemanticType,
        raw_value: String,
        source: SourceLocation,
        authority: Authority,
        extractor_id: impl Into<String>,
    ) -> Self {
        Self {
            concept,
            value,
            raw_value,
            source,
            span: None,
            authority,
            extractor_id: extractor_id.into(),
            is_matrix: false,
        }
    }

    pub fn with_span(mut self, span: SourceSpan) -> Self {
        self.source.line = span.line;
        self.source.column = span.column;
        self.span = Some(span);
        self
    }

    pub fn with_optional_span(mut self, span: Option<SourceSpan>) -> Self {
        if let Some(span) = span {
            self.source.line = span.line;
            self.source.column = span.column;
            self.span = Some(span);
        }
        self
    }

    pub fn with_matrix(mut self, is_matrix: bool) -> Self {
        self.is_matrix = is_matrix;
        self
    }
}
