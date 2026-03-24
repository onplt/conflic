import * as vscode from "vscode";
import * as path from "path";
import { DIAGNOSTIC_SOURCE, VIEW_CONCEPT_OVERVIEW } from "./constants";

// --- Data model ---

interface ConceptInfo {
  displayName: string;
  ruleId: string;
  files: Map<string, FileAssertion[]>;
}

interface FileAssertion {
  uri: vscode.Uri;
  range: vscode.Range;
  value: string;
  authority: string;
  severity: vscode.DiagnosticSeverity | undefined;
}

type TreeItem = ConceptItem | AssertionItem;

class ConceptItem extends vscode.TreeItem {
  constructor(
    public readonly conceptName: string,
    public readonly errorCount: number,
    public readonly warningCount: number,
    public readonly fileCount: number,
  ) {
    super(conceptName, vscode.TreeItemCollapsibleState.Collapsed);
    const hasErrors = errorCount > 0;
    const hasWarnings = warningCount > 0;
    const total = errorCount + warningCount;

    if (hasErrors) {
      this.iconPath = new vscode.ThemeIcon("error", new vscode.ThemeColor("errorForeground"));
      this.description = `${total} contradiction${total > 1 ? "s" : ""} across ${fileCount} file${fileCount > 1 ? "s" : ""}`;
    } else if (hasWarnings) {
      this.iconPath = new vscode.ThemeIcon("warning", new vscode.ThemeColor("list.warningForeground"));
      this.description = `${total} contradiction${total > 1 ? "s" : ""} across ${fileCount} file${fileCount > 1 ? "s" : ""}`;
    } else {
      this.iconPath = new vscode.ThemeIcon("pass", new vscode.ThemeColor("testing.iconPassed"));
      this.description = `consistent across ${fileCount} file${fileCount > 1 ? "s" : ""}`;
    }
  }
}

class AssertionItem extends vscode.TreeItem {
  constructor(
    public readonly uri: vscode.Uri,
    public readonly range: vscode.Range,
    public readonly value: string,
    public readonly authority: string,
    public readonly hasDiagnostic: boolean,
  ) {
    super(path.basename(uri.fsPath), vscode.TreeItemCollapsibleState.None);
    this.description = `${value} (${authority})`;
    this.resourceUri = uri;
    this.command = {
      title: "Open File",
      command: "vscode.open",
      arguments: [uri, { selection: range }],
    };

    if (hasDiagnostic) {
      this.iconPath = new vscode.ThemeIcon("circle-filled", new vscode.ThemeColor("errorForeground"));
    } else {
      this.iconPath = new vscode.ThemeIcon("circle-outline");
    }
    this.contextValue = "conflicAssertion";
  }
}

// --- TreeDataProvider ---

export class ConceptTreeDataProvider implements vscode.TreeDataProvider<TreeItem> {
  private _onDidChangeTreeData = new vscode.EventEmitter<TreeItem | undefined>();
  readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

  /** concept display name → ConceptInfo */
  private concepts = new Map<string, ConceptInfo>();
  /** URI string → diagnostics from last publish */
  private diagnosticsByUri = new Map<string, vscode.Diagnostic[]>();

  private refreshTimer: ReturnType<typeof setTimeout> | undefined;

  /**
   * Called from Language Client middleware on every diagnostics publish.
   * Only receives diagnostics with source === "conflic".
   */
  handleDiagnostics(uri: vscode.Uri, diagnostics: vscode.Diagnostic[]): void {
    const key = uri.toString();
    if (diagnostics.length === 0) {
      this.diagnosticsByUri.delete(key);
    } else {
      this.diagnosticsByUri.set(key, diagnostics);
    }

    // Debounce tree rebuilds — diagnostics arrive per-file in rapid succession
    if (this.refreshTimer) {
      clearTimeout(this.refreshTimer);
    }
    this.refreshTimer = setTimeout(() => {
      this.rebuildTree();
      this._onDidChangeTreeData.fire(undefined);
    }, 100);
  }

  /** Summary counts for status bar consumption. */
  getSummary(): { errors: number; warnings: number; concepts: number } {
    let errors = 0;
    let warnings = 0;
    for (const concept of this.concepts.values()) {
      for (const assertions of concept.files.values()) {
        for (const a of assertions) {
          if (a.severity === vscode.DiagnosticSeverity.Error) errors++;
          if (a.severity === vscode.DiagnosticSeverity.Warning) warnings++;
        }
      }
    }
    return { errors, warnings, concepts: this.concepts.size };
  }

  getTreeItem(element: TreeItem): vscode.TreeItem {
    return element;
  }

