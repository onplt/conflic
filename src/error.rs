use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, ConflicError>;

#[derive(Debug, Error)]
pub enum ConflicError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Git(#[from] GitError),
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Config file not found: {0}")]
    Missing(PathBuf),
    #[error("Failed to read config: {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to parse config: {path}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error(
        "Invalid [conflic].format value '{value}' in {path}. Expected one of: terminal, json, sarif"
    )]
    InvalidFormat { value: String, path: PathBuf },
    #[error(
        "Invalid [conflic].severity value '{value}' in {path}. Expected one of: error, warning, info"
    )]
    InvalidSeverity { value: String, path: PathBuf },
}

#[derive(Debug, Error)]
pub enum GitError {
    #[error("Invalid --diff ref '{value}': refs must not start with '-'")]
    InvalidDiffRef { value: String },
    #[error("Failed to run `git {command}`: {source}")]
    Spawn {
        command: String,
        #[source]
        source: std::io::Error,
    },
    #[error("`git {command}` failed: {stderr}")]
    CommandFailed { command: String, stderr: String },
}
