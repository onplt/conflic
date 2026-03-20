use std::fmt;
use std::path::PathBuf;

use super::assertion::ConfigAssertion;
use super::concept::SemanticConcept;

/// Severity of a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Info => write!(f, "INFO"),
            Severity::Warning => write!(f, "WARNING"),
            Severity::Error => write!(f, "ERROR"),
        }
    }
}

/// A single contradiction finding between two assertions.
#[derive(Debug, Clone)]
pub struct Finding {
    pub severity: Severity,
    pub left: ConfigAssertion,
    pub right: ConfigAssertion,
    pub explanation: String,
    pub rule_id: String,
}

/// A diagnostic raised while reading or resolving configuration files.
#[derive(Debug, Clone)]
pub struct ParseDiagnostic {
    pub severity: Severity,
    pub file: PathBuf,
    pub message: String,
    pub rule_id: String,
}

impl fmt::Display for ParseDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.file.display(), self.message)
    }
}

/// All comparison results for a single semantic concept.
#[derive(Debug, Clone)]
pub struct ConceptResult {
    pub concept: SemanticConcept,
    pub assertions: Vec<ConfigAssertion>,
    pub findings: Vec<Finding>,
}

/// The overall scan result.
#[derive(Debug, Clone)]
pub struct ScanResult {
    pub concept_results: Vec<ConceptResult>,
    pub parse_diagnostics: Vec<ParseDiagnostic>,
}

impl ScanResult {
    pub fn error_count(&self) -> usize {
        self.concept_results
            .iter()
            .flat_map(|cr| &cr.findings)
            .filter(|f| f.severity == Severity::Error)
            .count()
            + self
                .parse_diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .count()
    }

    pub fn warning_count(&self) -> usize {
        self.concept_results
            .iter()
            .flat_map(|cr| &cr.findings)
            .filter(|f| f.severity == Severity::Warning)
            .count()
            + self
                .parse_diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Warning)
                .count()
    }

    pub fn info_count(&self) -> usize {
        self.concept_results
            .iter()
            .flat_map(|cr| &cr.findings)
            .filter(|f| f.severity == Severity::Info)
            .count()
            + self
                .parse_diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Info)
                .count()
    }

    pub fn has_findings_at_or_above(&self, min_severity: Severity) -> bool {
        self.concept_results
            .iter()
            .flat_map(|cr| &cr.findings)
            .any(|f| f.severity >= min_severity)
            || self
                .parse_diagnostics
                .iter()
                .any(|d| d.severity >= min_severity)
    }
}
