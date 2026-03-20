use anyhow::{Context, Result};
use clap::Parser;
use std::process;

use conflic::cli::Cli;
use conflic::config::{self, ConflicConfig};
use conflic::fix::FixPlan;
use conflic::model::{ScanResult, Severity};
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
            eprintln!(
                "Error: .conflic.toml already exists at {}",
                config_path.display()
            );
            process::exit(3);
        }
        std::fs::write(&config_path, config::generate_template())
            .with_context(|| format!("Failed to create {}", config_path.display()))?;
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
    let config = ConflicConfig::load(&scan_path, cli.config.as_deref())
        .with_context(|| format!("Failed to load configuration for {}", scan_path.display()))?;
    let output_format = match cli.format {
        Some(format) => format,
        None => config.output_format()?,
    };
    let severity_filter = match cli.severity {
        Some(severity) => severity,
        None => config.severity_filter()?,
    };

    // Doctor mode
    if cli.doctor {
        let doctor_report = conflic::scan_doctor(&scan_path, &config)
            .with_context(|| format!("Doctor scan failed for {}", scan_path.display()))?;
        let output = report::doctor::render(&doctor_report, cli.no_color);
        print!("{}", output);
        return Ok(());
    }

    // Diff mode: only scan changed files + their concept peers
    let mut result = if let Some(ref git_ref) = cli.diff {
        let changed = conflic::git_changed_files(&scan_path, git_ref)
            .with_context(|| format!("Failed to collect changed files for git ref {}", git_ref))?;
        if changed.is_empty() {
            if !cli.quiet {
                eprintln!("No files changed since {}", git_ref);
            }
            return Ok(());
        }
        conflic::scan_diff(&scan_path, &config, &changed)
            .with_context(|| format!("Diff scan failed for {}", scan_path.display()))?
    } else if cli.diff_stdin {
        let stdin = std::io::read_to_string(std::io::stdin())
            .context("Failed to read changed file list from stdin")?;
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
        conflic::scan_diff(&scan_path, &config, &changed)
            .with_context(|| format!("Diff scan failed for {}", scan_path.display()))?
    } else {
        conflic::scan(&scan_path, &config)
            .with_context(|| format!("Scan failed for {}", scan_path.display()))?
    };

    if let Some(ref checked_concepts) = cli.check {
        filter_scan_result_by_concepts(&mut result, checked_concepts);
    }

    // Warn if --update-baseline and --baseline point to the same file
    if let (Some(update_path), Some(filter_path)) = (&cli.update_baseline, &cli.baseline) {
        // Normalize paths: try canonicalize for existing files, else normalize via
        // parent canonicalization + filename to handle ./foo vs foo
        let normalize = |p: &std::path::Path| -> std::path::PathBuf {
            if let Ok(c) = std::fs::canonicalize(p) {
                return c;
            }
            // File may not exist yet — canonicalize the parent directory instead
            if let (Some(parent), Some(name)) = (p.parent(), p.file_name()) {
                let parent_dir = if parent.as_os_str().is_empty() {
                    std::path::Path::new(".")
                } else {
                    parent
                };
                if let Ok(cp) = std::fs::canonicalize(parent_dir) {
                    return cp.join(name);
                }
            }
            p.to_path_buf()
        };
        if normalize(update_path) == normalize(filter_path) {
            eprintln!(
                "Error: --update-baseline and --baseline point to the same file. This would suppress all current findings."
            );
            process::exit(1);
        }
    }

    // Update baseline if requested
    if let Some(ref baseline_path) = cli.update_baseline {
        let baseline = conflic::baseline::generate_baseline(&result, &scan_path);
        conflic::baseline::save_baseline(&baseline, baseline_path)
            .with_context(|| format!("Failed to save baseline to {}", baseline_path.display()))?;
        eprintln!(
            "Baseline updated: {} entries saved to {}",
            baseline.entry_count(),
            baseline_path.display()
        );
        // Continue to normal output after saving
    }

    // Apply baseline filter if provided
    if let Some(ref baseline_path) = cli.baseline
        && baseline_path.exists()
    {
        let baseline = conflic::baseline::load_baseline(baseline_path)
            .with_context(|| format!("Failed to load baseline {}", baseline_path.display()))?;
        conflic::baseline::filter_baselined(&mut result, &baseline, &scan_path);
    }

    // Fix mode
    if cli.fix {
        let mut plan = conflic::fix::plan_fixes(&result);
        if let Some(ref concept_id) = cli.concept {
            filter_fix_plan_by_concept(&mut plan, concept_id);
        }

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

            let apply_result = conflic::fix::patcher::apply_fixes(&plan, !cli.no_backup);

            if !apply_result.files_backed_up.is_empty() {
                eprintln!("Backed up {} file(s)", apply_result.files_backed_up.len());
            }
            eprintln!("Modified {} file(s)", apply_result.files_modified.len());
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
    let output = report::render(&result, &output_format, cli.no_color, cli.verbose);

    if !cli.quiet || result.has_findings_at_or_above(severity_filter.to_severity()) {
        print!("{}", output);
    }

    // Exit code
    if result.error_count() > 0 {
        process::exit(1);
    } else if result.warning_count() > 0 && severity_filter.to_severity() <= Severity::Warning {
        process::exit(2);
    }

    Ok(())
}

fn filter_scan_result_by_concepts(result: &mut ScanResult, checked_concepts: &[String]) {
    result.concept_results.retain(|concept_result| {
        checked_concepts.iter().any(|selector| {
            conflic::config::concept_matches_selector(&concept_result.concept.id, selector)
        })
    });
}

fn filter_fix_plan_by_concept(plan: &mut FixPlan, concept_id: &str) {
    plan.proposals.retain(|proposal| {
        conflic::config::concept_matches_selector(&proposal.concept.id, concept_id)
    });
    plan.unfixable
        .retain(|item| conflic::config::concept_matches_selector(&item.concept.id, concept_id));
}
