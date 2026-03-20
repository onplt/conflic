use anyhow::Result;
use clap::Parser;
use std::process;

use conflic::cli::Cli;
use conflic::config::{self, ConflicConfig};
use conflic::model::Severity;
use conflic::report;

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle --lsp
    #[cfg(feature = "lsp")]
    if cli.lsp {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(conflic::lsp::server::run_lsp());
        return Ok(());
    }

    #[cfg(not(feature = "lsp"))]
    if cli.lsp {
        eprintln!("LSP support not compiled. Rebuild with `--features lsp`.");
        process::exit(1);
    }

    // Handle --init
    if cli.init {
        let config_path = cli.path.join(".conflic.toml");
        if config_path.exists() {
            eprintln!("Error: .conflic.toml already exists at {}", config_path.display());
            process::exit(3);
        }
        std::fs::write(&config_path, config::generate_template())?;
        println!("Created {}", config_path.display());
        return Ok(());
    }

    // Handle --list-concepts
    if cli.list_concepts {
        let extractors = conflic::extract::default_extractors();
        println!("Available concepts and extractors:\n");
        let mut seen = std::collections::HashSet::new();
        for ext in &extractors {
            let desc = ext.description();
            let id = ext.id();
            if seen.insert(id) {
                println!("  {:<40} {}", id, desc);
            }
        }
        return Ok(());
    }

    // Resolve scan path
    let scan_path = std::fs::canonicalize(&cli.path).unwrap_or_else(|_| cli.path.clone());

    // Load config
    let config = ConflicConfig::load(&scan_path, cli.config.as_deref())?;

    // Doctor mode
    if cli.doctor {
        let doctor_report = conflic::scan_doctor(&scan_path, &config)?;
        let output = report::doctor::render(&doctor_report, cli.no_color);
        print!("{}", output);
        return Ok(());
    }

    // Diff mode: only scan changed files + their concept peers
    let mut result = if let Some(ref git_ref) = cli.diff {
        let changed = conflic::git_changed_files(&scan_path, git_ref)?;
        if changed.is_empty() {
            if !cli.quiet {
                eprintln!("No files changed since {}", git_ref);
            }
            return Ok(());
        }
        conflic::scan_diff(&scan_path, &config, &changed)?
    } else if cli.diff_stdin {
        let stdin = std::io::read_to_string(std::io::stdin())?;
        let changed: Vec<std::path::PathBuf> = stdin
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| std::path::PathBuf::from(l.trim()))
            .collect();
        if changed.is_empty() {
            if !cli.quiet {
                eprintln!("No files provided on stdin");
            }
            return Ok(());
        }
        conflic::scan_diff(&scan_path, &config, &changed)?
    } else {
        conflic::scan(&scan_path, &config)?
    };

    // Update baseline if requested
    if let Some(ref baseline_path) = cli.update_baseline {
        let baseline = conflic::baseline::generate_baseline(&result);
        conflic::baseline::save_baseline(&baseline, baseline_path)?;
        eprintln!(
            "Baseline updated: {} findings saved to {}",
            baseline.fingerprints.len(),
            baseline_path.display()
        );
        // Continue to normal output after saving
    }

    // Apply baseline filter if provided
    if let Some(ref baseline_path) = cli.baseline {
        if baseline_path.exists() {
            let baseline = conflic::baseline::load_baseline(baseline_path)?;
            conflic::baseline::filter_baselined(&mut result, &baseline);
        }
    }

    // Fix mode
    if cli.fix {
        let plan = conflic::fix::plan_fixes(&result);

        // Always show preview first
        let output = conflic::fix::patcher::render_dry_run(&plan, cli.no_color);
        print!("{}", output);

        if !cli.dry_run && !plan.proposals.is_empty() {
            // Confirm unless --yes
            if !cli.yes {
                eprint!("Apply {} fix(es)? [y/N] ", plan.proposals.len());
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    eprintln!("Aborted.");
                    process::exit(1);
                }
            }

            let apply_result =
                conflic::fix::patcher::apply_fixes(&plan, !cli.no_backup);

            if !apply_result.files_backed_up.is_empty() {
                eprintln!(
                    "Backed up {} file(s)",
                    apply_result.files_backed_up.len()
                );
            }
            eprintln!(
                "Modified {} file(s)",
                apply_result.files_modified.len()
            );
            for (path, err) in &apply_result.errors {
                eprintln!("Error: {}: {}", path.display(), err);
            }
            if !apply_result.errors.is_empty() {
                process::exit(1);
            }
        }

        if cli.dry_run && (!plan.proposals.is_empty() || !plan.unfixable.is_empty()) {
            process::exit(1);
        }
        return Ok(());
    }

    // Render output
    let output = report::render(&result, &cli.format, cli.no_color, cli.verbose);

    if !cli.quiet || result.has_findings_at_or_above(cli.severity.to_severity()) {
        print!("{}", output);
    }

    // Exit code
    if result.error_count() > 0 {
        process::exit(1);
    } else if result.warning_count() > 0 && cli.severity.to_severity() <= Severity::Warning {
        process::exit(2);
    }

    Ok(())
}
