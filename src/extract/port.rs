use super::Extractor;
use crate::model::*;
use crate::parse::source_location::*;
use crate::parse::*;
use regex::Regex;
use serde::Deserialize;
use std::sync::LazyLock;

static ENV_DEFAULT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$\{(\w+):-([^}]*)\}").unwrap());

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ComposePortEntry {
    Scalar(String),
    Number(u16),
    Mapping(ComposePortMapping),
}

#[derive(Debug, Deserialize)]
struct ComposePortMapping {
    target: ComposePortValue,
    #[serde(default)]
    published: Option<ComposePortValue>,
    #[serde(default)]
    protocol: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ComposePortValue {
    Number(u16),
    String(String),
}

impl ComposePortValue {
    fn display(&self) -> String {
        match self {
            ComposePortValue::Number(value) => value.to_string(),
            ComposePortValue::String(value) => value.clone(),
        }
    }

    fn as_u16(&self) -> Option<u16> {
        match self {
            ComposePortValue::Number(value) => Some(*value),
            ComposePortValue::String(value) => value.trim().parse::<u16>().ok(),
        }
    }
}

// --- .env PORT extractor ---

pub struct EnvPortExtractor;

impl Extractor for EnvPortExtractor {
    fn id(&self) -> &str {
        "port-env"
    }
    fn description(&self) -> &str {
        "Port from .env files"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec![".env"]
    }

    fn matches_file(&self, filename: &str) -> bool {
        filename == ".env" || filename.starts_with(".env.")
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::Env(ref entries) = file.content {
            let mut results = Vec::new();
            for entry in entries {
                let key_upper = entry.key.to_uppercase();
                if key_upper == "PORT" || key_upper == "APP_PORT" || key_upper == "SERVER_PORT" {
                    // Try to resolve env var defaults: ${PORT:-3000}
                    let resolved =
                        resolve_env_default(&entry.value).unwrap_or_else(|| entry.value.clone());

                    if let Some(port) = parse_port(&resolved) {
                        results.push(ConfigAssertion::new(
                            SemanticConcept::app_port(),
                            SemanticType::Port(port),
                            entry.value.clone(),
                            SourceLocation {
                                file: file.path.clone(),
                                line: entry.line,
                                column: 0,
                                key_path: entry.key.clone(),
                            },
                            Authority::Declared,
                            self.id(),
                        ));
                    }
                }
            }
            return results;
        }
        vec![]
    }
}

// --- docker-compose ports extractor ---

pub struct DockerComposePortExtractor;

impl Extractor for DockerComposePortExtractor {
    fn id(&self) -> &str {
        "port-docker-compose"
    }
    fn description(&self) -> &str {
        "Port from docker-compose.yml"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec!["docker-compose"]
    }

