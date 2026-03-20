use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::config::ConflicConfig;
use crate::extract::Extractor;
use crate::model::{ConceptResult, ConfigAssertion, ParseDiagnostic, ScanResult};
use crate::pipeline::{ScannedFile, process_file_plans};
use crate::planning::{
    FileScanPlan, build_file_scan_plan, normalize_changed_files, normalize_content_overrides,
    prepare_scan,
};
use crate::solve;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncrementalScanKind {
    Full,
    Incremental,
}

#[derive(Debug, Clone)]
pub struct IncrementalScanStats {
    pub kind: IncrementalScanKind,
    pub parsed_files: usize,
    pub changed_files: usize,
    pub peer_files: usize,
    pub impacted_concepts: usize,
}

impl Default for IncrementalScanStats {
    fn default() -> Self {
        Self {
            kind: IncrementalScanKind::Full,
            parsed_files: 0,
            changed_files: 0,
            peer_files: 0,
            impacted_concepts: 0,
        }
    }
}

pub struct IncrementalWorkspace {
    root: PathBuf,
    config: ConflicConfig,
    extractors: Vec<Box<dyn Extractor>>,
    initial_diagnostics: Vec<ParseDiagnostic>,
    file_plans: HashMap<PathBuf, FileScanPlan>,
    concept_candidates: HashMap<String, HashSet<PathBuf>>,
    file_outcomes: HashMap<PathBuf, ScannedFile>,
    concept_assertion_files: HashMap<String, HashSet<PathBuf>>,
    concept_results: HashMap<String, ConceptResult>,
    parse_diagnostics: HashMap<PathBuf, Vec<ParseDiagnostic>>,
    last_stats: IncrementalScanStats,
}

impl IncrementalWorkspace {
    pub fn new(root: &Path, config: &ConflicConfig) -> Self {
        let preparation = prepare_scan(root, config);
        let mut workspace = Self {
            root: root.to_path_buf(),
            config: config.clone(),
            extractors: preparation.extractors,
            initial_diagnostics: preparation.initial_diagnostics,
            file_plans: HashMap::new(),
            concept_candidates: HashMap::new(),
            file_outcomes: HashMap::new(),
            concept_assertion_files: HashMap::new(),
            concept_results: HashMap::new(),
            parse_diagnostics: HashMap::new(),
            last_stats: IncrementalScanStats::default(),
        };

        for plan in preparation.file_plans {
            workspace.insert_file_plan(plan);
        }

        workspace
    }

    pub fn full_scan(&mut self, content_overrides: &HashMap<PathBuf, String>) -> ScanResult {
        let normalized_overrides = normalize_content_overrides(&self.root, content_overrides);
        let mut plans: Vec<FileScanPlan> = self.file_plans.values().cloned().collect();
        plans.sort_by(|left, right| left.path.cmp(&right.path));

        let outcomes = process_file_plans(
            &self.root,
            &plans,
            &self.extractors,
            Some(&normalized_overrides),
        );
        self.rebuild_from_outcomes(outcomes);
        self.last_stats = IncrementalScanStats {
            kind: IncrementalScanKind::Full,
            parsed_files: plans.len(),
            changed_files: plans.len(),
            peer_files: 0,
            impacted_concepts: self.concept_results.len(),
        };
        self.current_scan_result()
    }

