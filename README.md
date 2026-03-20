<p align="center">
  <img src="./static/logo/conflic-scan.svg" alt="conflic scanning animation" width="800">
</p>

<p align="center">
  <em>Detect semantic contradictions across config files</em>
</p>

<p align="center">
  <a href="https://github.com/onplt/conflic/actions/workflows/ci.yml"><img src="https://github.com/onplt/conflic/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://crates.io/crates/conflic"><img src="https://img.shields.io/crates/v/conflic.svg" alt="crates.io"></a>
  <a href="https://github.com/onplt/conflic/blob/main/LICENSE"><img src="https://img.shields.io/crates/l/conflic.svg" alt="License: MIT"></a>
  <a href="https://github.com/onplt/conflic/releases"><img src="https://img.shields.io/github/v/release/onplt/conflic" alt="GitHub Release"></a>
</p>

---

`conflic` scans a directory tree, extracts semantic assertions from configuration files, and reports contradictions between them.

The current implementation ships with:

- 29 built-in extractors across 8 built-in concepts
- custom extractors loaded from `.conflic.toml`
- terminal, JSON, and SARIF output
- diff-scoped scans
- baselines for suppressing known findings
- fix planning and safe auto-fix for supported targets
- an optional LSP server with incremental rescans and quick-fix code actions

## What `conflic` understands today

Recognized CI runtime settings come from YAML files under:

- `.github/workflows`
- `.circleci`
- `.gitlab-ci`
- the repository root file `.gitlab-ci.yml`

Built-in concepts and their current sources:

| Concept ID | Display name | Current sources |
| --- | --- | --- |
| `node-version` | Node.js Version | `.nvmrc`, `.node-version`, `package.json` `engines.node`, `Dockerfile*` `FROM node:*`, recognized CI YAML `node-version` / `node_version`, `.tool-versions` entries for `nodejs` or `node` |
| `python-version` | Python Version | `.python-version`, `pyproject.toml` `project.requires-python`, `pyproject.toml` `tool.poetry.dependencies.python`, `Dockerfile*` `FROM python:*`, recognized CI YAML `python-version` / `python_version` |
| `go-version` | Go Version | `go.mod` `go` directive, `Dockerfile*` `FROM golang:*` |
| `java-version` | Java Version | `pom.xml` tags `maven.compiler.source`, `maven.compiler.target`, `java.version`, or `release`, `Dockerfile*` `FROM openjdk:*`, `eclipse-temurin:*`, `amazoncorretto:*`, or `ibm-semeru-runtimes:*`, `.sdkmanrc` `java=...`, `.tool-versions` `java`, recognized CI YAML `java-version` / `java_version` |
| `ruby-version` | Ruby Version | `.ruby-version`, `Gemfile` `ruby "..."`, `Dockerfile*` `FROM ruby:*`, `.tool-versions` `ruby`, recognized CI YAML `ruby-version` / `ruby_version` |
| `dotnet-version` | .NET Version | `*.csproj` `TargetFramework` and `TargetFrameworks`, `global.json` `sdk.version`, `Dockerfile*` `FROM mcr.microsoft.com/dotnet/{sdk,aspnet,runtime}:...` |
| `app-port` | Application Port | `.env` and `.env.*` keys `PORT`, `APP_PORT`, or `SERVER_PORT`, `docker-compose*.yml` / `docker-compose*.yaml` service ports, `Dockerfile*` `EXPOSE` |
| `ts-strict-mode` | TypeScript Strict Mode | `tsconfig*.json` `compilerOptions.strict`, plus ESLint configs that explicitly turn off `@typescript-eslint/strict-boolean-expressions`, `@typescript-eslint/strict-type-checked`, or `@typescript-eslint/no-explicit-any` |

Important details behind those sources:

- `Dockerfile*` means `Dockerfile` plus filename variants such as `Dockerfile.dev`.
- `.env*` means `.env` plus variants such as `.env.local`.
- `docker-compose*.yml` / `.yaml` includes variants such as `docker-compose.override.yaml`.
- `tsconfig*.json` includes files like `tsconfig.app.json`.
- ESLint files currently recognized are `.eslintrc`, `.eslintrc.json`, `.eslintrc.yml`, `.eslintrc.yaml`, and any file named `eslint.config.*`.

