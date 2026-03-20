use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::config::ConflicConfig;
use crate::discover::FileDiscoverer;
use crate::extract::{self, Extractor};
use crate::model::ParseDiagnostic;

#[derive(Debug, Clone)]
pub(crate) struct FileScanPlan {
    pub(crate) filename: String,
    pub(crate) path: PathBuf,
    pub(crate) normalized_path: PathBuf,
    pub(crate) extractor_indices: Vec<usize>,
    pub(crate) concept_ids: HashSet<String>,
}

pub(crate) struct ScanPreparation {
    pub(crate) discovered_files: HashMap<String, Vec<PathBuf>>,
    pub(crate) file_plans: Vec<FileScanPlan>,
    pub(crate) extractor_count: usize,
    pub(crate) extractor_names: Vec<(String, String)>,
    pub(crate) extractors: Vec<Box<dyn Extractor>>,
    pub(crate) initial_diagnostics: Vec<ParseDiagnostic>,
}

pub(crate) struct ExtractorPreparation {
    pub(crate) extractor_count: usize,
    pub(crate) extractor_names: Vec<(String, String)>,
    pub(crate) extractors: Vec<Box<dyn Extractor>>,
    pub(crate) initial_diagnostics: Vec<ParseDiagnostic>,
}

pub(crate) fn prepare_scan(root: &Path, config: &ConflicConfig) -> ScanPreparation {
    let discovered_files = discover_files(root, config);
    let extractor_build = prepare_extractors(config);

    let file_plans: Vec<FileScanPlan> = discovered_files
        .iter()
        .flat_map(|(filename, paths)| {
            paths
                .iter()
                .map(|path| build_file_scan_plan(root, filename, path, &extractor_build.extractors))
                .collect::<Vec<_>>()
        })
        .collect();

    ScanPreparation {
        discovered_files,
        file_plans,
        extractor_count: extractor_build.extractor_count,
        extractor_names: extractor_build.extractor_names,
        extractors: extractor_build.extractors,
        initial_diagnostics: extractor_build.initial_diagnostics,
    }
}

pub(crate) fn prepare_extractors(config: &ConflicConfig) -> ExtractorPreparation {
    let extractor_build = extract::build_extractors(config);
    let extractor_names: Vec<(String, String)> = extractor_build
        .extractors
        .iter()
        .map(|extractor| {
            (
                extractor.id().to_string(),
                extractor.description().to_string(),
            )
        })
        .collect();
    let extractor_count = extractor_build.extractors.len();

    ExtractorPreparation {
        extractor_count,
        extractor_names,
        extractors: extractor_build.extractors,
        initial_diagnostics: extractor_build.diagnostics,
    }
}

pub(crate) fn discover_files(root: &Path, config: &ConflicConfig) -> HashMap<String, Vec<PathBuf>> {
    let discoverer = FileDiscoverer::new(
        root,
        config.conflic.exclude.clone(),
        custom_discovery_patterns(config),
    );
    discoverer.discover()
}

pub(crate) fn build_file_scan_plan(
    root: &Path,
    filename: &str,
    path: &Path,
    extractors: &[Box<dyn Extractor>],
) -> FileScanPlan {
    let extractor_indices = matching_extractor_indices(filename, path, extractors);
    let concept_ids = concept_ids_for_indices(extractors, &extractor_indices);

    FileScanPlan {
        filename: filename.to_string(),
        path: path.to_path_buf(),
        normalized_path: normalize_scan_path(root, path),
        extractor_indices,
        concept_ids,
    }
}

pub(crate) fn build_plan_for_path(
    root: &Path,
    path: &Path,
    extractors: &[Box<dyn Extractor>],
    allowed_extractors: Option<&HashSet<usize>>,
) -> Option<FileScanPlan> {
    let filename = path.file_name()?.to_str()?;
    let mut plan = build_file_scan_plan(root, filename, path, extractors);

    if let Some(allowed) = allowed_extractors {
        plan.extractor_indices
            .retain(|index| allowed.contains(index));
        plan.concept_ids = concept_ids_for_indices(extractors, &plan.extractor_indices);
    }

    (!plan.extractor_indices.is_empty()).then_some(plan)
}

pub(crate) fn impacted_extractor_indices(
    extractors: &[Box<dyn Extractor>],
    impacted_concepts: &HashSet<String>,
) -> HashSet<usize> {
    extractors
        .iter()
        .enumerate()
        .filter(|(_, extractor)| {
            extractor
                .concept_ids()
                .iter()
                .any(|concept_id| impacted_concepts.contains(concept_id))
        })
        .map(|(index, _)| index)
        .collect()
}

pub(crate) fn group_discovered_files(plans: &[FileScanPlan]) -> HashMap<String, Vec<PathBuf>> {
    let mut discovered_files: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for plan in plans {
        discovered_files
            .entry(plan.filename.clone())
            .or_default()
            .push(plan.path.clone());
    }

    for paths in discovered_files.values_mut() {
        paths.sort();
        paths.dedup();
    }

    discovered_files
}

pub(crate) fn normalize_changed_files(root: &Path, changed_files: &[PathBuf]) -> HashSet<PathBuf> {
    changed_files
        .iter()
        .filter_map(|path| crate::pathing::normalize_if_within_root(root, path))
        .collect()
}

pub(crate) fn normalize_content_overrides(
    root: &Path,
    content_overrides: &HashMap<PathBuf, String>,
) -> HashMap<PathBuf, String> {
    content_overrides
        .iter()
        .filter_map(|(path, content)| {
            crate::pathing::normalize_if_within_root(root, path)
                .map(|normalized| (normalized, content.clone()))
        })
        .collect()
}

fn matching_extractor_indices(
    filename: &str,
    path: &Path,
    extractors: &[Box<dyn Extractor>],
) -> Vec<usize> {
    extractors
        .iter()
        .enumerate()
        .filter(|(_, extractor)| extractor.matches_path(filename, path))
        .map(|(index, _)| index)
        .collect()
}

fn concept_ids_for_indices(
    extractors: &[Box<dyn Extractor>],
    extractor_indices: &[usize],
) -> HashSet<String> {
    extractor_indices
        .iter()
        .flat_map(|index| extractors[*index].concept_ids())
        .collect()
}

fn normalize_scan_path(root: &Path, path: &Path) -> PathBuf {
    crate::pathing::normalize_for_workspace(root, path)
}

fn custom_discovery_patterns(config: &ConflicConfig) -> Vec<String> {
    let (custom_extractors, _) = config.compiled_custom_extractors();
    custom_extractors
        .iter()
        .flat_map(|extractor| extractor.relevant_filenames())
        .map(str::to_string)
        .collect()
}
