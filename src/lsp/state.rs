use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};
use std::time::SystemTime;

use tokio::task::JoinHandle;
use tower_lsp::lsp_types::Url;

use crate::IncrementalWorkspace;
use crate::config::ConflicConfig;
use crate::model::ScanResult;

pub(super) struct LspState {
    pub(super) root_uri: RwLock<Option<PathBuf>>,
    pub(super) config: RwLock<ConflicConfig>,
    pub(super) config_modified: RwLock<Option<SystemTime>>,
    pub(super) scan_result: RwLock<Option<ScanResult>>,
    pub(super) workspace: Mutex<Option<IncrementalWorkspace>>,
    pub(super) open_documents: RwLock<HashMap<PathBuf, String>>,
    pub(super) previous_diagnostic_uris: RwLock<HashSet<Url>>,
    pub(super) scheduling: Mutex<ScanScheduler>,
}

impl LspState {
    pub(super) fn new() -> Self {
        Self {
            root_uri: RwLock::new(None),
            config: RwLock::new(ConflicConfig::default()),
            config_modified: RwLock::new(None),
            scan_result: RwLock::new(None),
            workspace: Mutex::new(None),
            open_documents: RwLock::new(HashMap::new()),
            previous_diagnostic_uris: RwLock::new(HashSet::new()),
            scheduling: Mutex::new(ScanScheduler::default()),
        }
    }