## How contradictions are evaluated

`conflic` compares extracted values using concept-aware semantics:

- Versions understand exact semver values, partial versions like `20` or `3.12`, npm-style ranges like `^20` or `>=18 <20`, and Docker tags like `22-alpine`.
- Ports understand single ports, ranges like `3000-3005`, and Docker-style mappings like `3000:8080`. For mappings, the container port is treated as the application port.
- Boolean concepts compare literal `true` / `false`.
- String concepts compare exact string equality.
- Custom extractors can opt into `version`, `port`, `boolean`, or plain string semantics.

Each assertion also carries an authority level:

- `advisory`: informational sources such as `.nvmrc`, `.node-version`, `.python-version`, `.ruby-version`, `.tool-versions`, `.sdkmanrc`, and non-final Docker build stages
- `declared`: project declarations such as `package.json`, `pyproject.toml`, `Gemfile`, `go.mod`, `.env`, `pom.xml`, `*.csproj`, and `Dockerfile EXPOSE`
- `enforced`: hard constraints such as final Docker runtime images, CI runtime settings, `docker-compose` ports, `global.json`, `tsconfig` strict mode, and ESLint strict-related rules that are turned off

Current severity mapping:

| Authority pair | Result |
| --- | --- |
| `enforced` + `enforced` | `error` |
| `enforced` + `declared` | `error` |
| `enforced` + `advisory` | `warning` |
| `declared` + `declared` | `warning` |
| `declared` + `advisory` | `info` |
| `advisory` + `advisory` | `info` |

Important current behavior:

- `--severity` and `[conflic].severity` affect exit codes and `--quiet`.
- They do not currently filter lower-severity findings out of terminal, JSON, or SARIF output.

## Installation

Requirements:

- Rust 1.94 or newer

From crates.io:

```bash
cargo install conflic
```

From source:

```bash
git clone https://github.com/conflic/conflic.git
cd conflic
cargo install --path .
```

Without the LSP server:

```bash
cargo install conflic --no-default-features
```

## Quick start

```bash
# Scan the current directory
conflic

# Scan a specific path
conflic path/to/workspace

# Create a starter config in that path
conflic path/to/workspace --init

# List built-in extractor IDs and descriptions
conflic --list-concepts

# Keep only selected concepts in the normal scan output
conflic --check node,python

# Emit machine-readable JSON
conflic --format json

# Emit SARIF
conflic --format sarif > conflic.sarif

# Show discovery, extractor, assertion, and comparison details
conflic --doctor

# Scope the scan to changes since a git ref
conflic --diff origin/main

# Or pass changed paths on stdin, one path per line
git diff --name-only origin/main | conflic --diff-stdin

# Create or update a baseline
conflic --update-baseline .conflic-baseline.json

# Suppress findings already present in that baseline
conflic --baseline .conflic-baseline.json

# Preview fix proposals without writing anything
conflic --fix --dry-run
```

## Discovery and parsing

Current implementation details that matter in practice:

- Discovery respects `.gitignore`, `.git/info/exclude`, and global Git ignore files.
- The walker always skips these directories: `node_modules`, `.git`, `vendor`, `target`, `dist`, `build`, `__pycache__`, `.tox`, `.venv`, and `venv`.
- `[conflic].exclude` can add extra exclusions as simple path segments, exact path prefixes, or glob patterns.
- JSON files are parsed as strict JSON first, then JSON5 as a fallback. This means comments, trailing commas, single-quoted strings, and unquoted keys are accepted when the JSON5 parser can handle them.
- Extensionless `.eslintrc` files are tried as JSON/JSON5 first, then YAML.
- YAML parsing supports anchors and merge keys.
- `tsconfig*.json` and structured ESLint configs resolve local `extends` chains with cycle detection.
- Local `extends` targets are blocked if they resolve outside the scan root. Those cases are surfaced as `PARSE002`.
- Missing local config references such as `tsconfig.base` also surface as `PARSE002`.
- `eslint.config.*` files are parsed, not executed. Current support is for exported object/array literals that are JSON5-like, optionally wrapped in `defineConfig(...)`, `tseslint.config(...)`, `typescriptEslint.config(...)`, or `eslint.config(...)`.
- Parse and configuration diagnostics are preserved in terminal output, JSON output, SARIF output, baselines, and LSP diagnostics.

