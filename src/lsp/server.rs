use std::path::PathBuf;
use std::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::config::ConflicConfig;
use crate::model::ScanResult;

use super::code_actions;
use super::diagnostics;

pub struct ConflicLspServer {
    client: Client,
    root_uri: RwLock<Option<PathBuf>>,
    config: RwLock<ConflicConfig>,
    scan_result: RwLock<Option<ScanResult>>,
}

impl ConflicLspServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            root_uri: RwLock::new(None),
            config: RwLock::new(ConflicConfig::default()),
            scan_result: RwLock::new(None),
        }
    }

    async fn run_scan(&self) {
        let root = {
            let guard = self.root_uri.read().unwrap();
            match guard.as_ref() {
                Some(r) => r.clone(),
                None => return,
            }
        };

        let config = {
            self.config.read().unwrap().clone()
        };

        // Run scan in a blocking task since our pipeline uses rayon
        let result = tokio::task::spawn_blocking(move || {
            crate::scan(&root, &config)
        })
        .await;

        match result {
            Ok(Ok(scan_result)) => {
                let diags = diagnostics::scan_result_to_diagnostics(&scan_result);

                // Publish diagnostics for each file
                for (uri, file_diags) in &diags {
                    self.client
                        .publish_diagnostics(uri.clone(), file_diags.clone(), None)
                        .await;
                }

                // Clear diagnostics for files that no longer have issues
                // (we'd need to track previous URIs for this — skip for now)

                *self.scan_result.write().unwrap() = Some(scan_result);
            }
            Ok(Err(e)) => {
                self.client
                    .log_message(MessageType::ERROR, format!("Scan failed: {}", e))
                    .await;
            }
            Err(e) => {
                self.client
                    .log_message(MessageType::ERROR, format!("Scan task panicked: {}", e))
                    .await;
            }
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for ConflicLspServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Store root URI
        if let Some(root_uri) = params.root_uri {
            if let Ok(path) = root_uri.to_file_path() {
                *self.root_uri.write().unwrap() = Some(path.clone());

                // Load config from workspace root
                if let Ok(config) = ConflicConfig::load(&path, None) {
                    *self.config.write().unwrap() = config;
                }
            }
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
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

        // Run initial scan
        self.run_scan().await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_save(&self, _params: DidSaveTextDocumentParams) {
        // Re-scan on save
        self.run_scan().await;
    }

    async fn did_open(&self, _params: DidOpenTextDocumentParams) {
        // Could trigger scan for just this file, but full scan is fast enough
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let scan_result = self.scan_result.read().unwrap();
        let scan_result = match scan_result.as_ref() {
            Some(r) => r,
            None => return Ok(None),
        };

        let plan = crate::fix::plan_fixes(scan_result);
        let actions =
            code_actions::proposals_to_code_actions(&plan, &params.text_document.uri, &params.range);

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions.into_iter().map(CodeActionOrCommand::CodeAction).collect()))
        }
    }
}

/// Start the LSP server on stdin/stdout.
pub async fn run_lsp() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = tower_lsp::LspService::new(|client| ConflicLspServer::new(client));
    tower_lsp::Server::new(stdin, stdout, socket)
        .serve(service)
        .await;
}