  getChildren(element?: TreeItem): TreeItem[] {
    if (!element) {
      // Root level: concepts sorted by error count descending
      return Array.from(this.concepts.entries())
        .sort((a, b) => {
          const aErr = countSeverity(a[1], vscode.DiagnosticSeverity.Error);
          const bErr = countSeverity(b[1], vscode.DiagnosticSeverity.Error);
          if (bErr !== aErr) return bErr - aErr;
          const aWarn = countSeverity(a[1], vscode.DiagnosticSeverity.Warning);
          const bWarn = countSeverity(b[1], vscode.DiagnosticSeverity.Warning);
          return bWarn - aWarn;
        })
        .map(([name, info]) => {
          return new ConceptItem(
            name,
            countSeverity(info, vscode.DiagnosticSeverity.Error),
            countSeverity(info, vscode.DiagnosticSeverity.Warning),
            info.files.size
          );
        });
    }

    if (element instanceof ConceptItem) {
      const info = this.concepts.get(element.conceptName);
      if (!info) return [];
      const items: AssertionItem[] = [];
      for (const assertions of info.files.values()) {
        for (const a of assertions) {
          items.push(
            new AssertionItem(
              a.uri,
              a.range,
              a.value,
              a.authority,
              a.severity !== undefined
            )
          );
        }
      }
      return items;
    }

    return [];
  }

  private rebuildTree(): void {
    this.concepts.clear();

    for (const [uriStr, diagnostics] of this.diagnosticsByUri) {
      const uri = vscode.Uri.parse(uriStr);
      for (const diag of diagnostics) {
        this.processDiagnosticWithUri(uri, diag);
      }
    }
  }

  /**
   * Parse a conflic diagnostic to extract concept/assertion info.
   *
   * Diagnostic message format (from Rust LSP):
   *   "{ConceptDisplayName}: {left_value} conflicts with {right_value} in {filename}"
   *
   * relatedInformation message format:
   *   "{ConceptDisplayName} = {value} ({authority})"
   */
  private processDiagnosticWithUri(uri: vscode.Uri, diag: vscode.Diagnostic): void {
    const msgMatch = diag.message.match(/^(.+?):\s+(.+?)\s+conflicts with\s+(.+?)\s+in\s+(.+)$/);
    if (!msgMatch) return;

    const conceptName = msgMatch[1];
    const localValue = msgMatch[2];
    const ruleId = typeof diag.code === "string" ? diag.code
      : (typeof diag.code === "object" && diag.code !== null && "value" in diag.code)
        ? String(diag.code.value)
        : "";

    let concept = this.concepts.get(conceptName);
    if (!concept) {
      concept = { displayName: conceptName, ruleId, files: new Map() };
      this.concepts.set(conceptName, concept);
    }

    // Add this file's assertion
    const uriKey = uri.toString();
    const existing = concept.files.get(uriKey);
    const localAuthority = this.extractLocalAuthority(diag);
    const assertion: FileAssertion = {
      uri,
      range: diag.range,
      value: localValue,
      authority: localAuthority,
      severity: diag.severity,
    };

    if (existing) {
      // Avoid duplicates for same range
      if (!existing.some((a) => a.range.start.line === diag.range.start.line)) {
        existing.push(assertion);
      }
    } else {
      concept.files.set(uriKey, [assertion]);
    }

    // Add peer assertions from relatedInformation
    const related = diag.relatedInformation;
    if (related) {
      for (const rel of related) {
        const peerMatch = rel.message.match(/^(.+?)\s+=\s+(.+?)\s+\((\w+)\)$/);
        if (peerMatch) {
          const peerUri = rel.location.uri;
          const peerKey = peerUri.toString();
          const peerAssertion: FileAssertion = {
            uri: peerUri,
            range: rel.location.range,
            value: peerMatch[2],
            authority: peerMatch[3],
            severity: undefined,
          };
          const peerExisting = concept.files.get(peerKey);
          if (peerExisting) {
            if (!peerExisting.some((a) => a.range.start.line === rel.location.range.start.line)) {
              peerExisting.push(peerAssertion);
            }
          } else {
            concept.files.set(peerKey, [peerAssertion]);
          }
        }
      }
    }
  }

  /**
   * The diagnostic message tells us about the remote side's value.
   * The relatedInformation tells us the remote side's authority.
   * We don't get the local authority directly, but we can infer from
   * whether this file also appears in another diagnostic's relatedInformation.
   * For simplicity, default to "unknown" if not derivable.
   */
  private extractLocalAuthority(diag: vscode.Diagnostic): string {
    // Check other diagnostics that reference this location in their relatedInformation.
    // This is complex to do across the tree, so we return a placeholder.
    // The authority is mostly decorative in the tree view.
    return "—";
  }
}

function countSeverity(info: ConceptInfo, severity: vscode.DiagnosticSeverity): number {
  let count = 0;
  for (const assertions of info.files.values()) {
    for (const a of assertions) {
      if (a.severity === severity) count++;
    }
  }
  return count;
}
