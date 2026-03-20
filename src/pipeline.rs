use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rayon::prelude::*;

use crate::config::ConflicConfig;
use crate::extract::Extractor;
use crate::model::{ConfigAssertion, ParseDiagnostic, ScanResult};
use crate::parse;
use crate::planning::{
    FileScanPlan, build_plan_for_path, discover_files, group_discovered_files,
    impacted_extractor_indices, normalize_changed_files, normalize_content_overrides,
    prepare_extractors, prepare_scan,
};
use crate::solve;

#[derive(Debug)]
pub struct DoctorFileInfo {
    pub path: PathBuf,
    pub filename: String,
    pub matched_extractors: Vec<String>,
    pub assertions: Vec<ConfigAssertion>,
    pub parse_diagnostics: Vec<ParseDiagnostic>,
}

#[derive(Debug)]
pub struct DoctorReport {
    pub root: PathBuf,
    pub discovered_files: HashMap<String, Vec<PathBuf>>,
    pub file_details: Vec<DoctorFileInfo>,
    pub scan_result: ScanResult,
    pub extractor_count: usize,
    pub extractor_names: Vec<(String, String)>,
}

#[derive(Debug)]
pub(crate) struct ScanPipeline {
    discovered_files: HashMap<String, Vec<PathBuf>>,
    extractor_count: usize,
    extractor_names: Vec<(String, String)>,
    file_outcomes: Vec<ScannedFile>,
    initial_diagnostics: Vec<ParseDiagnostic>,
}

#[derive(Debug, Clone)]
pub(crate) struct ScannedFile {
    pub(crate) filename: String,
    pub(crate) path: PathBuf,
    pub(crate) normalized_path: PathBuf,
    pub(crate) matched_extractors: Vec<String>,
    pub(crate) assertions: Vec<ConfigAssertion>,
    pub(crate) parse_diagnostics: Vec<ParseDiagnostic>,
}

impl ScanPipeline {
    pub(crate) fn full_scan_result(&self, root: &Path, config: &ConflicConfig) -> ScanResult {
        let assertions = self
            .file_outcomes
            .iter()
            .flat_map(|outcome| outcome.assertions.iter().cloned())
            .collect();

        let mut parse_diagnostics = self.initial_diagnostics.clone();
        parse_diagnostics.extend(
            self.file_outcomes
                .iter()
                .flat_map(|outcome| outcome.parse_diagnostics.iter().cloned()),
        );

        build_scan_result(root, config, assertions, parse_diagnostics)
    }

    pub(crate) fn into_doctor_report(self, root: &Path, config: &ConflicConfig) -> DoctorReport {
        let scan_result = self.full_scan_result(root, config);
        let file_details = self.doctor_file_details();

        DoctorReport {
            root: root.to_path_buf(),
            discovered_files: self.discovered_files,
            file_details,
            scan_result,
            extractor_count: self.extractor_count,
            extractor_names: self.extractor_names,
        }
    }

    fn doctor_file_details(&self) -> Vec<DoctorFileInfo> {
        self.file_outcomes
            .iter()
            .map(|outcome| DoctorFileInfo {
                path: outcome.path.clone(),
                filename: outcome.filename.clone(),
                matched_extractors: outcome.matched_extractors.clone(),
                assertions: outcome.assertions.clone(),
                parse_diagnostics: outcome.parse_diagnostics.clone(),
            })
            .collect()
    }
}

pub(crate) fn run_scan_pipeline(
    root: &Path,
    config: &ConflicConfig,
    content_overrides: Option<&HashMap<PathBuf, String>>,
) -> ScanPipeline {
    let preparation = prepare_scan(root, config);
    let normalized_overrides =
        content_overrides.map(|overrides| Arc::new(normalize_content_overrides(root, overrides)));
    let mut file_outcomes = process_file_plans(
        root,
        &preparation.file_plans,
        &preparation.extractors,
        normalized_overrides.as_ref(),
    );
    file_outcomes.sort_by(|left, right| left.path.cmp(&right.path));

    ScanPipeline {
        discovered_files: preparation.discovered_files,
        extractor_count: preparation.extractor_count,
        extractor_names: preparation.extractor_names,
        file_outcomes,
        initial_diagnostics: preparation.initial_diagnostics,
    }
}

