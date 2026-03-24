import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  Executable,
} from "vscode-languageclient/node";
import { LANGUAGE_CLIENT_ID, LANGUAGE_CLIENT_NAME, DIAGNOSTIC_SOURCE } from "./constants";
import type { ConceptTreeDataProvider } from "./conceptTreeView";

let client: LanguageClient | undefined;

export function createLanguageClient(
  binaryPath: string,
  outputChannel: vscode.OutputChannel,
  conceptProvider: ConceptTreeDataProvider
): LanguageClient {
  const serverOptions: Executable = {
    command: binaryPath,
    args: ["--lsp"],
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { language: "conflic-toml" },
      { language: "dockerfile" },
      { language: "json" },
      { language: "jsonc" },
      { language: "yaml" },
      { language: "toml" },
      { language: "properties" },
      { language: "plaintext", pattern: "**/.nvmrc" },
      { language: "plaintext", pattern: "**/.node-version" },
      { language: "plaintext", pattern: "**/.python-version" },
      { language: "plaintext", pattern: "**/.ruby-version" },
      { language: "plaintext", pattern: "**/.tool-versions" },
      { language: "plaintext", pattern: "**/.go-version" },
      { language: "plaintext", pattern: "**/.sdkmanrc" },
      { language: "xml", pattern: "**/*.csproj" },
      { language: "xml", pattern: "**/pom.xml" },
      { language: "xml", pattern: "**/global.json" },
      { language: "go", pattern: "**/go.mod" },
      { language: "ruby", pattern: "**/Gemfile" },
    ],
    outputChannel,
    middleware: {
      handleDiagnostics(uri, diagnostics, next) {
        // Forward all diagnostics to VS Code
        next(uri, diagnostics);

        // Feed conflic-specific diagnostics to the concept tree
        const conflicDiagnostics = diagnostics.filter(
          (d) => d.source === DIAGNOSTIC_SOURCE
        );
        conceptProvider.handleDiagnostics(uri, conflicDiagnostics);
      },
      provideDocumentSymbols() {
        // Disabled: server's DocumentSymbol response triggers a
        // protocolConverter error in vscode-languageclient.
        // Diagnostics, hover, code actions and references all work fine.
        return undefined;
      },
    },
  };

  client = new LanguageClient(
    LANGUAGE_CLIENT_ID,
    LANGUAGE_CLIENT_NAME,
    serverOptions,
    clientOptions
  );

  return client;
}

export function getClient(): LanguageClient | undefined {
  return client;
}
