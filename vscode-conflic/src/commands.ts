import * as vscode from "vscode";
import * as cp from "child_process";
import * as path from "path";
import { LanguageClient } from "vscode-languageclient/node";
import {
  CMD_SCAN_WORKSPACE,
  CMD_FIX_ALL,
  CMD_FIX_CONCEPT,
  CMD_SHOW_DOCTOR,
  CMD_INIT_CONFIG,
  CMD_SHOW_TREND,
  CMD_RESTART_SERVER,
  CMD_REVEAL_CONCEPT,
  OUTPUT_CHANNEL_NAME,
} from "./constants";
import type { ConceptTreeDataProvider } from "./conceptTreeView";

export function registerCommands(
  context: vscode.ExtensionContext,
  client: LanguageClient,
  binaryPath: string,
  conceptProvider: ConceptTreeDataProvider
): void {
  const outputChannel = vscode.window.createOutputChannel(OUTPUT_CHANNEL_NAME + " Commands");

  context.subscriptions.push(
    vscode.commands.registerCommand(CMD_SCAN_WORKSPACE, async () => {
      // Restart the LSP server to trigger a full rescan
      await client.restart();
      vscode.window.showInformationMessage("Conflic: Workspace scan triggered.");
    }),

    vscode.commands.registerCommand(CMD_FIX_ALL, async () => {
      const workspaceRoot = getWorkspaceRoot();
      if (!workspaceRoot) return;

      const confirm = await vscode.window.showWarningMessage(
        "Conflic: Apply all auto-fixes? This will modify files in your workspace.",
        { modal: true },
        "Apply Fixes"
      );
      if (confirm !== "Apply Fixes") return;

      await runConflicCommand(binaryPath, ["--fix", "--yes"], workspaceRoot, outputChannel);
      vscode.window.showInformationMessage("Conflic: Fixes applied. Files may have been modified.");
    }),

    vscode.commands.registerCommand(CMD_FIX_CONCEPT, async () => {
      const workspaceRoot = getWorkspaceRoot();
      if (!workspaceRoot) return;

      // Collect concept names from current diagnostics
      const conceptNames = new Set<string>();
      const allDiagnostics = vscode.languages.getDiagnostics();
      for (const [, diagnostics] of allDiagnostics) {
        for (const d of diagnostics) {
          if (d.source === "conflic") {
            const match = d.message.match(/^(.+?):/);
            if (match) conceptNames.add(match[1]);
          }
        }
      }

      if (conceptNames.size === 0) {
        vscode.window.showInformationMessage("Conflic: No contradictions found to fix.");
        return;
      }

      const selected = await vscode.window.showQuickPick(
        Array.from(conceptNames).sort(),
        { placeHolder: "Select a concept to fix" }
      );
      if (!selected) return;

      // Convert display name to concept ID (lowercase, spaces → hyphens)
      const conceptId = selected.toLowerCase().replace(/\s+/g, "-").replace(/\./g, "");

      const confirm = await vscode.window.showWarningMessage(
        `Conflic: Apply fixes for "${selected}"? This will modify files.`,
        { modal: true },
        "Apply"
      );
      if (confirm !== "Apply") return;

      await runConflicCommand(
        binaryPath,
        ["--fix", "--yes", "--concept", conceptId],
        workspaceRoot,
        outputChannel
      );
      vscode.window.showInformationMessage(`Conflic: Fixes applied for "${selected}".`);
    }),

    vscode.commands.registerCommand(CMD_SHOW_DOCTOR, async () => {
      const workspaceRoot = getWorkspaceRoot();
      if (!workspaceRoot) return;

      outputChannel.clear();
      outputChannel.show();
      outputChannel.appendLine("Running conflic --doctor ...\n");
      await runConflicCommand(binaryPath, ["--doctor"], workspaceRoot, outputChannel);
    }),

    vscode.commands.registerCommand(CMD_INIT_CONFIG, async () => {
      const workspaceRoot = getWorkspaceRoot();
      if (!workspaceRoot) return;

      await runConflicCommand(binaryPath, ["--init"], workspaceRoot, outputChannel);
      const configPath = path.join(workspaceRoot, ".conflic.toml");
      try {
        const doc = await vscode.workspace.openTextDocument(configPath);
        await vscode.window.showTextDocument(doc);
      } catch {
        // File may not exist if --init refused (exit code 3)
      }
    }),

    vscode.commands.registerCommand(CMD_SHOW_TREND, async () => {
      const workspaceRoot = getWorkspaceRoot();
      if (!workspaceRoot) return;

      outputChannel.clear();
      outputChannel.show();
      outputChannel.appendLine("Running conflic --trend ...\n");
      await runConflicCommand(binaryPath, ["--trend"], workspaceRoot, outputChannel);
    }),

    vscode.commands.registerCommand(CMD_RESTART_SERVER, async () => {
      await client.restart();
      vscode.window.showInformationMessage("Conflic: Language server restarted.");
    }),

    vscode.commands.registerCommand(CMD_REVEAL_CONCEPT, async (conceptName: string) => {
      // Gather all locations for this concept from diagnostics
      const locations: vscode.Location[] = [];
      const allDiagnostics = vscode.languages.getDiagnostics();
      for (const [uri, diagnostics] of allDiagnostics) {
        for (const d of diagnostics) {
          if (d.source === "conflic" && d.message.startsWith(conceptName + ":")) {
            locations.push(new vscode.Location(uri, d.range));
          }
        }
      }

      if (locations.length === 0) {
        vscode.window.showInformationMessage(`No assertions found for "${conceptName}".`);
        return;
      }

      // Use VS Code's built-in peek/go-to-locations
      if (locations.length === 1) {
        const loc = locations[0];
        const doc = await vscode.workspace.openTextDocument(loc.uri);
        await vscode.window.showTextDocument(doc, { selection: loc.range });
      } else {
        await vscode.commands.executeCommand(
          "editor.action.showReferences",
          locations[0].uri,
          locations[0].range.start,
          locations
        );
      }
    })
  );
}

function getWorkspaceRoot(): string | undefined {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) {
    vscode.window.showErrorMessage("Conflic: No workspace folder open.");
    return undefined;
  }
  return folders[0].uri.fsPath;
}

function runConflicCommand(
  binaryPath: string,
  args: string[],
  cwd: string,
  outputChannel: vscode.OutputChannel
): Promise<void> {
  return new Promise((resolve) => {
    const proc = cp.execFile(binaryPath, args, { cwd, timeout: 60000 }, (error, stdout, stderr) => {
      if (stdout) outputChannel.appendLine(stdout);
      if (stderr) outputChannel.appendLine(stderr);
      if (error && error.code !== null) {
        // conflic exit codes: 0=clean, 1=errors found, 2=warnings found, 3=init refused
        // These are normal operation, not actual errors
        if (typeof error.code === "number" && error.code <= 3) {
          resolve();
          return;
        }
        outputChannel.appendLine(`Error: ${error.message}`);
      }
      resolve();
    });
  });
}
