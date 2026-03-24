import * as vscode from "vscode";
import { LanguageClient } from "vscode-languageclient/node";
import { resolveBinary } from "./binaryResolver";
import { createLanguageClient } from "./client";
import { registerCommands } from "./commands";
import { createStatusBar } from "./statusBar";
import { ConceptTreeDataProvider } from "./conceptTreeView";
import { onConfigChange } from "./configuration";
import { OUTPUT_CHANNEL_NAME, VIEW_CONCEPT_OVERVIEW, CFG_PATH } from "./constants";

let client: LanguageClient | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const binaryPath = await resolveBinary(context);
  if (!binaryPath) {
    return;
  }

  const outputChannel = vscode.window.createOutputChannel(OUTPUT_CHANNEL_NAME);

  // Create sidebar tree view
  const conceptProvider = new ConceptTreeDataProvider();
  const treeView = vscode.window.createTreeView(VIEW_CONCEPT_OVERVIEW, {
    treeDataProvider: conceptProvider,
    showCollapseAll: true,
  });

  // Create and start language client
  client = createLanguageClient(binaryPath, outputChannel, conceptProvider);
  await client.start();
  outputChannel.appendLine(`Conflic LSP started (binary: ${binaryPath})`);

  // Status bar
  const statusBar = createStatusBar(conceptProvider);

  // Commands
  registerCommands(context, client, binaryPath, conceptProvider);

  // Watch for config changes that require server restart
  const configWatcher = onConfigChange(async (e) => {
    if (e.affectsConfiguration(CFG_PATH)) {
      const newPath = await resolveBinary(context);
      if (newPath && client) {
        outputChannel.appendLine(`Conflic binary path changed, restarting server...`);
        await client.stop();
        client = createLanguageClient(newPath, outputChannel, conceptProvider);
        await client.start();
      }
    }
  });

  context.subscriptions.push(
    outputChannel,
    treeView,
    statusBar,
    configWatcher,
    { dispose: () => { client?.stop(); } }
  );
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
