use crate::DoctorReport;
use owo_colors::OwoColorize;
use std::path::Path;

pub fn render(report: &DoctorReport, no_color: bool) -> String {
    let mut out = String::new();

    // Header
    out.push_str(&format!(
        "conflic v{} - doctor mode\n\n",
        env!("CARGO_PKG_VERSION")
    ));

    // Scan root
    out.push_str(&section("Scan Root", no_color));
    out.push_str(&format!("  {}\n\n", report.root.display()));

    // Registered extractors
    out.push_str(&section(
        &format!("Registered Extractors ({})", report.extractor_count),
        no_color,
    ));
    for (id, desc) in &report.extractor_names {
        out.push_str(&format!("  {:<40} {}\n", id, desc));
    }
    out.push('\n');

    // Discovered files
    let total_files: usize = report.discovered_files.values().map(|v| v.len()).sum();
    out.push_str(&section(
        &format!("Discovered Files ({})", total_files),
        no_color,
    ));

    let mut sorted_files: Vec<_> = report.discovered_files.iter().collect();
    sorted_files.sort_by_key(|(name, _)| (*name).clone());

    for (filename, paths) in &sorted_files {
        for path in *paths {
            let rel = simplify_path(path);
            if no_color {
                out.push_str(&format!("  {:<30} {}\n", filename, rel));
            } else {
                out.push_str(&format!("  {:<30} {}\n", filename, rel.dimmed()));
            }
        }
    }
    out.push('\n');

    // Per-file details
    out.push_str(&section("File Analysis", no_color));

    let mut sorted_details: Vec<_> = report.file_details.iter().collect();
    sorted_details.sort_by(|a, b| a.path.cmp(&b.path));

    for info in &sorted_details {
        let rel = simplify_path(&info.path);
        if no_color {
            out.push_str(&format!("  {}\n", rel));
        } else {
            out.push_str(&format!("  {}\n", rel.cyan()));
        }

        if info.matched_extractors.is_empty() {
            if no_color {
                out.push_str("    Extractors: (none matched)\n");
            } else {
                out.push_str(&format!("    Extractors: {}\n", "(none matched)".dimmed()));
            }
        } else {
            out.push_str(&format!(
                "    Extractors: {}\n",
                info.matched_extractors.join(", ")
            ));
        }

        for diagnostic in &info.parse_diagnostics {
            if no_color {
                out.push_str(&format!(
                    "    Parse diagnostic [{}]: {}\n",
                    diagnostic.severity, diagnostic.message
                ));
            } else {
                out.push_str(&format!(
                    "    Parse diagnostic [{}]: {}\n",
                    diagnostic.severity,
                    diagnostic.message.red()
                ));
            }
        }

        if info.assertions.is_empty() && info.parse_diagnostics.is_empty() {
            if no_color {
                out.push_str("    Assertions: (none extracted)\n");
            } else {
                out.push_str(&format!(
                    "    Assertions: {}\n",
                    "(none extracted)".dimmed()
                ));
            }
        } else {
            for a in &info.assertions {
                let key = if a.source.key_path.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", a.source.key_path)
                };
                out.push_str(&format!(
                    "    -> [{}] {} = \"{}\"{}  (line {}, {})\n",
                    a.concept.id,
                    a.concept.display_name,
                    a.raw_value,
                    key,
                    a.source.line,
                    a.authority,
                ));
            }
        }

        out.push('\n');
    }

    // Comparison results summary
    out.push_str(&section("Comparison Results", no_color));

    if report.scan_result.concept_results.is_empty() {
        out.push_str("  No concepts had enough assertions to compare.\n");
    } else {
        for cr in &report.scan_result.concept_results {
            let status = if cr.findings.is_empty() {
                if no_color {
                    "OK".to_string()
                } else {
                    "OK".green().to_string()
                }
            } else {
                let msg = format!("{} contradiction(s)", cr.findings.len());
                if no_color { msg } else { msg.red().to_string() }
            };

            out.push_str(&format!(
                "  {:<30} {} assertion(s), {}\n",
                cr.concept.display_name,
                cr.assertions.len(),
                status,
            ));

            for f in &cr.findings {
                out.push_str(&format!(
                    "    {} {}: {} vs {}\n",
                    f.severity, f.rule_id, f.left.raw_value, f.right.raw_value,
                ));
            }
        }
    }

    out.push('\n');

    // Final summary
    let separator = "-".repeat(50);
    if no_color {
        out.push_str(&separator);
    } else {
        out.push_str(&separator.dimmed().to_string());
    }
    out.push('\n');

    let total_assertions: usize = report.file_details.iter().map(|f| f.assertions.len()).sum();
    let total_findings: usize = report
        .scan_result
        .concept_results
        .iter()
        .map(|cr| cr.findings.len())
        .sum();

    out.push_str(&format!(
        "{} files discovered, {} assertions extracted, {} concepts compared, {} contradictions found\n",
        total_files,
        total_assertions,
        report.scan_result.concept_results.len(),
        total_findings,
    ));

    out
}

fn section(title: &str, no_color: bool) -> String {
    if no_color {
        format!("[{}]\n", title)
    } else {
        format!("[{}]\n", title.bold())
    }
}

fn simplify_path(path: &Path) -> String {
    crate::pathing::simplify_path(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DoctorFileInfo;
    use crate::model::ScanResult;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn test_render_uses_ascii_header_and_separator() {
        let report = DoctorReport {
            root: PathBuf::from("workspace"),
            discovered_files: HashMap::new(),
            file_details: Vec::<DoctorFileInfo>::new(),
            scan_result: ScanResult {
                concept_results: Vec::new(),
                parse_diagnostics: Vec::new(),
            },
            extractor_count: 0,
            extractor_names: Vec::new(),
        };

        let output = render(&report, true);

        assert!(
            output.contains(" - doctor mode"),
            "expected ASCII doctor header, got:\n{}",
            output
        );
        assert!(
            !output.contains("â"),
            "doctor output should not contain mojibake, got:\n{}",
            output
        );
    }
}