## Configuration

By default `conflic` looks for `.conflic.toml` in the scan root.

- `conflic --init [PATH]` writes a starter file to `PATH/.conflic.toml`
- `--config` overrides the config file path
- relative `--config` paths are resolved from the scan root, not from the shell working directory
- a missing implicit config is fine
- a missing explicit `--config` path is an error

Example:

```toml
[conflic]
severity = "warning"
format = "terminal"
exclude = []
skip_concepts = []

# [[ignore]]
# rule = "VER001"
# files = ["Dockerfile", ".nvmrc"]
# reason = "Intentional drift"

# [monorepo]
# per_package = true
# package_roots = ["packages/*", "apps/*"]
# global_concepts = ["node-version", "ts-strict-mode"]

# [[custom_extractor]]
# concept = "redis-version"
# display_name = "Redis Version"
# category = "runtime-version"
# type = "version"
#
# [[custom_extractor.source]]
# file = "docker-compose.yml"
# format = "yaml"
# path = "services.redis.image"
# pattern = "redis:(.*)"
# authority = "enforced"
#
# [[custom_extractor.source]]
# file = ".env"
# format = "env"
# key = "REDIS_VERSION"
# authority = "declared"
```

Config fields:

- `[conflic].severity`: `error`, `warning`, or `info`
- `[conflic].format`: `terminal`, `json`, or `sarif`
- `[conflic].exclude`: extra names, path prefixes, or glob patterns to skip during discovery
- `[conflic].skip_concepts`: concepts to drop before reporting; full IDs and built-in aliases are accepted
- `[[ignore]]`: contradiction-only suppression rules
- `[monorepo]`: package scoping controls
- `[[custom_extractor]]`: custom concept definitions

Built-in selector aliases accepted by:

- `--check`
- `[conflic].skip_concepts`
- `--concept` in fix mode

| Alias | Concept ID |
| --- | --- |
| `node` | `node-version` |
| `python` | `python-version` |
| `go` | `go-version` |
| `java` | `java-version` |
| `ruby` | `ruby-version` |
| `dotnet` | `dotnet-version` |
| `port` | `app-port` |
| `ts-strict` | `ts-strict-mode` |

Ignore rules behave like this:

- `file = "Dockerfile"` suppresses any finding where either side ends with `Dockerfile`
- `files = ["Dockerfile", ".nvmrc"]` suppresses only findings where both sides match one of those suffixes
- `rule = "VER001"` narrows the ignore to one rule ID
- `reason` is stored in config for humans but is not used by the engine

Monorepo settings:

- When `[monorepo].per_package = true` and `package_roots` is non-empty, contradictions are checked within each matched package instead of across the whole repository.
- Root-level files are still compared against each other and against package-local files.
- If multiple package root patterns match, the most specific match wins.
- `[monorepo].global_concepts` bypasses package scoping and compares those concepts repo-wide.
- `global_concepts` currently expects full concept IDs, not aliases.

## Custom extractors

Custom extractors are compiled from `.conflic.toml` at startup and merged with the built-in extractor set.

Supported extractor-level fields:

- `concept`: unique concept ID
- `display_name`: human-readable concept name
- `category`: known values `runtime-version`, `port`, `strict-mode`, `build-tool`, `package-manager`; anything else is kept as a custom category string
- `type`: `version`, `port`, `boolean`, or `string`; unknown values currently behave like `string`

Supported per-source fields:

- `file`: exact filename, exact path, or glob pattern
- `format`: `json`, `yaml`, `toml`, `env`, `plain`, or `dockerfile`
- `authority`: use `enforced`, `declared`, or `advisory`; unknown values currently fall back to `advisory`
- `pattern`: optional regex; if capture group 1 exists, that capture becomes the extracted value, otherwise the full match is used
- `path`: dot-separated lookup used by `json`, `yaml`, and `toml` sources
- `key`: exact key used by `env` sources

Format-specific behavior:

- `json`, `yaml`, and `toml` sources read a single value at `path`
- `env` sources read the first matching `key`
- `plain` sources operate on the whole trimmed file
- `dockerfile` sources test each `FROM` instruction's argument string and use the first match

Current validation behavior:

- invalid source formats, invalid file globs, invalid path globs, invalid relative globs, and invalid regex patterns are surfaced as `CONFIG001`
- if every source in a custom extractor is invalid, that extractor is skipped and a `CONFIG001` diagnostic is emitted
- missing format-specific fields such as `path` or `key` are not validated eagerly; those sources will simply produce no assertions

Example:

```toml
[[custom_extractor]]
concept = "redis-version"
display_name = "Redis Version"
category = "runtime-version"
type = "version"

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

## Diff scans and baselines

### Diff scans

`--diff <REF>` does more than scan changed files in isolation:

- it uses `git diff --name-only <REF> --`
- it also includes untracked files from `git ls-files --others --exclude-standard`
- it scans those changed files first
- it then pulls in peer files for any impacted concepts so comparisons remain meaningful

`--diff-stdin` uses the same peer-file expansion, but the changed path list comes from stdin instead of Git.

Current diff-mode behavior:

- parse diagnostics from untouched peer files are not carried into the diff result
- paths outside the scan root are ignored

### Baselines

```bash
# Write a baseline from the current result
conflic --update-baseline .conflic-baseline.json

# Suppress already-known findings and diagnostics
conflic --baseline .conflic-baseline.json
```

Baselines fingerprint both contradictions and parse/config diagnostics using stable fields such as:

- rule ID
- concept ID
- severity
- scan-root-relative file path
- key path
- normalized value text

Current baseline behavior:

- `--update-baseline` writes the file and then continues with normal reporting and normal exit-code handling
- `--baseline` suppresses matching contradictions and matching parse/config diagnostics
- if the `--baseline` file does not exist, it is silently ignored
- if `--baseline` and `--update-baseline` point to the same file, `conflic` exits with an error to avoid suppressing the freshly generated result

## Auto-fix

`conflic --fix` always prints a preview first. If proposals exist and `--dry-run` is not set, it then prompts for confirmation unless `--yes` is present.

Current fix winner model:

- the highest-authority assertion wins
- lower-authority contradictory assertions are proposed for update
- if multiple top-authority assertions disagree, the concept is marked unfixable instead of picking a winner arbitrarily

Currently supported fix targets:

- `.nvmrc` and `.node-version`
- `.python-version`
- `.ruby-version`
- `package.json` `engines.node`
- `global.json` `sdk.version`
- `go.mod` `go`
- `.tool-versions` entries for Node, Java, and Ruby
- `Gemfile` `ruby`
- `pom.xml` Java version tags
- `*.csproj` `TargetFramework`
- `Dockerfile*` `FROM` image tags for Node, Python, Go, Ruby, Java, and .NET
- `.env` and `.env.*` plain `KEY=value` port assignments
- `Dockerfile*` `EXPOSE`

Current fix limitations:

- `docker-compose` ports are not auto-fixed
- CI runtime settings are not auto-fixed
- `tsconfig` and ESLint strict-mode assertions are not auto-fixed
- matrix assertions are never auto-fixed
- `.csproj` `TargetFrameworks` entries are extracted as matrix assertions and therefore are not auto-fixed
- `.env` expressions such as `${PORT:-3000}` are compared but not rewritten automatically
- exact-value files such as `.nvmrc` are only rewritten when the winner can be rendered safely as an exact token

Operational details:

- backups are written as `*.conflic.bak` unless `--no-backup` is used
- writes are applied atomically
- `--dry-run` exits with code `1` whenever proposals or unfixable items exist

## Output formats

`conflic` currently supports three output formats:

- `terminal`: grouped by concept, with parse diagnostics first and concept assertions shown before findings
- `json`: top-level `version`, `concepts`, `parse_diagnostics`, and `summary`
- `sarif`: SARIF 2.1.0 with contradiction findings and parse/config diagnostics

Terminal output notes:

- by default, only concepts with findings are shown
- `--verbose` also shows concepts whose assertions are fully consistent
- `--quiet` suppresses output unless findings or diagnostics exist at or above the active threshold

Rule IDs currently emitted:

- `VER001`: version contradiction
- `PORT001`: port contradiction
- `BOOL001`: boolean contradiction
- `STR001`: string contradiction
- `PARSE001`: file read or parse failure
- `PARSE002`: blocked or failed local `extends` resolution
- `CONFIG001`: invalid custom extractor configuration

## LSP server

The default build includes an LSP server:

```bash
conflic --lsp
```

Current LSP capabilities:

- diagnostics for both sides of a contradiction
- diagnostics for parse and configuration issues
- quick-fix code actions backed by the same fix planner used by `--fix`
- incremental text sync
- debounced rescans
- targeted peer-file rescans through `IncrementalWorkspace`
- live `.conflic.toml` reload when the config file changes on disk or in an open editor buffer

For debugging incremental behavior, setting `CONFLIC_LSP_SCAN_STATS=1` causes the server to log full-scan and incremental-scan stats through the LSP log channel.

## Rust library usage

The crate can also be used as a library:

```rust
use conflic::config::ConflicConfig;

