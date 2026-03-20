use crate::model::*;
use owo_colors::OwoColorize;
use std::path::Path;

pub fn render(result: &ScanResult, no_color: bool, verbose: bool) -> String {
    let mut output = String::new();

    // Header
    output.push_str(&format!(
        "conflic v{} - scanning\n\n",
        env!("CARGO_PKG_VERSION")
    ));

    // Parse diagnostics
    if !result.parse_diagnostics.is_empty() {
        output.push_str("Parse Diagnostics:\n");
        for diagnostic in &result.parse_diagnostics {
            let severity_str = format_severity(diagnostic.severity, no_color);
            output.push_str(&format!(
                "  {}  {}: {}\n",
                severity_str,
                simplify_path(&diagnostic.file),
                diagnostic.message
            ));
        }
        output.push('\n');
    }

    let mut concepts_with_findings = 0;
    let mut concepts_checked = 0;

    for concept_result in &result.concept_results {
        concepts_checked += 1;

        if concept_result.findings.is_empty() && !verbose {
            continue;
        }

        // Concept header
        if no_color {
            output.push_str(&format!(
                "{} [{}]\n",
                concept_result.concept.display_name, concept_result.concept.id
            ));
        } else {
            output.push_str(&format!(
                "{} [{}]\n",
                concept_result.concept.display_name.bold(),
                concept_result.concept.id.dimmed()
            ));
        }

        if concept_result.findings.is_empty() {
            if no_color {
                output.push_str("  OK  All assertions consistent\n");
            } else {
                output.push_str(&format!("  {}  All assertions consistent\n", "OK".green()));
            }

            if verbose {
                for assertion in &concept_result.assertions {
                    output.push_str(&format!(
                        "    {}  {}\n",
                        format_location(&assertion.source, no_color),
                        assertion.raw_value,
                    ));
                }
            }

            output.push('\n');
            continue;
        }

        concepts_with_findings += 1;

        // Show all assertions for this concept
        for assertion in &concept_result.assertions {
            let loc = format_location(&assertion.source, no_color);
            let key_info = if assertion.source.key_path.is_empty() {
                String::new()
            } else {
                format!("  ({})", assertion.source.key_path)
            };

            output.push_str(&format!(
                "    {:<40} {}{}\n",
                loc, assertion.raw_value, key_info
            ));
        }

        // Show findings
        for finding in &concept_result.findings {
            let severity_str = format_severity(finding.severity, no_color);
            output.push_str(&format!("  {}  {}\n", severity_str, finding.explanation));
        }

        output.push('\n');
    }

    // Summary line
    let separator = "-".repeat(50);
    output.push_str(&format!(
        "{}\n",
        if no_color {
            separator.clone()
        } else {
            separator.dimmed().to_string()
        }
    ));

    let mut parts = vec![format!("{} concepts checked", concepts_checked)];

    let total_errors = result.error_count();
    let total_warnings = result.warning_count();
    let total_info = result.info_count();

    if total_errors > 0 {
        if no_color {
            parts.push(format!("{} error(s)", total_errors));
        } else {
            parts.push(format!("{}", format!("{} error(s)", total_errors).red()));
        }
    }
    if total_warnings > 0 {
        if no_color {
            parts.push(format!("{} warning(s)", total_warnings));
        } else {
            parts.push(format!(
                "{}",
                format!("{} warning(s)", total_warnings).yellow()
            ));
        }
    }
    if total_info > 0 {
        parts.push(format!("{} info", total_info));
    }
    if concepts_with_findings == 0 && result.parse_diagnostics.is_empty() {
        if no_color {
            parts.push("no contradictions found".to_string());
        } else {
            parts.push(format!("{}", "no contradictions found".green()));
        }
    }

    output.push_str(&parts.join(", "));
    output.push('\n');

    output
}

fn format_severity(severity: Severity, no_color: bool) -> String {
    if no_color {
        format!("{:<7}", severity)
    } else {
        match severity {
            Severity::Error => format!("{:<7}", "ERROR".red().bold()),
            Severity::Warning => format!("{:<7}", "WARNING".yellow().bold()),
            Severity::Info => format!("{:<7}", "INFO".dimmed()),
        }
    }
}

fn format_location(loc: &SourceLocation, no_color: bool) -> String {
    let rel_path = simplify_path(&loc.file);
    let location = format!("{}:{}", rel_path, loc.line);
    if no_color {
        location
    } else {
        location.cyan().to_string()
    }
}

fn simplify_path(path: &Path) -> String {
    let sanitized = crate::pathing::strip_windows_extended_length_prefix(path);

    // Try to make path relative to current dir
    if let Ok(cwd) = std::env::current_dir() {
        let sanitized_cwd = crate::pathing::strip_windows_extended_length_prefix(&cwd);
        if let Ok(rel) = sanitized.strip_prefix(&sanitized_cwd) {
            return rel.to_string_lossy().to_string();
        }
    }

    sanitized.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simplify_path_strips_windows_extended_length_prefix() {
        let simplified = simplify_path(Path::new(r"\\?\C:\workspace\package.json"));

        assert_eq!(simplified, r"C:\workspace\package.json");
    }

    #[test]
    fn test_render_uses_ascii_header_and_separator() {
        let output = render(
            &ScanResult {
                concept_results: Vec::new(),
                parse_diagnostics: Vec::new(),
            },
            true,
            false,
        );

        assert!(
            output.contains(" - scanning"),
            "expected ASCII scan header, got:\n{}",
            output
        );
        assert!(
            !output.contains("â"),
            "terminal output should not contain mojibake, got:\n{}",
            output
        );
    }
}
