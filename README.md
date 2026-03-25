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
  <a href="https://marketplace.visualstudio.com/items?itemName=ConflicScan.conflic"><img src="https://img.shields.io/visual-studio-marketplace/v/ConflicScan.conflic?label=VS%20Code" alt="VS Code Marketplace"></a>
</p>

---

Your `.nvmrc` says Node 20. Your `Dockerfile` pulls `node:18-alpine`. Your CI matrix tests against Node 22. Which one is right?

**conflic** finds these contradictions for you. It scans your project, extracts version pins, port declarations, and other configuration values from across file formats, then tells you where they disagree.

## Features

- **35 built-in extractors** covering Node.js, Python, Go, Java, Ruby, .NET versions, application ports, and TypeScript strict mode
- **IaC drift detection** for Terraform, Kubernetes, and Helm files
- **Custom extractors** defined in `.conflic.toml` for any concept you need to track
- **Policy rules** that enforce organizational constraints (e.g., "all services must use Node >= 20")
- **Cross-concept rules** that detect dependency violations (e.g., "Python 3.12 requires pip >= 22.3")
- **Diff-scoped scans** that focus on what changed since a git ref
- **Scan history and trends** to track configuration integrity over time
- **Multi-repo federation** to detect cross-repository drift across a fleet
- **Auto-fix** for supported file types, with previews and backups
- **LSP server** with live diagnostics, hover info, go-to-peer references, and quick-fix code actions
- **Impact analysis** to visualize the blast radius of configuration changes
- **Service topology mapping** from docker-compose and Kubernetes manifests
- **Organizational drift baselines** to enforce fleet-wide version standards
- **Environment promotion chains** to detect cross-environment drift (dev → staging → prod)
- **Wasm plugin system** for custom extractors and solvers (behind `wasm` feature flag)
- **Multiple output formats**: terminal, JSON, and SARIF

## Installation

Requires Rust 1.94+.

```bash
cargo install conflic
```

From source:

```bash
git clone https://github.com/onplt/conflic.git
cd conflic
cargo install --path .
```

Without the LSP server:

```bash
cargo install conflic --no-default-features
```

With Wasm plugin support:

```bash
cargo install conflic --features wasm
```

## Quick start

```bash
conflic                                      # scan current directory
conflic path/to/workspace                    # scan a specific path
conflic --format json                        # machine-readable output
conflic --format sarif > conflic.sarif       # SARIF for CI integrations
conflic --diff origin/main                   # only check what changed
conflic --since origin/main                  # only findings introduced since ref
conflic --fix --dry-run                      # preview auto-fix proposals
conflic --record                             # scan and save to history
conflic --trend                              # show trend report
conflic --federate federation.toml           # scan multiple repos
conflic --init                               # create a starter .conflic.toml
conflic --diff HEAD~1 --impact               # show blast radius of changes
conflic --topology                           # map service dependencies
conflic --capture-baseline org.toml          # capture organizational baseline
conflic --drift-baseline org.toml            # check conformance against baseline
```

### Example output

```
$ conflic

  Node.js Version  CONTRADICTION

    ✖ Dockerfile:1          FROM node:22-alpine          (enforced)
    ✖ .nvmrc:1              20                           (advisory)
    ✖ package.json:5        engines: ">=18 <20"          (declared)

  Application Port  CONTRADICTION

    ✖ Dockerfile:5          EXPOSE 3000                  (enforced)
    ✖ docker-compose.yml:8  ports: "6379:6379"           (enforced)

  2 concepts · 3 errors · 1 warning · 0 info
```

## Editor integration

### VS Code Extension

