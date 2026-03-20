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
    scan_root: Option<&Path>,
) -> Result<CompiledCustomSource, ParseDiagnostic> {
    validate_source_format(&source, config, index, config_path)?;

    let normalized_file = normalize_path_string(&source.file);
    let match_path = trim_relative_prefix(&normalized_file);
    let file_is_absolute =
        Path::new(&normalized_file).is_absolute() || normalized_file.starts_with("//");
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
        let candidate = if file_is_absolute {
            normalized_file.as_str()
        } else {
            match_path.as_str()
        };

        Some(compile_glob(candidate).map_err(|error| {
            config_diagnostic(
                config_path,
                format!(
                    "Custom extractor '{}' source {} has invalid path matcher '{}': {}",
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
        match_path,
        scan_root: scan_root.map(crate::pathing::normalize_root),
        file_is_absolute,
        filename_glob,
        path_glob,
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

        let normalized_path = crate::pathing::normalize_path(path);
        let path_str = normalize_path_string(&normalized_path.to_string_lossy());
        let relative_path = self.relative_path_for_match(&normalized_path);

        if let Some(glob) = &self.path_glob
            && self
                .path_match_candidate(&path_str, relative_path.as_deref())
                .is_some_and(|candidate| glob.is_match(candidate))
        {
            return true;
        }

        self.filename_glob.is_none()
            && self.match_path.contains('/')
            && self.path_match_candidate(&path_str, relative_path.as_deref())
                == Some(self.match_path.as_str())
    }

    fn path_match_candidate<'a>(
        &'a self,
        absolute_path: &'a str,
        relative_path: Option<&'a str>,
    ) -> Option<&'a str> {
        if self.file_is_absolute {
            Some(absolute_path)
        } else {
            relative_path
        }
    }

    fn relative_path_for_match(&self, normalized_path: &Path) -> Option<String> {
        if self.file_is_absolute {
            return None;
        }

        if !normalized_path.is_absolute() {
            return Some(trim_relative_prefix(&normalize_path_string(
                &normalized_path.to_string_lossy(),
            )));
        }

        let scan_root = self.scan_root.as_deref()?;
        let relative = normalized_path.strip_prefix(scan_root).ok()?;
        Some(trim_relative_prefix(&normalize_path_string(
            &relative.to_string_lossy(),
        )))
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

fn trim_relative_prefix(path: &str) -> String {
    path.trim_start_matches("./")
        .trim_start_matches(".\\")
        .to_string()
}
