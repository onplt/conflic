use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::{CustomExtractorConfig, CustomSourceConfig};
use crate::model::{ParseDiagnostic, Severity};

use super::{COMPILED_CONFIG_FILE_FALLBACK, CUSTOM_EXTRACTOR_CONFIG_RULE_ID, CompiledCustomSource};

pub(super) fn compile_source(
    source: CustomSourceConfig,
    config: &CustomExtractorConfig,
    index: usize,
    config_path: Option<&Path>,
) -> Result<CompiledCustomSource, ParseDiagnostic> {
    validate_source_format(&source, config, index, config_path)?;

    let normalized_file = normalize_path_string(&source.file);
    let is_glob = is_glob_pattern(&source.file);

    let filename_glob = if is_glob {
        Some(compile_glob(&source.file).map_err(|error| {
            config_diagnostic(
                config_path,
                format!(
                    "Custom extractor '{}' source {} has invalid file glob '{}': {}",
                    config.concept,
                    index + 1,
                    source.file,
                    error
                ),
            )
        })?)
    } else {
        None
    };

    let path_glob = if is_glob {
        Some(compile_glob(&normalized_file).map_err(|error| {
            config_diagnostic(
                config_path,
                format!(
                    "Custom extractor '{}' source {} has invalid path glob '{}': {}",
                    config.concept,
                    index + 1,
                    source.file,
                    error
                ),
            )
        })?)
    } else {
        None
    };

    let relative_path_glob = if is_glob {
        let trimmed = normalized_file
            .trim_start_matches("./")
            .trim_start_matches(".\\")
            .to_string();
        if !trimmed.starts_with("**/") && !Path::new(&trimmed).is_absolute() {
            Some(compile_glob(&format!("**/{}", trimmed)).map_err(|error| {
                config_diagnostic(
                    config_path,
                    format!(
                        "Custom extractor '{}' source {} has invalid relative path glob '{}': {}",
                        config.concept,
                        index + 1,
                        source.file,
                        error
                    ),
                )
            })?)
        } else {
            None
        }
    } else {
        None
    };

    let pattern_regex = source
        .pattern
        .as_deref()
        .map(Regex::new)
        .transpose()
        .map_err(|error| {
            config_diagnostic(
                config_path,
                format!(
                    "Custom extractor '{}' source {} has invalid regex pattern '{}': {}",
                    config.concept,
                    index + 1,
                    source.pattern.as_deref().unwrap_or_default(),
                    error
                ),
            )
        })?;

    Ok(CompiledCustomSource {
        config: source,
        normalized_file,
        filename_glob,
        path_glob,
        relative_path_glob,
        pattern_regex,
    })
}

impl CompiledCustomSource {
    pub(super) fn matches_filename(&self, filename: &str) -> bool {
        if let Some(glob) = &self.filename_glob {
            glob.is_match(filename)
        } else {
            filename == self.config.file
        }
    }

    pub(super) fn matches_path(&self, filename: &str, path: &Path) -> bool {
        if self.matches_filename(filename) {
            return true;
        }

        let path_str = normalize_path_string(&path.to_string_lossy());

        if let Some(glob) = &self.path_glob
            && glob.is_match(&path_str)
        {
            return true;
        }

        if let Some(glob) = &self.relative_path_glob
            && glob.is_match(&path_str)
        {
            return true;
        }

        self.filename_glob.is_none()
            && self.normalized_file.contains('/')
            && path_str.ends_with(&self.normalized_file)
    }
}

fn compile_glob(pattern: &str) -> Result<Arc<globset::GlobMatcher>, globset::Error> {
    Ok(Arc::new(globset::Glob::new(pattern)?.compile_matcher()))
}

fn validate_source_format(
    source: &CustomSourceConfig,
    config: &CustomExtractorConfig,
    index: usize,
    config_path: Option<&Path>,
) -> Result<(), ParseDiagnostic> {
    if matches!(
        source.format.as_str(),
        "json" | "yaml" | "toml" | "env" | "plain" | "dockerfile"
    ) {
        return Ok(());
    }

    Err(config_diagnostic(
        config_path,
        format!(
            "Custom extractor '{}' source {} has invalid source format '{}'. Expected one of: json, yaml, toml, env, plain, dockerfile",
            config.concept,
            index + 1,
            source.format
        ),
    ))
}

pub(super) fn config_diagnostic(
    config_path: Option<&Path>,
    message: impl Into<String>,
) -> ParseDiagnostic {
    ParseDiagnostic {
        severity: Severity::Error,
        file: config_path
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(COMPILED_CONFIG_FILE_FALLBACK)),
        message: message.into(),
        rule_id: CUSTOM_EXTRACTOR_CONFIG_RULE_ID.to_string(),
    }
}

fn is_glob_pattern(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?') || pattern.contains('[')
}

fn normalize_path_string(path: &str) -> String {
    path.replace('\\', "/")
}
