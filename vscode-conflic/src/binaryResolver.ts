import * as vscode from "vscode";
import * as path from "path";
import * as fs from "fs";
import * as os from "os";
import { execFile } from "child_process";
import { getBinaryPath } from "./configuration";

const PLATFORM_MAP: Record<string, Record<string, string>> = {
  linux: {
    x64: "conflic-linux-x64",
    arm64: "conflic-linux-arm64",
  },
  darwin: {
    x64: "conflic-darwin-x64",
    arm64: "conflic-darwin-arm64",
  },
  win32: {
    x64: "conflic-win32-x64.exe",
  },
};

/**
 * Resolve the conflic binary path.
 * Priority: user setting → bundled binary → PATH lookup → error.
 */
export async function resolveBinary(context: vscode.ExtensionContext): Promise<string | undefined> {
  // 1. User-configured path
  const configuredPath = getBinaryPath();
  if (configuredPath) {
    if (await isExecutable(configuredPath)) {
      return configuredPath;
    }
    vscode.window.showErrorMessage(
      `Conflic binary not found or not executable at configured path: ${configuredPath}`
    );
    return undefined;
  }

  // 2. Bundled binary
  const bundled = bundledBinaryPath(context);
  if (bundled) {
    const exists = await isExecutable(bundled);
    if (exists) {
      return bundled;
    }
  }

  // 3. PATH lookup
  const pathBinary = await findOnPath();
  if (pathBinary) {
    return pathBinary;
  }

  // 4. Not found
  const choice = await vscode.window.showErrorMessage(
    "Conflic binary not found. Install it via `cargo install conflic` or set the path in settings.",
    "Open Settings"
  );
  if (choice === "Open Settings") {
    vscode.commands.executeCommand("workbench.action.openSettings", "conflic.path");
  }
  return undefined;
}

function bundledBinaryPath(context: vscode.ExtensionContext): string | undefined {
  const platformBins = PLATFORM_MAP[process.platform];
  if (!platformBins) {
    return undefined;
  }
  const binaryName = platformBins[process.arch];
  if (!binaryName) {
    return undefined;
  }
  return path.join(context.extensionPath, "bin", binaryName);
}

function isExecutable(filePath: string): Promise<boolean> {
  return new Promise((resolve) => {
    fs.access(filePath, fs.constants.X_OK, (err) => {
      if (err) {
        // On Windows, X_OK is not reliable — check if file exists instead
        if (process.platform === "win32") {
          fs.access(filePath, fs.constants.F_OK, (err2) => resolve(!err2));
        } else {
          resolve(false);
        }
      } else {
        resolve(true);
      }
    });
  });
}

function findOnPath(): Promise<string | undefined> {
  const cmd = process.platform === "win32" ? "where" : "which";
  return new Promise((resolve) => {
    execFile(cmd, ["conflic"], async (error, stdout) => {
      if (error || !stdout.trim()) {
        if (process.platform !== "win32") {
          const fallbackPath = path.join(os.homedir(), ".cargo", "bin", "conflic");
          if (await isExecutable(fallbackPath)) {
            return resolve(fallbackPath);
          }
        }
        resolve(undefined);
      } else {
        resolve(stdout.trim().split(/\r?\n/)[0]);
      }
    });
  });
}
