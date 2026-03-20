use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "conflic",
    version,
    about = "Detect semantic contradictions across config files"
)]
pub struct Cli {
    /// Directory to scan (defaults to current directory)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Output format
    #[arg(long, short, default_value = "terminal", value_enum)]
    pub format: OutputFormat,

    /// Minimum severity to report
    #[arg(long, short, default_value = "warning", value_enum)]
    pub severity: SeverityFilter,

    /// Only check specific concept(s), comma-separated
    #[arg(long, value_delimiter = ',')]
    pub check: Option<Vec<String>>,

    /// Show explanations for each rule
    #[arg(long)]
    pub explain: bool,

    /// Create a template .conflic.toml in the target directory
    #[arg(long)]
    pub init: bool,

    /// Path to config file (default: .conflic.toml in scanned dir)
    #[arg(long, short)]
    pub config: Option<PathBuf>,

    /// Suppress all output except findings
    #[arg(long, short)]
    pub quiet: bool,

    /// Show all assertions, even when no contradictions found
    #[arg(long, short)]
    pub verbose: bool,

    /// Disable colored output
    #[arg(long)]
    pub no_color: bool,

    /// List all available concepts and their extractors
    #[arg(long)]
    pub list_concepts: bool,

    /// Diagnostic mode: show discovered files, extractors, and assertions
    #[arg(long)]
    pub doctor: bool,

    /// Only scan files changed since the given git ref (e.g. HEAD~1, main)
    #[arg(long)]
    pub diff: Option<String>,

    /// Read changed file list from stdin (one path per line)
    #[arg(long)]
    pub diff_stdin: bool,

    /// Show proposed fixes based on authority winners
    #[arg(long)]
    pub fix: bool,

    /// Only show proposed changes without applying them (use with --fix)
    #[arg(long)]
    pub dry_run: bool,

    /// Skip confirmation prompt when applying fixes (use with --fix)
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Don't create .conflic.bak backup files when applying fixes
    #[arg(long)]
    pub no_backup: bool,

    /// Only fix a specific concept (use with --fix)
    #[arg(long)]
    pub concept: Option<String>,

    /// Path to baseline file to suppress known findings
    #[arg(long)]
    pub baseline: Option<PathBuf>,

    /// Generate or update a baseline file from current findings
    #[arg(long)]
    pub update_baseline: Option<PathBuf>,

    /// Start LSP server on stdin/stdout for editor integration
    #[arg(long)]
    pub lsp: bool,
}

#[derive(ValueEnum, Clone, Debug, PartialEq)]
pub enum OutputFormat {
    Terminal,
    Json,
    Sarif,
}

#[derive(ValueEnum, Clone, Debug, PartialEq)]
pub enum SeverityFilter {
    Error,
    Warning,
    Info,
}

impl SeverityFilter {
    pub fn to_severity(&self) -> crate::model::Severity {
        match self {
            SeverityFilter::Error => crate::model::Severity::Error,
            SeverityFilter::Warning => crate::model::Severity::Warning,
            SeverityFilter::Info => crate::model::Severity::Info,
        }
    }
}