let root = std::path::Path::new(".");
let config = ConflicConfig::load(root, None)?;
let result = conflic::scan(root, &config)?;
```

Public entry points currently re-exported from the crate root include:

- `scan`
- `scan_with_overrides`
- `scan_diff`
- `scan_doctor`
- `git_changed_files`
- `IncrementalWorkspace`
- `IncrementalScanKind`
- `IncrementalScanStats`
- `DoctorReport`
- `DoctorFileInfo`

## CLI reference

| Flag | Current behavior |
| --- | --- |
| `[PATH]` | Directory to scan. Defaults to `.`. With `--init`, `.conflic.toml` is created in this directory. |
| `-f, --format <FORMAT>` | Output format: `terminal`, `json`, or `sarif`. Defaults to config first, then `terminal`. |
| `-s, --severity <SEVERITY>` | Active severity threshold: `error`, `warning`, or `info`. Affects exit status and `--quiet`, not report filtering. |
| `--check <A,B,...>` | Keep only selected concepts in the normal scan result. Accepts full concept IDs and built-in aliases. |
| `--init` | Create a template `.conflic.toml`. Exits with code `3` if the file already exists. |
| `-c, --config <PATH>` | Use an explicit config file. Relative paths are resolved from the scan root. |
| `-q, --quiet` | Suppress output unless findings or diagnostics exist at or above the active threshold. |
| `-v, --verbose` | Show consistent concepts as well as contradictory ones in terminal output. |
| `--no-color` | Disable terminal colors. |
| `--list-concepts` | Print built-in extractor IDs and descriptions, then exit. Custom extractors are not loaded for this command. |
| `--doctor` | Run diagnostic mode and exit. |
| `--diff <REF>` | Use Git to collect changed tracked files since `<REF>` plus untracked files, then run a diff-scoped scan. |
| `--diff-stdin` | Read changed file paths from stdin, one path per line, and run the same diff-scoped scan. |
| `--fix` | Build and print a fix plan, then apply proposals unless `--dry-run` is also set. |
| `--dry-run` | With `--fix`, preview only. Returns code `1` if any proposal or unfixable item exists. |
| `-y, --yes` | Skip the interactive confirmation prompt in fix mode. |
| `--no-backup` | Do not create `*.conflic.bak` files when applying fixes. |
| `--concept <CONCEPT>` | In fix mode, keep only proposals and unfixable items for one concept selector. |
| `--baseline <PATH>` | Suppress findings and parse/config diagnostics that match the baseline file, if that file exists. |
| `--update-baseline <PATH>` | Write a baseline JSON file from the current scan result, then continue normal reporting. |
| `--lsp` | Start the LSP server on stdin/stdout. |

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | No error findings, and no warning findings when the active threshold is `warning` or lower |
| `1` | Error-level contradiction or parse/config diagnostic, or an operational failure such as a config/Git error, rejected fix apply, or `--fix --dry-run` with work to do |
| `2` | Warning-level findings are present and the active threshold is `warning` or `info` |
| `3` | `--init` refused to overwrite an existing `.conflic.toml` |

Info-only findings do not currently produce a non-zero exit code.

## License

[MIT](LICENSE)