pub(crate) fn run_diff_scan_pipeline(
    root: &Path,
    config: &ConflicConfig,
    changed_files: &[PathBuf],
) -> ScanPipeline {
    let changed_normalized = normalize_changed_files(root, changed_files);
    let preparation = prepare_extractors(config);
    let normalized_config_path =
        crate::pathing::normalize_for_workspace(root, &config.resolved_config_path(root));
    let config_changed = changed_files
        .iter()
        .map(|path| crate::pathing::normalize_for_workspace(root, path))
        .any(|path| path == normalized_config_path);

    let mut impacted_concepts = HashSet::new();
    let mut changed_plans = Vec::new();

    for path in &changed_normalized {
        if let Some(plan) = build_plan_for_path(root, path, &preparation.extractors, None) {
            impacted_concepts.extend(plan.concept_ids.iter().cloned());
            if path.exists() {
                changed_plans.push(plan);
            }
        }
    }

    if config_changed {
        impacted_concepts.extend(
            preparation
                .extractors
                .iter()
                .flat_map(|extractor| extractor.concept_ids()),
        );
    }

    let mut file_outcomes = process_file_plans(root, &changed_plans, &preparation.extractors, None);
    impacted_concepts.extend(file_outcomes.iter().flat_map(|outcome| {
        outcome
            .assertions
            .iter()
            .map(|assertion| assertion.concept.id.clone())
    }));

    let mut scanned_plans = changed_plans.clone();
    if !impacted_concepts.is_empty() {
        let impacted_extractors =
            impacted_extractor_indices(&preparation.extractors, &impacted_concepts);
        if !impacted_extractors.is_empty() {
            let discovered_files = discover_files(root, config);
            let mut peer_plans = Vec::new();

            for paths in discovered_files.values() {
                for path in paths {
                    let normalized_path = crate::pathing::normalize_for_workspace(root, path);
                    if changed_normalized.contains(&normalized_path) {
                        continue;
                    }

                    if let Some(plan) = build_plan_for_path(
                        root,
                        path,
                        &preparation.extractors,
                        Some(&impacted_extractors),
                    ) {
                        peer_plans.push(plan);
                    }
                }
            }

            file_outcomes.extend(process_file_plans(
                root,
                &peer_plans,
                &preparation.extractors,
                None,
            ));
            scanned_plans.extend(peer_plans);
        }
    }

    file_outcomes.sort_by(|left, right| left.path.cmp(&right.path));

    ScanPipeline {
        discovered_files: group_discovered_files(&scanned_plans),
        extractor_count: preparation.extractor_count,
        extractor_names: preparation.extractor_names,
        file_outcomes,
        initial_diagnostics: preparation.initial_diagnostics,
    }
}

pub(crate) fn process_file_plans(
    root: &Path,
    plans: &[FileScanPlan],
    extractors: &[Box<dyn Extractor>],
    content_overrides: Option<&Arc<HashMap<PathBuf, String>>>,
) -> Vec<ScannedFile> {
    plans
        .par_iter()
        .map(|plan| process_file_plan(root, plan, extractors, content_overrides))
        .collect()
}

fn process_file_plan(
    root: &Path,
    plan: &FileScanPlan,
    extractors: &[Box<dyn Extractor>],
    content_overrides: Option<&Arc<HashMap<PathBuf, String>>>,
) -> ScannedFile {
    let matched_extractors: Vec<String> = plan
        .extractor_indices
        .iter()
        .map(|index| extractors[*index].id().to_string())
        .collect();

    if plan.extractor_indices.is_empty() {
        return ScannedFile {
            filename: plan.filename.clone(),
            path: plan.path.clone(),
            normalized_path: plan.normalized_path.clone(),
            matched_extractors,
            assertions: Vec::new(),
            parse_diagnostics: Vec::new(),
        };
    }

    let parsed = match content_overrides {
        Some(overrides) => parse::parse_file_with_shared_context(
            &plan.path,
            root,
            overrides.get(&plan.normalized_path).cloned(),
            Arc::clone(overrides),
        ),
        None => parse::parse_file(&plan.path, root),
    };

    match parsed {
        Ok(parsed) => {
            let mut assertions = Vec::new();
            for index in &plan.extractor_indices {
                assertions.extend(extractors[*index].extract(&parsed));
            }

            ScannedFile {
                filename: plan.filename.clone(),
                path: plan.path.clone(),
                normalized_path: plan.normalized_path.clone(),
                matched_extractors,
                assertions,
                parse_diagnostics: parsed.take_parse_diagnostics(),
            }
        }
        Err(error) => ScannedFile {
            filename: plan.filename.clone(),
            path: plan.path.clone(),
            normalized_path: plan.normalized_path.clone(),
            matched_extractors,
            assertions: Vec::new(),
            parse_diagnostics: vec![error],
        },
    }
}

fn build_scan_result(
    root: &Path,
    config: &ConflicConfig,
    assertions: Vec<ConfigAssertion>,
    parse_diagnostics: Vec<ParseDiagnostic>,
) -> ScanResult {
    let concept_results = solve::compare_assertions(root, assertions, config);

    ScanResult {
        concept_results,
        parse_diagnostics,
    }
}