The easiest way to use conflic is the [VS Code extension](https://marketplace.visualstudio.com/items?itemName=ConflicScan.conflic). Install it from the Marketplace and contradictions show up as squiggly underlines the moment you open a project — no setup needed.

The extension provides:
- **Real-time diagnostics** with inline squiggles and problem panel entries
- **Hover cards** showing concept details, authority levels, and all peer declarations
- **Quick Fix** code actions (click the lightbulb or press `Ctrl+.`) with authority-based auto-resolution
- **Go-to-peer references** to jump between files asserting the same concept
- **Concept Overview sidebar** with a tree view of every detected concept
- **Status bar counter** showing remaining errors and warnings

Search "Conflic" in the Extensions panel or install from the command line:

```bash
code --install-extension ConflicScan.conflic
```

### Other editors

Any editor with LSP support can use conflic as a language server:

```bash
conflic --lsp
```

The server communicates over stdin/stdout. Point your editor's LSP client to the conflic binary with the `--lsp` flag. See the [LSP server](#lsp-server) section for details on supported capabilities.

## What conflic knows about

### Built-in concepts

| Concept | Sources |
| --- | --- |
| **Node.js Version** | `.nvmrc`, `.node-version`, `package.json` engines, Dockerfiles, CI workflows, `.tool-versions`, Kubernetes manifests, Helm values, Terraform |
| **Python Version** | `.python-version`, `pyproject.toml`, Dockerfiles, CI workflows, Kubernetes manifests, Helm values, Terraform |
| **Go Version** | `go.mod`, Dockerfiles, Kubernetes manifests, Helm values, Terraform |
| **Java Version** | `pom.xml`, Dockerfiles (OpenJDK, Temurin, Corretto, Semeru), `.sdkmanrc`, `.tool-versions`, CI workflows, Kubernetes manifests, Helm values, Terraform |
| **Ruby Version** | `.ruby-version`, `Gemfile`, Dockerfiles, `.tool-versions`, CI workflows, Kubernetes manifests, Helm values, Terraform |
| **.NET Version** | `*.csproj`, `global.json`, Dockerfiles, Kubernetes manifests, Helm values, Terraform |
| **Application Port** | `.env` / `.env.*`, `docker-compose*.yml`, Dockerfile `EXPOSE`, Kubernetes manifests, Helm values, Terraform |
| **TypeScript Strict Mode** | `tsconfig*.json`, ESLint configs (legacy and flat) |

CI workflows are recognized from `.github/workflows/*.yml`, `.circleci/config.yml`, `.gitlab-ci.yml`, and `.gitlab-ci/*.yml`.

Dockerfiles include variants like `Dockerfile.dev`. ESLint configs include `.eslintrc`, `.eslintrc.json`, `.eslintrc.yml`, and `eslint.config.*` files.

### Infrastructure-as-Code (IaC) sources

conflic extracts versions and ports from IaC files, enabling drift detection between application configs and infrastructure definitions:

- **Terraform** (`*.tf`): Lambda/Cloud Functions `runtime` values (`nodejs20.x`, `python3.12`, `java21`, etc.), container `image` tags, `container_port` and `host_port` assignments
- **Kubernetes** (`deployment.yaml`, `service.yaml`, `statefulset.yaml`, `pod.yaml`, `job.yaml`, `cronjob.yaml`): container `image` tags, `containerPort`, Service `targetPort`
- **Helm** (`values.yaml`, `values.yml`): `image.repository` + `image.tag` patterns (including nested multi-service charts), `port`, `containerPort`, `targetPort`, `servicePort` keys

IaC assertions use appropriate authority levels: Terraform resource attributes and Kubernetes container images are `enforced`, Helm values are `declared`.

### How values are compared

conflic doesn't just do string comparison. It understands the semantics of each value type:

- **Versions**: exact values (`20.0.0`), partials (`20`), ranges (`^20`, `>=18 <20`), and Docker tags (`22-alpine`) are compared using semver-aware logic
- **Ports**: single ports, ranges (`3000-3005`), and Docker mappings (`3000:8080`) are compared by their container port
- **Booleans**: literal `true` / `false`
- **Strings**: exact equality

### Authority levels

Each assertion carries an authority level that determines the severity of contradictions:

| Level | Meaning | Examples |
| --- | --- | --- |
| **enforced** | Hard constraint; build breaks if wrong | Final Dockerfile `FROM`, CI runtime versions, docker-compose ports |
| **declared** | Should match, but not mechanically enforced | `package.json` engines, `pyproject.toml`, `.env`, `pom.xml` |
| **advisory** | Informational; nice to keep in sync | `.nvmrc`, `.python-version`, `.tool-versions`, non-final Docker stages |

When two assertions conflict, severity depends on the authority pair:

| Pair | Severity |
| --- | --- |
| enforced + enforced | error |
| enforced + declared | error |
| enforced + advisory | warning |
| declared + declared | warning |
| declared + advisory | info |
| advisory + advisory | info |

## Configuration

conflic looks for `.conflic.toml` in the scan root. Run `conflic --init` to generate a starter config.

```toml
[conflic]
severity = "warning"       # minimum severity: "error", "warning", or "info"
format = "terminal"        # output: "terminal", "json", or "sarif"
exclude = []               # extra directories or glob patterns to skip
skip_concepts = []         # concepts to ignore entirely

# Suppress a specific contradiction
[[ignore]]
rule = "VER001"
files = ["Dockerfile", ".nvmrc"]
reason = "Multi-stage build; final stage matches"

# Monorepo support
[monorepo]
per_package = true
package_roots = ["packages/*", "apps/*"]
global_concepts = ["node-version", "ts-strict-mode"]

# Organizational policies
[[policy]]
id = "POL001"
concept = "node-version"
rule = ">= 20"
severity = "error"
message = "Node 18 is EOL. All services must use Node 20+."
```

You can use short aliases like `node`, `python`, `port` in `--check`, `skip_concepts`, and `--concept` flags.

### Custom extractors

Track any configuration value by defining custom extractors:

```toml
[[custom_extractor]]
concept = "redis-version"
display_name = "Redis Version"
category = "runtime-version"
type = "version"
solver = "semver"           # optional: "semver", "port", "boolean", "exact-string"

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

Supported source formats: `json`, `yaml`, `toml`, `env`, `plain`, `dockerfile`.

### Policy rules

Policies enforce organizational constraints independent of inter-file contradictions:

```toml
[[policy]]
id = "POL002"
concept = "app-port"
rule = "!= 80, != 443"
severity = "warning"
message = "Privileged ports require root."

[[policy]]
id = "POL003"
concept = "python-version"
rule = "!= 3.8, != 3.9"
severity = "error"
message = "Python 3.8 and 3.9 are EOL."
```

Version policies use semver ranges (`>= 20`). Port policies use port specs (`!= 80`). String policies use comma-separated blacklists (`!= value1, != value2`).

### Cross-concept rules

Define dependency relationships between concepts. When one concept matches a condition, another concept must satisfy a constraint:

```toml
[[concept_rule]]
id = "RULE001"
severity = "warning"
message = "Python 3.12+ requires pip >= 22.3"

[concept_rule.when]
concept = "python-version"
matches = ">= 3.12"

[concept_rule.then]
concept = "pip-version"
requires = ">= 22.3"
```

Cross-concept rules are evaluated after per-concept contradiction detection and policy evaluation. The `when.matches` field supports semver ranges and exact values. The `then.requires` field uses the same format. Findings from concept rules appear as additional entries in the scan results.

### Wasm plugins

> **Note:** Requires the `wasm` feature flag (`cargo install conflic --features wasm`).

Extend conflic with custom extractors and solvers written as WebAssembly modules:

```toml
[[plugin]]
name = "my-extractor"
path = "plugins/my-extractor.wasm"
kind = "extractor"                    # "extractor", "solver", or "both"
file_patterns = ["*.custom"]

[[plugin]]
name = "my-solver"
path = "plugins/my-solver.wasm"
kind = "solver"
concepts = ["my-concept"]
```

Plugins communicate via JSON through linear memory. Extractor plugins export `conflic_extract(ptr, len) -> i64`, solver plugins export `conflic_solve(ptr, len) -> i64`. Both must also export `conflic_alloc(len) -> i32` and `conflic_dealloc(ptr, len)` for memory management.

## Scan history and trends

Track configuration integrity over time with scan history:

```bash
conflic --record                    # scan and record results in .conflic-history.json
conflic --trend                     # show trend report from recorded history
conflic --since v1.0.0              # only show findings introduced since a git ref
```

`--record` appends a snapshot (commit SHA, author, timestamp, finding counts) to `.conflic-history.json` in the scan root. This file should typically be gitignored.

`--trend` shows a table of historical snapshots with error/warning/info counts, plus lists of new and resolved findings between the last two scans.

`--since <REF>` uses `git blame` to determine when each finding was introduced and filters out findings that predate the given ref. This is useful in CI to answer "did this PR make things worse?"

## Multi-repository federation

Scan multiple repositories and detect cross-repo drift:

```bash
conflic --init-federation           # create a template conflic-federation.toml
conflic --federate conflic-federation.toml   # run federated scan
conflic --federate conflic-federation.toml --format json  # JSON output
```

Federation config (`conflic-federation.toml`):

```toml
[[repository]]
name = "api-gateway"
path = "../api-gateway"
group = "backend"

[[repository]]
name = "user-service"
path = "../user-service"
group = "backend"

[[repository]]
name = "web-app"
path = "../web-app"
group = "frontend"
```

Each repository is scanned independently using its own `.conflic.toml` (if present). Repositories in the same `group` are compared for cross-repo drift: if the same concept has different values across repos in a group, it's reported as drift. The federation report shows per-repo finding counts and cross-repo drift entries.

The exit code is `1` if any repository has errors or if cross-repo drift is detected.

## Diff scans and baselines

### Diff scans

`--diff <REF>` scans files changed since a git ref, plus any peer files needed to evaluate impacted concepts. This keeps CI fast while still catching cross-file contradictions.

```bash
conflic --diff origin/main
git diff --name-only origin/main | conflic --diff-stdin
```

### Baselines

Suppress known findings so you can adopt conflic incrementally:

```bash
conflic --update-baseline .conflic-baseline.json   # save current state
conflic --baseline .conflic-baseline.json          # suppress known issues
```

Baselines track findings by rule ID, concept, severity, file path, and value, so new contradictions are still caught even if old ones are suppressed.

### Organizational drift baselines

Capture a snapshot of your current configuration values as an organizational standard, then check any repository for conformance:

```bash
conflic --capture-baseline .conflic-baseline.toml   # save current values as the standard
conflic --drift-baseline .conflic-baseline.toml     # check conformance against the standard
```

The captured baseline is a TOML file containing expected values and optional tolerances for each concept:

```toml
version = "1.0"

[[expectation]]
concept = "node-version"
expected = "20.11.0"
tolerance = "20.x"

[[expectation]]
concept = "python-version"
expected = "3.12.2"
```

Drift is classified as **exact** (matches), **minor** (within tolerance, e.g. patch difference), **major** (outside tolerance), or **missing** (concept expected but not found). This is useful for enforcing fleet-wide standards across many repositories.

### Impact analysis

When using `--diff`, add `--impact` to see the blast radius of your changes:

```bash
conflic --diff origin/main --impact
```

The impact report shows:

- **Root changes**: files you directly modified
- **Direct impacts**: peer files that share concepts with your changes
- **Transitive impacts**: files affected via cross-concept rule dependencies
- **Blast radius summary**: total files affected, concepts affected, and worst severity

### Service topology

Analyze service dependencies from docker-compose and Kubernetes manifests:

```bash
conflic --topology
```

This builds a service dependency graph by extracting:

- **docker-compose**: service names, ports, `depends_on`, `links`, and environment variable cross-references
- **Kubernetes**: Deployments, Services, StatefulSets, Jobs, and CronJobs with label-based selector matching

The output includes a list of services with their ports and dependencies, plus detected edges (depends_on, link, env reference, selector match) between services.

### Environment promotion chains

Track configuration consistency across deployment environments (e.g. dev → staging → prod):

```toml
[promotion]
chain = ["dev", "staging", "prod"]

[[promotion.pattern]]
environment = "dev"
files = ["*.dev.*", "*.dev"]

[[promotion.pattern]]
environment = "staging"
files = ["*.staging.*", "*.staging"]

[[promotion.pattern]]
environment = "prod"
files = ["*.prod.*", "*.prod"]
```

When configured, conflic compares assertion values across environments in chain order. Cross-environment contradictions produce `PROMO001` findings with warning severity.

## Auto-fix

```bash
conflic --fix              # preview + prompt before applying
conflic --fix --dry-run    # preview only
conflic --fix --yes        # apply without prompting
```

The highest-authority assertion wins. Lower-authority files are updated to match. If the top-authority values disagree with each other, the concept is marked unfixable.

Supported fix targets include version files (`.nvmrc`, `.python-version`, `.ruby-version`), package manifests (`package.json`, `go.mod`, `Gemfile`, `pom.xml`, `*.csproj`, `global.json`), Dockerfiles (`FROM` tags and `EXPOSE`), `.tool-versions`, and `.env` port values.

Backups are written as `*.conflic.bak` unless `--no-backup` is passed. All writes are atomic.

## LSP server

> **Tip:** If you use VS Code, you don't need to configure the LSP manually — the [VS Code extension](https://marketplace.visualstudio.com/items?itemName=ConflicScan.conflic) handles everything for you.

For other editors, start the server with:

```bash
conflic --lsp
```

The server communicates over stdin/stdout using the Language Server Protocol. It provides:

- **Diagnostics** for contradictions and parse errors on both sides of each finding
- **Hover** showing the concept name, authority, all peer declarations, and contradiction status
- **Go-to-peer references** to jump between all files asserting the same concept
- **Document symbols** listing all extracted assertions in the outline view
- **Quick-fix code actions** using the same fix planner as `--fix`
- **Incremental rescans** with debouncing and peer-file invalidation
- **Live config reload** when `.conflic.toml` changes

Set `CONFLIC_LSP_SCAN_STATS=1` to log scan statistics for debugging.

## Library usage

```rust
use conflic::config::ConflicConfig;

let root = std::path::Path::new(".");
let config = ConflicConfig::load(root, None)?;
let result = conflic::scan(root, &config)?;
```

Key exports: `scan`, `scan_with_overrides`, `scan_diff`, `scan_doctor`, `git_changed_files`, `IncrementalWorkspace`.

## Discovery and parsing

- Respects `.gitignore` and Git exclude files
- Always skips `node_modules`, `.git`, `vendor`, `target`, `dist`, `build`, `__pycache__`, `.tox`, `.venv`, `venv`
- JSON files fall back to JSON5 parsing (comments, trailing commas, single-quoted strings)
- YAML supports anchors and merge keys
- `tsconfig` and ESLint `extends` chains are resolved with cycle detection
- `eslint.config.*` files are statically parsed (not executed)
- Extends targets outside the scan root are blocked and reported as `PARSE002`

## Rule IDs

| ID | Meaning |
| --- | --- |
| `VER001` | Version contradiction |
| `PORT001` | Port contradiction |
| `BOOL001` | Boolean contradiction |
| `STR001` | String contradiction |
| `<custom>` | Cross-concept rule violation (uses the `id` from `[[concept_rule]]`) |
| `POL*` | Policy violation |
| `PROMO001` | Cross-environment promotion chain violation |
| `PARSE001` | File read or parse failure |
| `PARSE002` | Blocked or failed `extends` resolution |
| `CONFIG001` | Invalid custom extractor configuration |

## CLI reference

| Flag | Description |
| --- | --- |
| `[PATH]` | Directory to scan (default: `.`) |
| `-f, --format` | Output format: `terminal`, `json`, `sarif` |
| `-s, --severity` | Severity threshold: `error`, `warning`, `info` |
| `--check <A,B,...>` | Only report selected concepts |
| `--init` | Create a template `.conflic.toml` |
| `-c, --config <PATH>` | Explicit config file path |
| `-q, --quiet` | Suppress output when clean |
| `-v, --verbose` | Also show consistent concepts |
| `--no-color` | Disable colors |
| `--list-concepts` | Print built-in extractors and exit |
| `--doctor` | Run diagnostic mode |
| `--diff <REF>` | Diff-scoped scan since a git ref |
| `--diff-stdin` | Diff-scoped scan from stdin paths |
| `--fix` | Auto-fix contradictions |
| `--dry-run` | Preview fixes without applying |
| `-y, --yes` | Skip confirmation prompt |
| `--no-backup` | Don't create `.conflic.bak` files |
| `--concept <ID>` | Limit fix to one concept |
| `--baseline <PATH>` | Suppress known findings |
| `--update-baseline <PATH>` | Save current findings as baseline |
| `--record` | Record scan in `.conflic-history.json` |
| `--trend` | Show trend report from scan history |
| `--since <REF>` | Only show findings introduced since a git ref |
| `--federate <PATH>` | Run federated scan across multiple repos |
| `--init-federation` | Create a template `conflic-federation.toml` |
| `--impact` | Show blast radius of changes (use with `--diff`) |
| `--capture-baseline <PATH>` | Capture current scan as an organizational baseline |
| `--drift-baseline <PATH>` | Check conformance against an organizational baseline |
| `--topology` | Analyze service topology from docker-compose and Kubernetes |
| `--lsp` | Start the LSP server |

## GitHub Action

Use conflic as a CI gate to block PRs that introduce configuration contradictions.

### Quick start

```yaml
- uses: onplt/conflic@v1
  with:
    severity: warning
    fail-on: error
```

### Inputs

| Input | Default | Description |
| --- | --- | --- |
| `version` | `latest` | Conflic version to install (e.g., `1.0.1`) |
| `path` | `.` | Directory to scan |
| `severity` | `error` | Minimum severity to report: `error`, `warning`, `info` |
| `fail-on` | `error` | Severity threshold that causes failure: `error`, `warning`, `info`, `none` |
| `diff` | `""` | Git ref for diff-scoped scan. `auto` = PR base SHA |
| `sarif-upload` | `true` | Upload SARIF to GitHub Code Scanning |
| `baseline` | `""` | Path to baseline file |
| `config` | `""` | Path to `.conflic.toml` |
| `args` | `""` | Additional CLI arguments |

### Outputs

| Output | Description |
| --- | --- |
| `exit-code` | Raw conflic exit code (0/1/2) |
| `error-count` | Number of error-level findings |
| `warning-count` | Number of warning-level findings |
| `sarif-file` | Path to generated SARIF file |

### Scenarios

**PR diff scan** — only check changed files:

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0
- uses: onplt/conflic@v1
  with:
    diff: auto
    fail-on: error
```

**SARIF annotations** — inline PR comments via Code Scanning:

```yaml
permissions:
  security-events: write
steps:
  - uses: actions/checkout@v4
  - uses: onplt/conflic@v1
    with:
      sarif-upload: true
      fail-on: none
```

**Baseline workflow** — suppress known issues:

```yaml
- uses: onplt/conflic@v1
  with:
    baseline: .conflic-baseline.json
    diff: auto
    fail-on: error
```

`severity` controls what conflic reports. `fail-on` controls what fails the action. This lets you annotate warnings in PRs without blocking merges.

More examples in [`.github/examples/`](.github/examples/).

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Clean (no findings at or above threshold) |
| `1` | Error-level finding or operational failure |
| `2` | Warning-level findings present |
| `3` | `--init` refused (config already exists) |

## Contributing

Contributions are welcome! Whether it's a bug report, a new extractor, or a documentation fix — every bit helps.

```bash
git clone https://github.com/onplt/conflic.git
cd conflic
cargo build
cargo test
```

If you're adding a new extractor, drop a test fixture in `tests/fixtures/` and add an integration test. See the existing extractors in `src/` for the pattern to follow.

Please open an issue before starting on large features so we can align on the approach.

## License

[MIT](LICENSE)
