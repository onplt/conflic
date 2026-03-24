# Conflic — Config Contradiction Detector

> Your `.nvmrc` says Node 20. Your `Dockerfile` pulls `node:22-alpine`. Your `package.json` engines field requires `>=18 <20`. **Which one is actually running in production?**

If you've ever wasted hours debugging a deployment only to realize two config files disagreed about a version number, Conflic is for you. It scans your workspace, finds every place a version, port, or setting is declared, and tells you when they contradict each other — right inside your editor.

No more "works on my machine." No more silent config drift.

---

## What does it look like?

When Conflic detects a contradiction, you'll see it immediately:

- **Red/yellow squiggles** on the exact line where the conflict lives
- **Inline hints** explaining what the conflict is and where the other value lives
- A **sidebar panel** with a bird's-eye view of every concept in your project
- A **status bar counter** showing how many issues remain

For example, open a project with a `Dockerfile` that says `FROM node:22-alpine` and a `.nvmrc` that says `20` — Conflic will underline both and explain the mismatch.

---

## Getting started

1. Install the extension from the Marketplace
2. Open any project that has config files (Dockerfiles, package.json, .env, docker-compose, CI workflows, etc.)
3. That's it. Conflic activates automatically and starts scanning.

No configuration needed. It works out of the box.

If you want to customize behavior (suppress known issues, add custom extractors, enforce team policies), drop a `.conflic.toml` in your project root. More on that [below](#configuration).

---

## What Conflic understands

Conflic ships with **35 built-in extractors** that know how to read version pins, port declarations, and settings from common config formats:

| What | Where it looks |
|------|----------------|
| **Node.js version** | `.nvmrc`, `.node-version`, `package.json`, Dockerfile, `.tool-versions`, GitHub/GitLab/CircleCI workflows |
| **Python version** | `.python-version`, `pyproject.toml`, Dockerfile, CI workflows |
| **Go version** | `go.mod`, Dockerfile |
| **Java version** | `pom.xml`, Dockerfile (OpenJDK, Temurin, Corretto, Semeru), `.sdkmanrc`, `.tool-versions`, CI workflows |
| **Ruby version** | `.ruby-version`, `Gemfile`, Dockerfile, `.tool-versions`, CI workflows |
| **.NET version** | `*.csproj`, `global.json`, Dockerfile |
| **Application ports** | `.env`, `docker-compose.yml`, Dockerfile `EXPOSE` |
| **TypeScript strict mode** | `tsconfig.json`, ESLint configs (legacy & flat) |
| **IaC (Terraform, K8s, Helm)** | `*.tf`, `deployment.yaml`, `values.yaml`, and more |

It doesn't just do string comparison either. `node:22-alpine`, `^22`, `>=22 <23`, and `22.0.0` are all recognized as the same major version thanks to semver-aware logic.

---

## How severity works

Not every contradiction is equally bad. Conflic uses an **authority model** to decide how loud to yell:

| Authority | What it means | Examples |
|-----------|---------------|----------|
| **Enforced** | This is what actually runs. If it's wrong, things break. | Dockerfile `FROM`, CI matrix versions, docker-compose ports |
| **Declared** | Should match reality, but nothing mechanically enforces it. | `package.json` engines, `pyproject.toml`, `.env` values |
| **Advisory** | Nice to keep in sync, but won't cause a build failure. | `.nvmrc`, `.python-version`, `.tool-versions` |

The severity of a diagnostic depends on which authority levels are fighting:

| Conflict between | You'll see |
|------------------|------------|
| Enforced vs Enforced | Error (red) |
| Enforced vs Declared | Error (red) |
| Enforced vs Advisory | Warning (yellow) |
| Declared vs Declared | Warning (yellow) |
| Declared vs Advisory | Info (blue) |
| Advisory vs Advisory | Info (blue) |

---

## Quick Fix

See a lightbulb icon on a contradiction? Click it (or press `Ctrl+.`). Conflic will offer to fix the value automatically.

The fix logic is simple: **the highest-authority source wins**. If your Dockerfile says `node:22` (enforced) and `.nvmrc` says `20` (advisory), the quick fix updates `.nvmrc` to `22`. If two enforced sources disagree with each other, Conflic marks it as unfixable — that's a decision only you can make.

---

## Hover for context

Hover over any value that Conflic recognizes and you'll see:

- The **concept name** (e.g., "Node.js Version")
- The **authority level** of this particular declaration
- Every **other file** in your project that declares the same concept
- Whether there's a **contradiction** and with whom

This is especially useful in large projects where you might not even know that three different files all have opinions about your Python version.

---

## Sidebar: Concept Overview

The Conflic sidebar (click the icon in the Activity Bar) gives you a tree view of every concept detected in your workspace:

- Concepts with contradictions are marked with warning/error icons
- Expand a concept to see every file that contributes a value
- Click any item to jump straight to the relevant line

It's like a table of contents for your project's configuration.

---

## Commands

Open the Command Palette (`Ctrl+Shift+P`) and type "Conflic":

| Command | What it does |
|---------|--------------|
| **Scan Workspace** | Force a full rescan (usually automatic, but useful after big refactors) |
| **Fix All Contradictions** | Apply auto-fixes across the entire workspace |
| **Fix Concept...** | Pick a single concept to fix |
| **Show Doctor Report** | See which files were discovered, which extractors ran, and any parse errors |
| **Initialize .conflic.toml** | Generate a starter config file in your project root |
| **Show Trend History** | View how your contradiction count has changed over time |
| **Restart Language Server** | If something feels off, restart the LSP backend |

---

## Settings

| Setting | Default | What it controls |
|---------|---------|------------------|
| `conflic.path` | `""` | Path to the conflic binary. Leave empty to use the bundled one or auto-detect from `PATH`. |
| `conflic.severity` | `"warning"` | Minimum severity to show. Set to `"error"` if you only care about the serious stuff. |
| `conflic.autoScan` | `true` | Scan automatically on file save. Turn off for manual-only scanning. |
| `conflic.trace.server` | `"off"` | LSP trace level. Set to `"verbose"` when filing bug reports. |

---

## Configuration

Create a `.conflic.toml` in your project root for fine-grained control:

```toml
[conflic]
severity = "warning"
exclude = ["vendor/**", "node_modules/**"]

# Suppress a known contradiction you've already reviewed
[[ignore]]
rule = "VER001"
files = ["Dockerfile", ".nvmrc"]
reason = "Multi-stage build — final stage matches .nvmrc"

# Enforce team standards
[[policy]]
id = "POL001"
concept = "node-version"
rule = ">= 20"
severity = "error"
message = "Node 18 is EOL. All services must use Node 20+."
```

### Custom extractors

Tracking something Conflic doesn't know about? Define your own:

```toml
[[custom_extractor]]
concept = "redis-version"
display_name = "Redis Version"
category = "runtime-version"
type = "version"
solver = "semver"

[[custom_extractor.source]]
file = "docker-compose.yml"
format = "yaml"
path = "services.redis.image"
pattern = "redis:(.*)"
authority = "enforced"

[[custom_extractor.source]]
file = ".env"
format = "env"
key = "REDIS_VERSION"
authority = "declared"
```

Now Conflic will flag it if your `docker-compose.yml` pulls `redis:7` but `.env` says `REDIS_VERSION=6.2`.

---

## Rule IDs

When you see a diagnostic, the code tells you what kind of contradiction it is:

| Code | Meaning |
|------|---------|
| `VER001` | Version mismatch |
| `PORT001` | Port conflict |
| `BOOL001` | Boolean disagreement (e.g., strict mode on vs off) |
| `STR001` | String value mismatch |
| `POL*` | Policy violation |
| `PARSE001` | Couldn't read or parse a file |
| `PARSE002` | Failed to resolve an `extends` chain (tsconfig, eslint) |

---

## Troubleshooting

**Extension installed but no diagnostics appear?**

1. Check the Output panel: `View` → `Output` → select "Conflic" from the dropdown
2. Make sure the binary is accessible: run `conflic --help` in your terminal, or set `conflic.path` in settings
3. The extension only activates when your workspace contains known config files (Dockerfile, package.json, .env, etc.)

**Too many diagnostics?**

- Raise the severity threshold: set `conflic.severity` to `"error"` in settings
- Suppress specific findings with `[[ignore]]` blocks in `.conflic.toml`
- Use `conflic.autoScan: false` to switch to manual scanning only

**LSP server crashes?**

- Check the Output panel for error details
- Try `Ctrl+Shift+P` → "Conflic: Restart Language Server"
- If the issue persists, set `conflic.trace.server` to `"verbose"` and [open an issue](https://github.com/onplt/conflic/issues) with the log

---

## Works great with

- **GitHub Actions**: Use the [conflic GitHub Action](https://github.com/onplt/conflic#github-action) to catch contradictions in CI before they reach production
- **SARIF**: Conflic can output SARIF for integration with GitHub Code Scanning
- **Monorepos**: Built-in support for per-package scanning with shared global concepts

---

## Platform support

The extension bundles the conflic binary for **Windows x64**. On other platforms (macOS, Linux), install the CLI first:

```bash
cargo install conflic
```

The extension will automatically find it on your `PATH`.

---

## Links

- [GitHub](https://github.com/onplt/conflic) — source code, issues, and full CLI documentation
- [Changelog](https://github.com/onplt/conflic/releases) — what's new in each release
- [crates.io](https://crates.io/crates/conflic) — Rust crate page

---

## License

[MIT](https://github.com/onplt/conflic/blob/main/LICENSE)
