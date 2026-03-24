import * as vscode from "vscode";
import type { ConceptTreeDataProvider } from "./conceptTreeView";

export function createStatusBar(
  conceptProvider: ConceptTreeDataProvider
): vscode.StatusBarItem {
  const item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
  item.command = "workbench.actions.view.problems";
  item.name = "Conflic";
  updateStatusBar(item, conceptProvider);
  item.show();

  conceptProvider.onDidChangeTreeData(() => {
    updateStatusBar(item, conceptProvider);
  });

  return item;
}

function updateStatusBar(
  item: vscode.StatusBarItem,
  conceptProvider: ConceptTreeDataProvider
): void {
  const { errors, warnings, concepts } = conceptProvider.getSummary();

  if (errors === 0 && warnings === 0) {
    item.text = "$(pass) Conflic: clean";
    item.backgroundColor = undefined;
    item.tooltip = concepts > 0
      ? `Conflic — ${concepts} concept${concepts > 1 ? "s" : ""} checked, no contradictions`
      : "Conflic — no contradictions detected";
  } else {
    const parts: string[] = [];
    if (errors > 0) parts.push(`$(error) ${errors}`);
    if (warnings > 0) parts.push(`$(warning) ${warnings}`);
    item.text = `Conflic: ${parts.join(" ")}`;
    item.backgroundColor = errors > 0
      ? new vscode.ThemeColor("statusBarItem.errorBackground")
      : new vscode.ThemeColor("statusBarItem.warningBackground");
    item.tooltip = `Conflic — ${errors} error${errors !== 1 ? "s" : ""}, ${warnings} warning${warnings !== 1 ? "s" : ""} across ${concepts} concept${concepts !== 1 ? "s" : ""}`;
  }
}
