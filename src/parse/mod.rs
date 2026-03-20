pub mod dockerfile;
pub mod env;
pub mod extends;
pub mod json;
pub mod plain_text;
pub mod source_location;
pub mod toml_parser;
pub mod xml;
pub mod yaml;

use crate::model::{ParseDiagnostic, Severity};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

pub(crate) const PARSE_FILE_ERROR_RULE_ID: &str = "PARSE001";
pub(crate) const PARSE_EXTENDS_ERROR_RULE_ID: &str = "PARSE002";

pub(crate) fn parse_diagnostic(
    severity: Severity,
    file: PathBuf,
    rule_id: &str,
    message: impl Into<String>,
) -> ParseDiagnostic {
    ParseDiagnostic {
        severity,
        file,
        message: message.into(),
        rule_id: rule_id.to_string(),
    }
}

/// Supported file formats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileFormat {
    Json,
    Yaml,
    Toml,
    Dockerfile,
    Env,
    PlainText,
}

/// Parsed content from a config file.
#[derive(Debug, Clone)]
pub enum FileContent {
    Json(serde_json::Value),
    Yaml(YamlValue),
    Toml(toml::Value),
    Dockerfile(Vec<DockerInstruction>),
    Env(Vec<EnvEntry>),
    PlainText(String),
}

/// YAML is deserialized into a JSON-like tree because all supported YAML
/// extractors address string-keyed mappings and scalar values.
pub type YamlValue = serde_json::Value;

/// A single instruction from a Dockerfile.
#[derive(Debug, Clone)]
pub struct DockerInstruction {
    pub instruction: String,
    pub arguments: String,
    pub line: usize,
    pub stage_index: usize,
    pub stage_name: Option<String>,
    pub is_final_stage: bool,
}

/// A key-value entry from a .env file.
#[derive(Debug, Clone)]
pub struct EnvEntry {
    pub key: String,
    pub value: String,
    pub line: usize,
}

/// A parsed config file with its content and metadata.
#[derive(Debug)]
pub struct ParsedFile {
    pub path: PathBuf,
    pub scan_root: PathBuf,
    pub format: FileFormat,
    pub content: FileContent,
    pub raw_text: String,
    parse_diagnostics: RefCell<Vec<ParseDiagnostic>>,
}

impl ParsedFile {
    pub fn push_parse_diagnostic(&self, diagnostic: ParseDiagnostic) {
        self.parse_diagnostics.borrow_mut().push(diagnostic);
    }

    pub fn take_parse_diagnostics(&self) -> Vec<ParseDiagnostic> {
        self.parse_diagnostics.borrow_mut().drain(..).collect()
    }
}

/// Detect format from filename and parse the file.
pub fn parse_file(path: &Path, scan_root: &Path) -> Result<ParsedFile, ParseDiagnostic> {
    let raw_text = std::fs::read_to_string(path).map_err(|e| {
        parse_diagnostic(
            Severity::Error,
            path.to_path_buf(),
            PARSE_FILE_ERROR_RULE_ID,
            format!("Failed to read: {}", e),
        )
    })?;

    parse_file_with_content(path, scan_root, raw_text)
}

pub fn parse_file_with_content(
    path: &Path,
    scan_root: &Path,
    raw_text: String,
) -> Result<ParsedFile, ParseDiagnostic> {
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let lower = filename.to_ascii_lowercase();

    let (format, content) = if lower == ".eslintrc" {
        parse_extensionless_eslint_config(&raw_text).map_err(|e| {
            parse_diagnostic(
                Severity::Error,
                path.to_path_buf(),
                PARSE_FILE_ERROR_RULE_ID,
                e,
            )
        })?
    } else {
        let format = detect_format(filename, path);
        let content = parse_content_for_format(format.clone(), &raw_text).map_err(|e| {
            parse_diagnostic(
                Severity::Error,
                path.to_path_buf(),
                PARSE_FILE_ERROR_RULE_ID,
                e,
            )
        })?;
        (format, content)
    };

    Ok(ParsedFile {
        path: path.to_path_buf(),
        scan_root: std::fs::canonicalize(scan_root).unwrap_or_else(|_| scan_root.to_path_buf()),
        format,
        content,
        raw_text,
        parse_diagnostics: RefCell::new(Vec::new()),
    })
}

fn parse_content_for_format(format: FileFormat, raw_text: &str) -> Result<FileContent, String> {
    match format {
        FileFormat::Json => json::parse_json(raw_text),
        FileFormat::Yaml => yaml::parse_yaml(raw_text),
        FileFormat::Toml => toml_parser::parse_toml(raw_text),
        FileFormat::Dockerfile => Ok(FileContent::Dockerfile(dockerfile::parse_dockerfile(
            raw_text,
        ))),
        FileFormat::Env => Ok(FileContent::Env(env::parse_env(raw_text))),
        FileFormat::PlainText => Ok(FileContent::PlainText(plain_text::parse_plain_text(
            raw_text,
        ))),
    }
}

fn parse_extensionless_eslint_config(raw_text: &str) -> Result<(FileFormat, FileContent), String> {
    match json::parse_json(raw_text) {
        Ok(content) => Ok((FileFormat::Json, content)),
        Err(json_error) => match yaml::parse_yaml(raw_text) {
            Ok(content) => Ok((FileFormat::Yaml, content)),
            Err(yaml_error) => Err(format!(
                "Failed to parse .eslintrc as JSON/JSON5 or YAML: {}; {}",
                json_error, yaml_error
            )),
        },
    }
}

pub fn detect_format(filename: &str, path: &Path) -> FileFormat {
    let lower = filename.to_lowercase();

    // Dockerfile variants
    if lower == "dockerfile" || lower.starts_with("dockerfile.") {
        return FileFormat::Dockerfile;
    }

    // .env variants
    if lower == ".env" || lower.starts_with(".env.") {
        return FileFormat::Env;
    }

    // Plain text single-value files
    if matches!(
        lower.as_str(),
        ".nvmrc"
            | ".node-version"
            | ".python-version"
            | ".ruby-version"
            | ".tool-versions"
            | ".go-version"
            | ".sdkmanrc"
    ) {
        return FileFormat::PlainText;
    }

    // By extension
    match path.extension().and_then(|e| e.to_str()) {
        Some("json" | "jsonc" | "json5") => FileFormat::Json,
        Some("yml" | "yaml") => FileFormat::Yaml,
        Some("toml") => FileFormat::Toml,
        Some("env") => FileFormat::Env,
        _ => FileFormat::PlainText,
    }
}
