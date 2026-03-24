import * as vscode from "vscode";
import { EXTENSION_ID, CFG_PATH, CFG_SEVERITY, CFG_AUTO_SCAN, CFG_TRACE } from "./constants";

export function getBinaryPath(): string {
  return vscode.workspace.getConfiguration(EXTENSION_ID).get<string>("path", "");
}

export function getSeverity(): string {
  return vscode.workspace.getConfiguration(EXTENSION_ID).get<string>("severity", "warning");
}

export function getAutoScan(): boolean {
  return vscode.workspace.getConfiguration(EXTENSION_ID).get<boolean>("autoScan", true);
}

export function getTrace(): string {
  return vscode.workspace.getConfiguration(EXTENSION_ID).get<string>("trace.server", "off");
}

export function onConfigChange(callback: (e: vscode.ConfigurationChangeEvent) => void): vscode.Disposable {
  return vscode.workspace.onDidChangeConfiguration((e) => {
    if (
      e.affectsConfiguration(CFG_PATH) ||
      e.affectsConfiguration(CFG_SEVERITY) ||
      e.affectsConfiguration(CFG_AUTO_SCAN) ||
      e.affectsConfiguration(CFG_TRACE)
    ) {
      callback(e);
    }
  });
}
