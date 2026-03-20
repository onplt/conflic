pub mod dockerfile;
pub mod env;
pub mod extends;
pub mod json;
pub mod plain_text;
pub mod source_location;
pub mod toml_parser;
pub mod yaml;

use std::path::{Path, PathBuf};

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
    Yaml(serde_yml::Value),
    Toml(toml::Value),
    Dockerfile(Vec<DockerInstruction>),
    Env(Vec<EnvEntry>),
    PlainText(String),
}

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
#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub path: PathBuf,
    pub format: FileFormat,
    pub content: FileContent,
    pub raw_text: String,
}

/// Parse error with context.
#[derive(Debug)]
pub struct ParseError {
    pub file: PathBuf,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.file.display(), self.message)
    }
}

/// Detect format from filename and parse the file.
pub fn parse_file(path: &Path) -> Result<ParsedFile, ParseError> {
    let raw_text = std::fs::read_to_string(path).map_err(|e| ParseError {
        file: path.to_path_buf(),
        message: format!("Failed to read: {}", e),
    })?;

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    let format = detect_format(filename, path);

    let content = match format {
        FileFormat::Json => json::parse_json(&raw_text).map_err(|e| ParseError {
            file: path.to_path_buf(),
            message: e,
        })?,
        FileFormat::Yaml => yaml::parse_yaml(&raw_text).map_err(|e| ParseError {
            file: path.to_path_buf(),
            message: e,
        })?,
        FileFormat::Toml => toml_parser::parse_toml(&raw_text).map_err(|e| ParseError {
            file: path.to_path_buf(),
            message: e,
        })?,
        FileFormat::Dockerfile => {
            FileContent::Dockerfile(dockerfile::parse_dockerfile(&raw_text))
        }
        FileFormat::Env => FileContent::Env(env::parse_env(&raw_text)),
        FileFormat::PlainText => FileContent::PlainText(plain_text::parse_plain_text(&raw_text)),
    };

    Ok(ParsedFile {
        path: path.to_path_buf(),
        format,
        content,
        raw_text,
    })
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
        ".nvmrc" | ".node-version" | ".python-version" | ".ruby-version" | ".tool-versions" | ".go-version" | ".sdkmanrc"
    ) {
        return FileFormat::PlainText;
    }

    // By extension
    match path.extension().and_then(|e| e.to_str()) {
        Some("json" | "jsonc" | "json5") => FileFormat::Json,
        Some("yml" | "yaml") => FileFormat::Yaml,
        Some("toml") => FileFormat::Toml,
        Some("env") => FileFormat::Env,
        _ => {
            // Try to detect from content hints in filename
            if lower.ends_with("rc") && !lower.contains('.') {
                FileFormat::PlainText
            } else {
                FileFormat::PlainText
            }
        }
    }
}
