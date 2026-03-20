use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::model::ScanResult;
use crate::{IncrementalScanKind, IncrementalScanStats, IncrementalWorkspace};

use super::code_actions;
use super::diagnostics;
use super::state::{LspState, ScanRequest, ScanTrigger};
use super::text_document::apply_content_changes;

const SCAN_DEBOUNCE_DELAY: Duration = Duration::from_millis(250);
const SCAN_STATS_ENV: &str = "CONFLIC_LSP_SCAN_STATS";

pub struct ConflicLspServer {
    client: Client,
    state: Arc<LspState>,
}

impl ConflicLspServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            state: Arc::new(LspState::new()),
        }
    }

    fn is_workspace_config_path(&self, path: &Path) -> bool {
        self.state.workspace_root().as_ref().is_some_and(|root| {
            path == crate::pathing::normalize_for_workspace(root, Path::new(".conflic.toml"))
        })
    }

    async fn normalize_workspace_document_path(&self, path: PathBuf) -> Option<PathBuf> {
        let root = self.state.workspace_root()?;
        if let Some(normalized) = crate::pathing::normalize_if_within_root(&root, &path) {
            Some(normalized)
        } else {
            self.client
                .log_message(
                    MessageType::WARNING,
                    format!(
                        "Ignored document outside workspace root: {}",
                        path.display()
                    ),
                )
                .await;
            None
        }
    }

    async fn reload_config_and_request_scan(&self, root: &Path, trigger: ScanTrigger) {
        match self.state.reload_config(root) {
            Ok(()) => self.request_full_scan(trigger),
            Err(error) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("Failed to reload conflic config: {}", error),
                    )
                    .await;
            }
        }
    }

    async fn schedule_document_scan(&self, path: PathBuf, trigger: ScanTrigger) {
        if self.is_workspace_config_path(&path) {
            if let Some(root) = self.state.workspace_root() {
                self.reload_config_and_request_scan(&root, ScanTrigger::Immediate)
                    .await;
            }
            return;
        }

        self.request_file_scan(trigger, path);
    }

    fn request_scan(&self, trigger: ScanTrigger, request: ScanRequest) {
        let generation = {
            let mut scheduling = self
                .state
                .scheduling
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let generation = scheduling.register_request(request);

            match trigger {
                ScanTrigger::Immediate => generation,
                ScanTrigger::Debounced => {
                    let state = Arc::clone(&self.state);
                    let client = self.client.clone();
                    let task = tokio::spawn(async move {
                        tokio::time::sleep(SCAN_DEBOUNCE_DELAY).await;
                        ConflicLspServer::enqueue_scan(state, client, generation);
                    });
                    scheduling.arm_debounce(generation, task);
                    return;
                }
            }
        };

        Self::enqueue_scan(Arc::clone(&self.state), self.client.clone(), generation);
    }

    fn request_full_scan(&self, trigger: ScanTrigger) {
        self.request_scan(trigger, ScanRequest::Full);
    }

    fn request_file_scan(&self, trigger: ScanTrigger, path: PathBuf) {
        self.request_scan(trigger, ScanRequest::Files(vec![path]));
    }

    fn enqueue_scan(state: Arc<LspState>, client: Client, generation: u64) {
        let request = {
            let mut scheduling = state
                .scheduling
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let Some(request) = scheduling.prepare_scan(generation) else {
                return;
            };
            request
        };

        tokio::spawn(async move {
            ConflicLspServer::run_scan_generation(state, client, generation, request).await;
        });
    }

    async fn run_scan_generation(
        state: Arc<LspState>,
        client: Client,
        generation: u64,
        request: ScanRequest,
    ) {
        let root = {
            let guard = state
                .root_uri
                .read()
                .unwrap_or_else(|error| error.into_inner());
            guard.as_ref().cloned()
        };
        let Some(root) = root else {
            Self::finish_scan(state, client, generation);
            return;
        };

        if let Err(error) = state.maybe_reload_config_from_disk_if_changed(&root)
            && Self::is_generation_current(&state, generation)
        {
            client
                .log_message(
                    MessageType::ERROR,
                    format!("Failed to refresh conflic config: {}", error),
                )
                .await;
        }

        let config = state
            .config
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let open_documents = state
            .open_documents
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let state_for_scan = Arc::clone(&state);

        let result = tokio::task::spawn_blocking(move || {
            let mut workspace_guard = state_for_scan
                .workspace
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let workspace =
                workspace_guard.get_or_insert_with(|| IncrementalWorkspace::new(&root, &config));

            let scan_result = match request {
                ScanRequest::Full => workspace.full_scan(&open_documents),
                ScanRequest::Files(paths) => workspace.scan_incremental(&paths, &open_documents),
            };
            let stats = workspace.last_stats();

            (scan_result, stats)
        })
        .await;

        match result {
            Ok((scan_result, stats)) => {
                if Self::is_generation_current(&state, generation) {
                    Self::publish_scan_result(&state, &client, generation, scan_result).await;
                    Self::log_scan_stats(&client, &stats).await;
                }
            }
            Err(error) => {
                if Self::is_generation_current(&state, generation) {
                    client
                        .log_message(MessageType::ERROR, format!("Scan task panicked: {}", error))
                        .await;
                }
            }
        }

        Self::finish_scan(state, client, generation);
    }

    async fn publish_scan_result(
        state: &Arc<LspState>,
        client: &Client,
        generation: u64,
        scan_result: ScanResult,
    ) {
        let diagnostics = diagnostics::scan_result_to_diagnostics(&scan_result);
        let new_uris: HashSet<Url> = diagnostics.keys().cloned().collect();

        let stale_uris: Vec<Url> = {
            let previous = state
                .previous_diagnostic_uris
                .read()
                .unwrap_or_else(|error| error.into_inner());
            previous.difference(&new_uris).cloned().collect()
        };

        for uri in stale_uris {
            if !Self::is_generation_current(state, generation) {
                return;
            }
            client.publish_diagnostics(uri, vec![], None).await;
        }

        for (uri, file_diagnostics) in &diagnostics {
            if !Self::is_generation_current(state, generation) {
                return;
            }
            client
                .publish_diagnostics(uri.clone(), file_diagnostics.clone(), None)
                .await;
        }

        if !Self::is_generation_current(state, generation) {
            return;
        }

        *state
            .previous_diagnostic_uris
            .write()
            .unwrap_or_else(|error| error.into_inner()) = new_uris;
        *state
            .scan_result
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(scan_result);
    }

    async fn log_scan_stats(client: &Client, stats: &IncrementalScanStats) {
        if std::env::var_os(SCAN_STATS_ENV).is_none() {
            return;
        }

        let kind = match stats.kind {
            IncrementalScanKind::Full => "full",
            IncrementalScanKind::Incremental => "incremental",
        };
        client
            .log_message(
                MessageType::LOG,
                format!(
                    "conflic-lsp scan kind={} parsed_files={} changed_files={} peer_files={} impacted_concepts={}",
                    kind,
                    stats.parsed_files,
                    stats.changed_files,
                    stats.peer_files,
                    stats.impacted_concepts
                ),
            )
            .await;
    }

    fn is_generation_current(state: &Arc<LspState>, generation: u64) -> bool {
        state
            .scheduling
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_generation_current(generation)
    }

    fn finish_scan(state: Arc<LspState>, client: Client, generation: u64) {
        let next_generation = state
            .scheduling
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .finish_scan(generation);

        if let Some(next_generation) = next_generation {
            Self::enqueue_scan(state, client, next_generation);
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for ConflicLspServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        if let Some(root_uri) = params.root_uri
            && let Ok(path) = root_uri.to_file_path()
        {
            let root = crate::pathing::normalize_root(&path);
            *self
                .state
                .root_uri
                .write()
                .unwrap_or_else(|error| error.into_inner()) = Some(root.clone());

            if let Err(error) = self.state.reload_config(&root) {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("Failed to load conflic config: {}", error),
                    )
                    .await;
            }
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::INCREMENTAL),
                        save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                        ..Default::default()
                    },
                )),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "conflic-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "conflic LSP server initialized")
            .await;

        self.request_full_scan(ScanTrigger::Immediate);
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let Ok(raw_path) = params.text_document.uri.to_file_path() else {
            return;
        };
        let Some(path) = self.normalize_workspace_document_path(raw_path).await else {
            return;
        };

        if let Some(text) = params.text {
            self.state
                .open_documents
                .write()
                .unwrap_or_else(|error| error.into_inner())
                .insert(path.clone(), text);
        }

        self.schedule_document_scan(path, ScanTrigger::Debounced)
            .await;
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let Ok(raw_path) = params.text_document.uri.to_file_path() else {
            return;
        };
        let Some(path) = self.normalize_workspace_document_path(raw_path).await else {
            return;
        };

        self.state
            .open_documents
            .write()
            .unwrap_or_else(|error| error.into_inner())
            .insert(path.clone(), params.text_document.text);

        self.schedule_document_scan(path, ScanTrigger::Debounced)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let Ok(raw_path) = params.text_document.uri.to_file_path() else {
            return;
        };
        let Some(path) = self.normalize_workspace_document_path(raw_path).await else {
            return;
        };
        if params.content_changes.is_empty() {
            return;
        }

        {
            let mut open_documents = self
                .state
                .open_documents
                .write()
                .unwrap_or_else(|error| error.into_inner());
            let document = open_documents
                .entry(path.clone())
                .or_insert_with(|| std::fs::read_to_string(&path).unwrap_or_default());
            apply_content_changes(document, params.content_changes);
        }

        self.schedule_document_scan(path, ScanTrigger::Debounced)
            .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let Ok(raw_path) = params.text_document.uri.to_file_path() else {
            return;
        };
        let Some(path) = self.normalize_workspace_document_path(raw_path).await else {
            return;
        };

        self.state
            .open_documents
            .write()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&path);

        self.schedule_document_scan(path, ScanTrigger::Debounced)
            .await;
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let file_path = match params.text_document.uri.to_file_path() {
            Ok(raw_path) => match self.normalize_workspace_document_path(raw_path).await {
                Some(path) => path,
                None => return Ok(None),
            },
            Err(_) => return Ok(None),
        };

        let scan_result = self
            .state
            .scan_result
            .read()
            .unwrap_or_else(|error| error.into_inner());
        let scan_result = match scan_result.as_ref() {
            Some(result) => result,
            None => return Ok(None),
        };

        let plan = crate::fix::plan_fixes(scan_result);
        let document_text = self
            .state
            .open_documents
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .get(&file_path)
            .cloned();
        let actions = code_actions::proposals_to_code_actions(
            &plan,
            &params.text_document.uri,
            &params.range,
            document_text.as_deref(),
        );

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(
                actions
                    .into_iter()
                    .map(CodeActionOrCommand::CodeAction)
                    .collect(),
            ))
        }
    }
}

/// Start the LSP server on stdin/stdout.
pub async fn run_lsp() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = tower_lsp::LspService::new(ConflicLspServer::new);
    tower_lsp::Server::new(stdin, stdout, socket)
        .serve(service)
        .await;
}