    pub fn scan_incremental(
        &mut self,
        changed_files: &[PathBuf],
        content_overrides: &HashMap<PathBuf, String>,
    ) -> ScanResult {
        let changed_normalized = normalize_changed_files(&self.root, changed_files);
        if changed_normalized.is_empty() {
            self.last_stats = IncrementalScanStats {
                kind: IncrementalScanKind::Incremental,
                ..IncrementalScanStats::default()
            };
            return self.current_scan_result();
        }

        let normalized_overrides = normalize_content_overrides(&self.root, content_overrides);
        let mut changed_plans = Vec::new();
        let mut changed_paths = HashSet::new();
        let mut impacted_concepts = HashSet::new();

        for normalized_path in changed_normalized {
            impacted_concepts.extend(self.file_outcome_concepts(&normalized_path));

            if let Some(previous_plan) = self.file_plans.get(&normalized_path) {
                impacted_concepts.extend(previous_plan.concept_ids.iter().cloned());
            }

            match self.ensure_file_plan(&normalized_path, &normalized_overrides) {
                Some(plan) => {
                    impacted_concepts.extend(plan.concept_ids.iter().cloned());
                    changed_paths.insert(plan.normalized_path.clone());
                    changed_plans.push(plan);
                }
                None => {
                    self.remove_file_plan(&normalized_path);
                    self.remove_file_outcome(&normalized_path);
                }
            }
        }

        let changed_outcomes = process_file_plans(
            &self.root,
            &changed_plans,
            &self.extractors,
            Some(&normalized_overrides),
        );

        for outcome in &changed_outcomes {
            impacted_concepts.extend(
                outcome
                    .assertions
                    .iter()
                    .map(|assertion| assertion.concept.id.clone()),
            );
        }

        let peer_paths: HashSet<PathBuf> = impacted_concepts
            .iter()
            .flat_map(|concept_id| {
                self.concept_candidates
                    .get(concept_id)
                    .into_iter()
                    .flat_map(|paths| paths.iter().cloned())
            })
            .filter(|path| !changed_paths.contains(path))
            .collect();

        let mut peer_plans: Vec<FileScanPlan> = peer_paths
            .iter()
            .filter_map(|path| self.file_plans.get(path).cloned())
            .collect();
        peer_plans.sort_by(|left, right| left.path.cmp(&right.path));

        let peer_outcomes = process_file_plans(
            &self.root,
            &peer_plans,
            &self.extractors,
            Some(&normalized_overrides),
        );

        for outcome in changed_outcomes.into_iter().chain(peer_outcomes) {
            self.upsert_file_outcome(outcome);
        }

        self.recompute_concept_results(&impacted_concepts);
        self.last_stats = IncrementalScanStats {
            kind: IncrementalScanKind::Incremental,
            parsed_files: changed_plans.len() + peer_plans.len(),
            changed_files: changed_plans.len(),
            peer_files: peer_plans.len(),
            impacted_concepts: impacted_concepts.len(),
        };

        self.current_scan_result()
    }

    pub fn current_scan_result(&self) -> ScanResult {
        let mut concept_results: Vec<ConceptResult> =
            self.concept_results.values().cloned().collect();
        concept_results
            .sort_by(|left, right| left.concept.display_name.cmp(&right.concept.display_name));

        let mut parse_diagnostics = self.initial_diagnostics.clone();
        let mut cached_diagnostics: Vec<ParseDiagnostic> = self
            .parse_diagnostics
            .values()
            .flat_map(|diagnostics| diagnostics.iter().cloned())
            .collect();
        cached_diagnostics.sort_by(|left, right| {
            left.file
                .cmp(&right.file)
                .then_with(|| left.rule_id.cmp(&right.rule_id))
                .then_with(|| left.message.cmp(&right.message))
        });
        parse_diagnostics.extend(cached_diagnostics);

        ScanResult {
            concept_results,
            parse_diagnostics,
        }
    }

    pub fn last_stats(&self) -> IncrementalScanStats {
        self.last_stats.clone()
    }

    fn rebuild_from_outcomes(&mut self, outcomes: Vec<ScannedFile>) {
        self.file_outcomes.clear();
        self.concept_assertion_files.clear();
        self.concept_results.clear();
        self.parse_diagnostics.clear();

        for outcome in outcomes {
            self.upsert_file_outcome(outcome);
        }

        let impacted_concepts: HashSet<String> =
            self.concept_assertion_files.keys().cloned().collect();
        self.recompute_concept_results(&impacted_concepts);
    }

