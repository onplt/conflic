# Contributing to conflic

Thank you for your interest in contributing to conflic! This guide will help you get started.

## Getting Started

### Prerequisites

- **Rust 1.94+** (edition 2024)
- **Git**

### Setup

```bash
git clone https://github.com/conflic/conflic.git
cd conflic
cargo build
cargo test
```

### Project Structure

```
src/
├── main.rs              # CLI entrypoint
├── lib.rs               # Public API (scan, scan_diff, scan_doctor)
├── cli.rs               # Clap argument definitions
├── config.rs            # .conflic.toml parsing
├── baseline.rs          # Baseline fingerprinting
├── discover/            # File discovery (directory walking)
├── parse/               # File format parsers (JSON, YAML, TOML, ENV, Dockerfile)
├── extract/             # Semantic extractors (Node, Python, Go, Ruby, Java, .NET, ports)
├── model/               # Core data types (assertions, concepts, findings)
├── solve/               # Contradiction detection engine
├── report/              # Output formatters (terminal, JSON, SARIF)
├── fix/                 # Auto-fix engine
└── lsp/                 # Language Server Protocol implementation
```

The pipeline flows: **Discover** → **Parse** → **Extract** → **Solve** → **Report**.

## How to Contribute

### Reporting Bugs

Open a [bug report](https://github.com/conflic/conflic/issues/new?template=bug_report.md) with:
- The command you ran
- Expected vs. actual behavior
- Relevant config files (sanitized)
- `conflic --doctor` output if applicable

### Suggesting Features

Open a [feature request](https://github.com/conflic/conflic/issues/new?template=feature_request.md). Good feature requests include:
- The problem you're trying to solve
- Your proposed solution
- Alternative approaches you've considered

### Adding a New Extractor

This is the most common type of contribution. To add a new extractor:

1. Create a new module in `src/extract/` (e.g., `src/extract/terraform_version.rs`)
2. Implement the `Extractor` trait:
   ```rust
   pub trait Extractor: Send + Sync {
       fn id(&self) -> &'static str;
       fn description(&self) -> &'static str;
       fn extract(&self, files: &HashMap<String, Vec<PathBuf>>) -> Vec<ConfigAssertion>;
   }
   ```
3. Register your extractor in `src/extract/mod.rs` → `default_extractors()`
4. Add a test fixture in `tests/fixtures/`
5. Add an integration test in `tests/integration.rs`

### Pull Requests

1. Fork the repository and create a branch from `main`
2. Make your changes
3. Add or update tests as needed
4. Run the full test suite: `cargo test`
5. Run the linter: `cargo clippy -- -D warnings`
6. Format your code: `cargo fmt`
7. Open a pull request

## Development

### Running Tests

```bash
# All tests
cargo test

# A specific test
cargo test test_node_contradiction

# With output
cargo test -- --nocapture
```

### Building

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Without LSP feature
cargo build --no-default-features
```

### Testing Locally

```bash
# Scan a test fixture
cargo run -- tests/fixtures/node_contradiction

# Doctor mode on a fixture
cargo run -- --doctor tests/fixtures/node_contradiction

# List concepts
cargo run -- --list-concepts
```

## Code Style

- Follow standard Rust conventions (`cargo fmt` enforces this)
- Use `cargo clippy` with no warnings
- Keep functions focused and small
- Add doc comments to public items
- Prefer returning `Result` over panicking

## Commit Messages

- Use imperative mood ("Add feature" not "Added feature")
- Keep the first line under 72 characters
- Reference issues when applicable (`Fixes #123`)

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
