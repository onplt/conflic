use crate::model::*;
use crate::parse::*;
use crate::parse::source_location::*;
use super::Extractor;
use regex::Regex;

// --- .env PORT extractor ---

pub struct EnvPortExtractor;

impl Extractor for EnvPortExtractor {
    fn id(&self) -> &str { "port-env" }
    fn description(&self) -> &str { "Port from .env files" }
    fn relevant_filenames(&self) -> Vec<&str> { vec![".env"] }

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
                    let resolved = resolve_env_default(&entry.value)
                        .unwrap_or_else(|| entry.value.clone());

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
    fn id(&self) -> &str { "port-docker-compose" }
    fn description(&self) -> &str { "Port from docker-compose.yml" }
    fn relevant_filenames(&self) -> Vec<&str> { vec!["docker-compose"] }

    fn matches_file(&self, filename: &str) -> bool {
        filename.starts_with("docker-compose") && (filename.ends_with(".yml") || filename.ends_with(".yaml"))
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::Yaml(ref value) = file.content {
            let mut results = Vec::new();

            if let Some(services) = value.get("services").and_then(|s| s.as_mapping()) {
                for (_service_name, service_config) in services {
                    if let Some(ports) = service_config.get("ports").and_then(|p| p.as_sequence()) {
                        for port_val in ports {
                            if let Some(port_str) = port_val.as_str().or_else(|| {
                                port_val.as_i64().map(|_| "").and(None) // numbers handled below
                            }) {
                                let line = find_line_for_key(&file.raw_text, port_str);
                                if let Some(port) = parse_port(port_str) {
                                    results.push(ConfigAssertion::new(
                                        SemanticConcept::app_port(),
                                        SemanticType::Port(port),
                                        port_str.to_string(),
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
                            } else if let Some(port_num) = port_val.as_i64() {
                                let port_str = port_num.to_string();
                                let line = find_line_for_key(&file.raw_text, &port_str);
                                if let Some(port) = parse_port(&port_str) {
                                    results.push(ConfigAssertion::new(
                                        SemanticConcept::app_port(),
                                        SemanticType::Port(port),
                                        port_str,
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
            }
            return results;
        }
        vec![]
    }
}

// --- Dockerfile EXPOSE extractor ---

pub struct DockerfilePortExtractor;

impl Extractor for DockerfilePortExtractor {
    fn id(&self) -> &str { "port-dockerfile" }
    fn description(&self) -> &str { "Port from Dockerfile EXPOSE" }
    fn relevant_filenames(&self) -> Vec<&str> { vec!["Dockerfile"] }

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
    let re = Regex::new(r"\$\{(\w+):-([^}]*)\}").unwrap();
    if let Some(caps) = re.captures(raw) {
        Some(caps[2].to_string())
    } else {
        None
    }
}