    fn ensure_file_plan(
        &mut self,
        normalized_path: &Path,
        content_overrides: &HashMap<PathBuf, String>,
    ) -> Option<FileScanPlan> {
        let path = crate::pathing::normalize_if_within_root(&self.root, normalized_path)?;

        if let Some(plan) = self.file_plans.get(&path) {
            return Some(plan.clone());
        }

        if !content_overrides.contains_key(&path) && !path.exists() {
            return None;
        }

        let filename = path.file_name()?.to_str()?.to_string();
        let plan = build_file_scan_plan(&self.root, &filename, &path, &self.extractors);
        if plan.extractor_indices.is_empty() {
            return None;
        }

        self.insert_file_plan(plan.clone());
        Some(plan)
    }

    fn insert_file_plan(&mut self, plan: FileScanPlan) {
        let normalized_path = plan.normalized_path.clone();
        for concept_id in &plan.concept_ids {
            self.concept_candidates
                .entry(concept_id.clone())
                .or_default()
                .insert(normalized_path.clone());
        }
        self.file_plans.insert(normalized_path, plan);
    }

    fn remove_file_plan(&mut self, normalized_path: &Path) {
        if let Some(plan) = self.file_plans.remove(normalized_path) {
            for concept_id in &plan.concept_ids {
                if let Some(paths) = self.concept_candidates.get_mut(concept_id) {
                    paths.remove(normalized_path);
                    if paths.is_empty() {
                        self.concept_candidates.remove(concept_id);
                    }
                }
            }
        }
    }

    fn upsert_file_outcome(&mut self, outcome: ScannedFile) {
        let normalized_path = outcome.normalized_path.clone();
        self.remove_file_outcome(&normalized_path);

        for assertion in &outcome.assertions {
            self.concept_assertion_files
                .entry(assertion.concept.id.clone())
                .or_default()
                .insert(normalized_path.clone());
        }

        if outcome.parse_diagnostics.is_empty() {
            self.parse_diagnostics.remove(&normalized_path);
        } else {
            self.parse_diagnostics
                .insert(normalized_path.clone(), outcome.parse_diagnostics.clone());
        }

        self.file_outcomes.insert(normalized_path, outcome);
    }

    fn remove_file_outcome(&mut self, normalized_path: &Path) {
        if let Some(previous) = self.file_outcomes.remove(normalized_path) {
            for concept_id in previous
                .assertions
                .iter()
                .map(|assertion| &assertion.concept.id)
            {
                if let Some(paths) = self.concept_assertion_files.get_mut(concept_id) {
                    paths.remove(normalized_path);
                    if paths.is_empty() {
                        self.concept_assertion_files.remove(concept_id);
                    }
                }
            }
        }

        self.parse_diagnostics.remove(normalized_path);
    }

    fn file_outcome_concepts(&self, normalized_path: &Path) -> HashSet<String> {
        self.file_outcomes
            .get(normalized_path)
            .map(|outcome| {
                outcome
                    .assertions
                    .iter()
                    .map(|assertion| assertion.concept.id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn recompute_concept_results(&mut self, impacted_concepts: &HashSet<String>) {
        for concept_id in impacted_concepts {
            let assertions = self.assertions_for_concept(concept_id);
            let result = solve::compare_assertions(&self.root, assertions, &self.config)
                .into_iter()
                .find(|concept_result| concept_result.concept.id == *concept_id);

            match result {
                Some(concept_result) => {
                    self.concept_results
                        .insert(concept_id.clone(), concept_result);
                }
                None => {
                    self.concept_results.remove(concept_id);
                }
            }
        }
    }

    fn assertions_for_concept(&self, concept_id: &str) -> Vec<ConfigAssertion> {
        self.concept_assertion_files
            .get(concept_id)
            .into_iter()
            .flat_map(|paths| paths.iter())
            .filter_map(|path| self.file_outcomes.get(path))
            .flat_map(|outcome| {
                outcome
                    .assertions
                    .iter()
                    .filter(|assertion| assertion.concept.id == concept_id)
                    .cloned()
            })
            .collect()
    }
}