    fn matches_file(&self, filename: &str) -> bool {
        filename.starts_with("docker-compose")
            && (filename.ends_with(".yml") || filename.ends_with(".yaml"))
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::Yaml(ref value) = file.content {
            let mut results = Vec::new();

            if let Some(services) = value.get("services").and_then(|s| s.as_object()) {
                for (_service_name, service_config) in services {
                    if let Some(ports) = service_config.get("ports").and_then(|p| p.as_array()) {
                        for port_val in ports {
                            if let Some((raw_value, port, line_hints)) =
                                parse_compose_port_entry(port_val)
                            {
                                let line = find_line_for_hints(&file.raw_text, &line_hints);
                                results.push(ConfigAssertion::new(
                                    SemanticConcept::app_port(),
                                    SemanticType::Port(port),
                                    raw_value,
                                    SourceLocation {
                                        file: file.path.clone(),
                                        line,
                                        column: 0,
                                        key_path: "services.*.ports".into(),
                                    },
                                    Authority::Enforced,
                                    self.id(),
                                ));
                            }
                        }
                    }
                }
            }
            return results;
        }
        vec![]
    }
}

// --- Dockerfile EXPOSE extractor ---

pub struct DockerfilePortExtractor;

impl Extractor for DockerfilePortExtractor {
    fn id(&self) -> &str {
        "port-dockerfile"
    }
    fn description(&self) -> &str {
        "Port from Dockerfile EXPOSE"
    }
    fn relevant_filenames(&self) -> Vec<&str> {
        vec!["Dockerfile"]
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::Dockerfile(ref instructions) = file.content {
            let mut results = Vec::new();
            for instr in instructions {
                if instr.instruction == "EXPOSE" {
                    // EXPOSE can have multiple ports: "EXPOSE 3000 8080"
                    for port_str in instr.arguments.split_whitespace() {
                        // Strip /tcp or /udp suffix
                        let clean = port_str.split('/').next().unwrap_or(port_str);
                        if let Some(port) = parse_port(clean) {
                            results.push(ConfigAssertion::new(
                                SemanticConcept::app_port(),
                                SemanticType::Port(port),
                                port_str.to_string(),
                                SourceLocation {
                                    file: file.path.clone(),
                                    line: instr.line,
                                    column: 0,
                                    key_path: "EXPOSE".into(),
                                },
                                Authority::Declared,
                                self.id(),
                            ));
                        }
                    }
                }
            }
            return results;
        }
        vec![]
    }
}

/// Resolve ${VAR:-default} to the default value.
fn resolve_env_default(raw: &str) -> Option<String> {
    ENV_DEFAULT_RE.captures(raw).map(|caps| caps[2].to_string())
}

fn parse_compose_port_entry(value: &serde_json::Value) -> Option<(String, PortSpec, Vec<String>)> {
    let entry: ComposePortEntry = serde_json::from_value(value.clone()).ok()?;

    match entry {
        ComposePortEntry::Scalar(raw) => {
            let port = parse_compose_short_port(&raw)?;
            Some((raw.clone(), port, vec![raw]))
        }
        ComposePortEntry::Number(raw) => {
            let raw = raw.to_string();
            let port = parse_compose_short_port(&raw)?;
            Some((raw.clone(), port, vec![raw]))
        }
        ComposePortEntry::Mapping(mapping) => parse_compose_long_form_port(mapping),
    }
}

fn parse_compose_long_form_port(
    mapping: ComposePortMapping,
) -> Option<(String, PortSpec, Vec<String>)> {
    let target = mapping.target.as_u16()?;
    let target_display = mapping.target.display();
    let protocol_suffix = mapping
        .protocol
        .as_deref()
        .filter(|protocol| !protocol.is_empty() && !protocol.eq_ignore_ascii_case("tcp"))
        .map(|protocol| format!("/{}", protocol))
        .unwrap_or_default();

    if let Some(published) = mapping.published {
        let published_display = published.display();
        let port = if let Some(host) = published.as_u16() {
            PortSpec::Mapping {
                host,
                container: target,
            }
        } else if parse_port(&published_display).is_some() {
            // Ranged published ports still expose a single container target, which is the
            // application port we compare across files.
            PortSpec::Single(target)
        } else {
            return None;
        };
        Some((
            format!(
                "{}:{}{}",
                published_display, target_display, protocol_suffix
            ),
            port,
            vec![
                format!("published: {}", published_display),
                format!("target: {}", target_display),
                published_display,
                target_display,
            ],
        ))
    } else {
        Some((
            format!("{}{}", target_display, protocol_suffix),
            PortSpec::Single(target),
            vec![format!("target: {}", target_display), target_display],
        ))
    }
}

fn find_line_for_hints(raw_text: &str, hints: &[String]) -> usize {
    hints
        .iter()
        .find_map(|hint| find_exact_text_span(raw_text, hint).map(|span| span.line))
        .unwrap_or(1)
}

fn parse_compose_short_port(raw: &str) -> Option<PortSpec> {
    let trimmed = strip_port_protocol(raw);
    let segments = split_compose_port_segments(trimmed);

    match segments.as_slice() {
        [] => None,
        [container] => parse_compose_port_fragment(container),
        [published, container] => {
            let container = parse_compose_port_fragment(container)?;
            compose_short_port_spec(published, container)
        }
        _ => {
            let container = parse_compose_port_fragment(segments.last()?)?;
            let published = segments.get(segments.len() - 2)?;
            compose_short_port_spec(published, container)
        }
    }
}

fn strip_port_protocol(raw: &str) -> &str {
    let trimmed = raw.trim();
    match trimmed.rsplit_once('/') {
        Some((value, suffix))
            if suffix.eq_ignore_ascii_case("tcp") || suffix.eq_ignore_ascii_case("udp") =>
        {
            value
        }
        _ => trimmed,
    }
}

fn split_compose_port_segments(raw: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;

    for (index, ch) in raw.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => {
                segments.push(raw[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }

    segments.push(raw[start..].trim());
    segments
}

fn parse_compose_port_fragment(raw: &str) -> Option<PortSpec> {
    let trimmed = raw.trim();

    if trimmed.is_empty() {
        return None;
    }

    if let Some((start, end)) = trimmed.split_once('-') {
        let start = start.trim().parse::<u16>().ok()?;
        let end = end.trim().parse::<u16>().ok()?;
        return Some(PortSpec::Range(start, end));
    }

    let port = trimmed.parse::<u16>().ok()?;
    Some(PortSpec::Single(port))
}

fn compose_short_port_spec(published: &str, container: PortSpec) -> Option<PortSpec> {
    let published = parse_compose_port_fragment(published)?;

    match (published, container) {
        (PortSpec::Single(host), PortSpec::Single(container)) => {
            Some(PortSpec::Mapping { host, container })
        }
        (_, container) => Some(container),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_compose_short_range_mapping_as_container_range() {
        let (_, port, _) =
            parse_compose_port_entry(&json!("9090-9091:8080-8081")).expect("port should parse");

        assert_eq!(port, PortSpec::Range(8080, 8081));
    }

    #[test]
    fn test_parse_compose_short_host_range_single_container_port() {
        let (_, port, _) =
            parse_compose_port_entry(&json!("8000-9000:80")).expect("port should parse");

        assert_eq!(port, PortSpec::Single(80));
    }

    #[test]
    fn test_parse_compose_short_host_ip_range_mapping() {
        let (_, port, _) = parse_compose_port_entry(&json!("127.0.0.1:5000-5010:5000-5010"))
            .expect("port should parse");

        assert_eq!(port, PortSpec::Range(5000, 5010));
    }

    #[test]
    fn test_parse_compose_short_host_ip_without_host_port_is_rejected() {
        assert!(
            parse_compose_port_entry(&json!("127.0.0.1:8080")).is_none(),
            "host IP bindings still require a published host port in Compose short syntax"
        );
    }
}