    pub(super) fn workspace_root(&self) -> Option<PathBuf> {
        self.root_uri
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub(super) fn reload_config(&self, root: &Path) -> std::result::Result<(), String> {
        let config_path = config_path_for_root(root);
        let open_config = self
            .open_documents
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .get(&config_path)
            .cloned();
        let modified = config_modified_time(&config_path);

        let config = match open_config {
            Some(content) => {
                ConflicConfig::load_from_content(root, None, &content).map_err(|e| e.to_string())?
            }
            None => ConflicConfig::load(root, None).map_err(|e| e.to_string())?,
        };

        *self
            .config
            .write()
            .unwrap_or_else(|error| error.into_inner()) = config;
        *self
            .config_modified
            .write()
            .unwrap_or_else(|error| error.into_inner()) = modified;
        *self
            .workspace
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = None;

        Ok(())
    }

    pub(super) fn maybe_reload_config_from_disk_if_changed(
        &self,
        root: &Path,
    ) -> std::result::Result<bool, String> {
        let config_path = config_path_for_root(root);
        if self
            .open_documents
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .contains_key(&config_path)
        {
            return Ok(false);
        }

        let current_modified = config_modified_time(&config_path);
        let previous_modified = *self
            .config_modified
            .read()
            .unwrap_or_else(|error| error.into_inner());

        if current_modified == previous_modified {
            return Ok(false);
        }

        let config = ConflicConfig::load(root, None).map_err(|e| e.to_string())?;
        *self
            .config
            .write()
            .unwrap_or_else(|error| error.into_inner()) = config;
        *self
            .config_modified
            .write()
            .unwrap_or_else(|error| error.into_inner()) = current_modified;
        *self
            .workspace
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = None;

        Ok(true)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ScanRequest {
    Full,
    Files(Vec<PathBuf>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScanTrigger {
    Immediate,
    Debounced,
}

#[derive(Default)]
pub(super) struct ScanScheduler {
    next_generation: u64,
    latest_requested_generation: u64,
    pending_generation: Option<u64>,
    execution: ScanExecutionState,
    debounce: DebounceState,
    scheduled_scope: PendingScanScope,
}

impl ScanScheduler {
    pub(super) fn register_request(&mut self, request: ScanRequest) -> u64 {
        self.next_generation += 1;
        let generation = self.next_generation;
        self.latest_requested_generation = generation;
        self.scheduled_scope.merge(request);
        self.cancel_debounce();
        generation
    }

    pub(super) fn arm_debounce(&mut self, generation: u64, task: JoinHandle<()>) {
        self.debounce = DebounceState::Scheduled { generation, task };
    }

    pub(super) fn prepare_scan(&mut self, generation: u64) -> Option<ScanRequest> {
        self.clear_debounce_if_matching(generation);

        if generation < self.latest_requested_generation {
            return None;
        }

        if self.execution.running_generation().is_some() {
            self.pending_generation = Some(generation);
            return None;
        }

        let request = self.scheduled_scope.take_request()?;
        self.execution = ScanExecutionState::Running { generation };
        Some(request)
    }

    pub(super) fn finish_scan(&mut self, generation: u64) -> Option<u64> {
        if self.execution.running_generation() == Some(generation) {
            self.execution = ScanExecutionState::Idle;
        }

        if self.scheduled_scope.is_empty() {
            self.pending_generation = None;
            return None;
        }

        let next_generation = self
            .pending_generation
            .take()
            .unwrap_or(self.latest_requested_generation);
        Some(if next_generation > generation {
            next_generation
        } else {
            self.latest_requested_generation
        })
    }

    pub(super) fn is_generation_current(&self, generation: u64) -> bool {
        generation == self.latest_requested_generation
    }

    fn cancel_debounce(&mut self) {
        if let DebounceState::Scheduled { task, .. } = std::mem::take(&mut self.debounce) {
            task.abort();
        }
    }

    fn clear_debounce_if_matching(&mut self, generation: u64) {
        let should_clear = matches!(self.debounce, DebounceState::Scheduled { generation: scheduled, .. } if scheduled == generation);
        if should_clear {
            self.debounce = DebounceState::Idle;
        }
    }
}

#[derive(Default)]
struct PendingScanScope {
    full_scan: bool,
    changed_files: HashSet<PathBuf>,
}

impl PendingScanScope {
    fn merge(&mut self, request: ScanRequest) {
        match request {
            ScanRequest::Full => {
                self.full_scan = true;
                self.changed_files.clear();
            }
            ScanRequest::Files(paths) => {
                if !self.full_scan {
                    self.changed_files.extend(paths);
                }
            }
        }
    }

    fn take_request(&mut self) -> Option<ScanRequest> {
        if self.full_scan {
            self.full_scan = false;
            self.changed_files.clear();
            return Some(ScanRequest::Full);
        }

        if self.changed_files.is_empty() {
            None
        } else {
            Some(ScanRequest::Files(
                std::mem::take(&mut self.changed_files)
                    .into_iter()
                    .collect(),
            ))
        }
    }

    fn is_empty(&self) -> bool {
        !self.full_scan && self.changed_files.is_empty()
    }
}

#[derive(Default)]
enum ScanExecutionState {
    #[default]
    Idle,
    Running {
        generation: u64,
    },
}

impl ScanExecutionState {
    fn running_generation(&self) -> Option<u64> {
        match self {
            Self::Idle => None,
            Self::Running { generation } => Some(*generation),
        }
    }
}

#[derive(Default)]
enum DebounceState {
    #[default]
    Idle,
    Scheduled {
        generation: u64,
        task: JoinHandle<()>,
    },
}

fn config_path_for_root(root: &Path) -> PathBuf {
    crate::pathing::normalize_for_workspace(root, Path::new(".conflic.toml"))
}

fn config_modified_time(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_request_overrides_pending_file_scope() {
        let mut scheduler = ScanScheduler::default();
        scheduler.register_request(ScanRequest::Files(vec![PathBuf::from("a.txt")]));
        let generation = scheduler.register_request(ScanRequest::Full);

        assert_eq!(scheduler.prepare_scan(generation), Some(ScanRequest::Full));
    }

    #[test]
    fn pending_generation_is_replayed_after_running_scan_finishes() {
        let mut scheduler = ScanScheduler::default();
        let first_generation = scheduler.register_request(ScanRequest::Full);
        assert_eq!(
            scheduler.prepare_scan(first_generation),
            Some(ScanRequest::Full)
        );

        let second_generation =
            scheduler.register_request(ScanRequest::Files(vec![PathBuf::from("changed.txt")]));
        assert_eq!(scheduler.prepare_scan(second_generation), None);
        assert_eq!(
            scheduler.finish_scan(first_generation),
            Some(second_generation)
        );
    }
}
