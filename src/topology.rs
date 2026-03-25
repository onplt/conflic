use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::parse::{FileContent, ParsedFile};

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// A node in the service topology graph.
#[derive(Debug, Clone)]
pub struct ServiceNode {
    /// Logical name (docker-compose service name, k8s metadata.name).
    pub name: String,
    /// Source file where this service is defined.
    pub file: PathBuf,
    /// Line number of the definition.
    pub line: usize,
    /// Container ports this service exposes.
    pub container_ports: Vec<u16>,
    /// Host→container port mappings (docker-compose only).
    pub port_mappings: Vec<(u16, u16)>,
    /// Environment variables (key → value).
    pub environment: HashMap<String, String>,
    /// Explicit dependencies (depends_on, links).
    pub depends_on: Vec<String>,
    /// Kubernetes labels on the workload.
    pub labels: HashMap<String, String>,
    /// Kubernetes selector (for Service kind).
    pub selector: HashMap<String, String>,
    /// The kind of resource (e.g., "docker-compose", "Deployment", "Service").
    pub kind: String,
}

/// An edge representing a dependency between two services.
#[derive(Debug, Clone)]
pub struct ServiceEdge {
    pub from: String,
    pub to: String,
    pub edge_kind: EdgeKind,
    /// Source file and line of the reference.
    pub file: PathBuf,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeKind {
    DependsOn,
    Link,
    EnvReference,
    SelectorMatch,
}

impl std::fmt::Display for EdgeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EdgeKind::DependsOn => write!(f, "depends_on"),
            EdgeKind::Link => write!(f, "link"),
            EdgeKind::EnvReference => write!(f, "env reference"),
            EdgeKind::SelectorMatch => write!(f, "selector"),
        }
    }
}

/// The full service topology graph.
#[derive(Debug, Default)]
pub struct ServiceGraph {
    pub nodes: BTreeMap<String, ServiceNode>,
    pub edges: Vec<ServiceEdge>,
}

// ---------------------------------------------------------------------------
// Topology findings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TopologyFinding {
    pub rule_id: String,
    pub severity: TopologySeverity,
    pub message: String,
    pub file: PathBuf,
    pub line: usize,
    pub related_file: Option<PathBuf>,
    pub related_line: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopologySeverity {
    Error,
    Warning,
}

impl std::fmt::Display for TopologySeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TopologySeverity::Error => write!(f, "error"),
            TopologySeverity::Warning => write!(f, "warning"),
        }
    }
}

/// Result of topology analysis.
#[derive(Debug)]
pub struct TopologyReport {
    pub graph: ServiceGraph,
    pub findings: Vec<TopologyFinding>,
}

// ---------------------------------------------------------------------------
// Graph extraction
// ---------------------------------------------------------------------------

/// Build a service graph from a set of parsed files.
pub fn build_service_graph(parsed_files: &[ParsedFile], scan_root: &Path) -> ServiceGraph {
    let mut graph = ServiceGraph::default();

    for file in parsed_files {
        let rel = file
            .path
            .strip_prefix(scan_root)
            .unwrap_or(&file.path);
        let filename = file
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        if filename.starts_with("docker-compose") && (filename.ends_with(".yml") || filename.ends_with(".yaml")) {
            if let FileContent::Yaml(ref value) = file.content {
                extract_compose_services(value, rel, file, &mut graph);
            }
        } else if is_k8s_filename(&filename) || has_k8s_path_hint(&file.path) {
            if let FileContent::Yaml(ref value) = file.content {
                extract_k8s_resources(value, rel, file, &mut graph);
            }
        }
    }

    // Build edges from depends_on, links, and env references
    build_dependency_edges(&mut graph);

    // Build edges from k8s selector matches
    build_k8s_selector_edges(&mut graph);

    graph
}

fn is_k8s_filename(lower: &str) -> bool {
    let k8s_stems = [
        "deployment", "statefulset", "service", "job", "cronjob", "pod", "daemonset",
    ];
    k8s_stems.iter().any(|stem| {
        lower.starts_with(stem)
            && (lower.ends_with(".yml") || lower.ends_with(".yaml"))
    })
}

fn has_k8s_path_hint(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    (path_str.contains("k8s") || path_str.contains("kubernetes") || path_str.contains("manifests"))
        && (path_str.ends_with(".yml") || path_str.ends_with(".yaml"))
}

fn extract_compose_services(
    value: &serde_json::Value,
    rel_path: &Path,
    file: &ParsedFile,
    graph: &mut ServiceGraph,
) {
    let Some(services) = value.get("services").and_then(|v| v.as_object()) else {
        return;
    };

    for (svc_name, svc_config) in services {
        let line = find_line(&file.raw_text, svc_name);

        let mut node = ServiceNode {
            name: svc_name.clone(),
            file: rel_path.to_path_buf(),
            line,
            container_ports: Vec::new(),
            port_mappings: Vec::new(),
            environment: HashMap::new(),
            depends_on: Vec::new(),
            labels: HashMap::new(),
            selector: HashMap::new(),
            kind: "docker-compose".into(),
        };

        // Extract ports
        if let Some(ports) = svc_config.get("ports").and_then(|v| v.as_array()) {
            for port_val in ports {
                extract_compose_port(port_val, &mut node);
            }
        }

        // Extract environment
        match svc_config.get("environment") {
            Some(serde_json::Value::Object(map)) => {
                for (k, v) in map {
                    let val = match v {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::Bool(b) => b.to_string(),
                        _ => continue,
                    };
                    node.environment.insert(k.clone(), val);
                }
            }
            Some(serde_json::Value::Array(arr)) => {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        if let Some((k, v)) = s.split_once('=') {
                            node.environment.insert(k.trim().into(), v.trim().into());
                        }
                    }
                }
            }
            _ => {}
        }

        // Extract depends_on
        match svc_config.get("depends_on") {
            Some(serde_json::Value::Array(arr)) => {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        node.depends_on.push(s.into());
                    }
                }
            }
            Some(serde_json::Value::Object(map)) => {
                for k in map.keys() {
                    node.depends_on.push(k.clone());
                }
            }
            _ => {}
        }

        // Extract links
        if let Some(links) = svc_config.get("links").and_then(|v| v.as_array()) {
            for link in links {
                if let Some(s) = link.as_str() {
                    // links can be "service" or "service:alias"
                    let target = s.split(':').next().unwrap_or(s);
                    if !node.depends_on.contains(&target.to_string()) {
                        node.depends_on.push(target.into());
                    }
                }
            }
        }

        let key = format!("compose:{}", svc_name);
        graph.nodes.insert(key, node);
    }
}

fn extract_compose_port(port_val: &serde_json::Value, node: &mut ServiceNode) {
    match port_val {
        serde_json::Value::String(s) => parse_compose_port_str(s, node),
        serde_json::Value::Number(n) => {
            if let Some(p) = n.as_u64().and_then(|v| u16::try_from(v).ok()) {
                node.container_ports.push(p);
            }
        }
        serde_json::Value::Object(map) => {
            if let Some(target) = map.get("target") {
                let port = target
                    .as_u64()
                    .and_then(|v| u16::try_from(v).ok())
                    .or_else(|| target.as_str().and_then(|s| s.parse().ok()));
                if let Some(p) = port {
                    node.container_ports.push(p);
                    if let Some(published) = map.get("published") {
                        let host = published
                            .as_u64()
                            .and_then(|v| u16::try_from(v).ok())
                            .or_else(|| published.as_str().and_then(|s| s.parse().ok()));
                        if let Some(h) = host {
                            node.port_mappings.push((h, p));
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn parse_compose_port_str(s: &str, node: &mut ServiceNode) {
    let trimmed = s.trim();
    // Strip protocol suffix
    let trimmed = trimmed.split('/').next().unwrap_or(trimmed);

    // Split on ':' respecting brackets
    let segments: Vec<&str> = {
        let mut segs = Vec::new();
        let mut depth: usize = 0;
        let mut start = 0;
        for (i, c) in trimmed.char_indices() {
            match c {
                '[' => depth += 1,
                ']' => depth = depth.saturating_sub(1),
                ':' if depth == 0 => {
                    segs.push(&trimmed[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        segs.push(&trimmed[start..]);
        segs
    };

    match segments.len() {
        1 => {
            // Just container port or range
            if let Some(p) = parse_port_or_range_first(segments[0]) {
                node.container_ports.push(p);
            }
        }
        2 => {
            // host:container
            if let Some(container) = parse_port_or_range_first(segments[1]) {
                node.container_ports.push(container);
                if let Some(host) = parse_port_or_range_first(segments[0]) {
                    node.port_mappings.push((host, container));
                }
            }
        }
        3.. => {
            // ip:host:container
            if let Some(container) = parse_port_or_range_first(segments.last().unwrap()) {
                node.container_ports.push(container);
                if segments.len() >= 2 {
                    if let Some(host) = parse_port_or_range_first(segments[segments.len() - 2]) {
                        node.port_mappings.push((host, container));
                    }
                }
            }
        }
        _ => {}
    }
}

fn parse_port_or_range_first(s: &str) -> Option<u16> {
    let s = s.trim();
    if let Some((start, _)) = s.split_once('-') {
        start.trim().parse().ok()
    } else {
        s.parse().ok()
    }
}

fn extract_k8s_resources(
    value: &serde_json::Value,
    rel_path: &Path,
    file: &ParsedFile,
    graph: &mut ServiceGraph,
) {
    let kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let name = value
        .get("metadata")
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if name.is_empty() {
        return;
    }

    let line = find_line(&file.raw_text, &name);
    let workload_kinds = ["Deployment", "StatefulSet", "DaemonSet", "ReplicaSet", "Pod", "Job", "CronJob"];

    if workload_kinds.contains(&kind) {
        let mut node = ServiceNode {
            name: name.clone(),
            file: rel_path.to_path_buf(),
            line,
            container_ports: Vec::new(),
            port_mappings: Vec::new(),
            environment: HashMap::new(),
            depends_on: Vec::new(),
            labels: HashMap::new(),
            selector: HashMap::new(),
            kind: kind.into(),
        };

        // Extract labels from template (used for selector matching)
        if let Some(labels) = value
            .get("spec")
            .and_then(|v| v.get("template"))
            .and_then(|v| v.get("metadata"))
            .and_then(|v| v.get("labels"))
            .and_then(|v| v.as_object())
        {
            for (k, v) in labels {
                if let Some(s) = v.as_str() {
                    node.labels.insert(k.clone(), s.into());
                }
            }
        }

        // Pod-level labels
        if let Some(labels) = value
            .get("metadata")
            .and_then(|v| v.get("labels"))
            .and_then(|v| v.as_object())
        {
            for (k, v) in labels {
                if let Some(s) = v.as_str() {
                    node.labels.entry(k.clone()).or_insert_with(|| s.into());
                }
            }
        }

        // Extract container ports
        let containers = get_k8s_containers(value);
        for container in &containers {
            if let Some(ports) = container.get("ports").and_then(|v| v.as_array()) {
                for port_obj in ports {
                    if let Some(cp) = port_obj.get("containerPort").and_then(|v| v.as_u64()) {
                        if let Ok(p) = u16::try_from(cp) {
                            node.container_ports.push(p);
                        }
                    }
                }
            }
        }

        // Extract environment variables
        for container in &containers {
            if let Some(env_arr) = container.get("env").and_then(|v| v.as_array()) {
                for env_obj in env_arr {
                    if let (Some(k), Some(v)) = (
                        env_obj.get("name").and_then(|v| v.as_str()),
                        env_obj.get("value").and_then(|v| v.as_str()),
                    ) {
                        node.environment.insert(k.into(), v.into());
                    }
                }
            }
        }

        let key = format!("k8s:{}:{}", kind.to_lowercase(), name);
        graph.nodes.insert(key, node);
    } else if kind == "Service" {
        let mut node = ServiceNode {
            name: name.clone(),
            file: rel_path.to_path_buf(),
            line,
            container_ports: Vec::new(),
            port_mappings: Vec::new(),
            environment: HashMap::new(),
            depends_on: Vec::new(),
            labels: HashMap::new(),
            selector: HashMap::new(),
            kind: "Service".into(),
        };

        // Extract selector
        if let Some(selector) = value
            .get("spec")
            .and_then(|v| v.get("selector"))
            .and_then(|v| v.as_object())
        {
            for (k, v) in selector {
                if let Some(s) = v.as_str() {
                    node.selector.insert(k.clone(), s.into());
                }
            }
        }

        // Extract target ports
        if let Some(ports) = value
            .get("spec")
            .and_then(|v| v.get("ports"))
            .and_then(|v| v.as_array())
        {
            for port_obj in ports {
                if let Some(tp) = port_obj.get("targetPort").and_then(|v| v.as_u64()) {
                    if let Ok(p) = u16::try_from(tp) {
                        node.container_ports.push(p);
                    }
                }
                // Also check port (the service port, used for host-side mapping)
                if let Some(sp) = port_obj.get("port").and_then(|v| v.as_u64()) {
                    if let Ok(service_port) = u16::try_from(sp) {
                        if let Some(tp) = port_obj.get("targetPort").and_then(|v| v.as_u64()) {
                            if let Ok(target) = u16::try_from(tp) {
                                node.port_mappings.push((service_port, target));
                            }
                        }
                    }
                }
            }
        }

        let key = format!("k8s:service:{}", name);
        graph.nodes.insert(key, node);
    }
}

fn get_k8s_containers(value: &serde_json::Value) -> Vec<&serde_json::Value> {
    let mut containers = Vec::new();

    // Pod spec
    if let Some(arr) = value
        .get("spec")
        .and_then(|v| v.get("containers"))
        .and_then(|v| v.as_array())
    {
        containers.extend(arr.iter());
    }

    // Deployment/StatefulSet
    if let Some(arr) = value
        .get("spec")
        .and_then(|v| v.get("template"))
        .and_then(|v| v.get("spec"))
        .and_then(|v| v.get("containers"))
        .and_then(|v| v.as_array())
    {
        containers.extend(arr.iter());
    }

    // CronJob
    if let Some(arr) = value
        .get("spec")
        .and_then(|v| v.get("jobTemplate"))
        .and_then(|v| v.get("spec"))
        .and_then(|v| v.get("template"))
        .and_then(|v| v.get("spec"))
        .and_then(|v| v.get("containers"))
        .and_then(|v| v.as_array())
    {
        containers.extend(arr.iter());
    }

    containers
}

fn build_dependency_edges(graph: &mut ServiceGraph) {
    let compose_names: HashMap<&str, &str> = graph
        .nodes
        .iter()
        .filter(|(k, _)| k.starts_with("compose:"))
        .map(|(k, n)| (n.name.as_str(), k.as_str()))
        .collect();

    let mut edges = Vec::new();

    for (key, node) in &graph.nodes {
        if !key.starts_with("compose:") {
            continue;
        }

        // depends_on edges
        for dep in &node.depends_on {
            let edge_kind = if node.depends_on.contains(dep) {
                EdgeKind::DependsOn
            } else {
                EdgeKind::Link
            };
            edges.push(ServiceEdge {
                from: key.clone(),
                to: format!("compose:{}", dep),
                edge_kind,
                file: node.file.clone(),
                line: node.line,
            });
        }

        // Environment variable references to other services
        for (env_key, env_val) in &node.environment {
            for (svc_name, _svc_key) in &compose_names {
                if *svc_name == node.name {
                    continue;
                }
                // Check if env value references another service by name (host/URL pattern)
                if env_val.contains(svc_name)
                    && (env_key.to_uppercase().contains("HOST")
                        || env_key.to_uppercase().contains("URL")
                        || env_key.to_uppercase().contains("ADDR")
                        || env_key.to_uppercase().contains("ENDPOINT")
                        || env_key.to_uppercase().contains("DSN")
                        || env_key.to_uppercase().contains("CONNECTION")
                        || env_val.contains(&format!("{}:", svc_name)))
                {
                    edges.push(ServiceEdge {
                        from: key.clone(),
                        to: format!("compose:{}", svc_name),
                        edge_kind: EdgeKind::EnvReference,
                        file: node.file.clone(),
                        line: node.line,
                    });
                }
            }
        }
    }

    graph.edges.extend(edges);
}

fn build_k8s_selector_edges(graph: &mut ServiceGraph) {
    let services: Vec<(String, HashMap<String, String>, PathBuf, usize)> = graph
        .nodes
        .iter()
        .filter(|(_, n)| n.kind == "Service" && !n.selector.is_empty())
        .map(|(k, n)| (k.clone(), n.selector.clone(), n.file.clone(), n.line))
        .collect();

    let workloads: Vec<(String, HashMap<String, String>)> = graph
        .nodes
        .iter()
        .filter(|(k, _)| k.starts_with("k8s:") && !k.starts_with("k8s:service:"))
        .map(|(k, n)| (k.clone(), n.labels.clone()))
        .collect();

    for (svc_key, selector, file, line) in &services {
        for (wl_key, labels) in &workloads {
            if selector_matches(selector, labels) {
                graph.edges.push(ServiceEdge {
                    from: svc_key.clone(),
                    to: wl_key.clone(),
                    edge_kind: EdgeKind::SelectorMatch,
                    file: file.clone(),
                    line: *line,
                });
            }
        }
    }
}

fn selector_matches(selector: &HashMap<String, String>, labels: &HashMap<String, String>) -> bool {
    if selector.is_empty() {
        return false;
    }
    selector
        .iter()
        .all(|(k, v)| labels.get(k).map_or(false, |lv| lv == v))
}

// ---------------------------------------------------------------------------
// Topology analysis
// ---------------------------------------------------------------------------

/// Analyze the service graph for topology contradictions.
pub fn analyze_topology(parsed_files: &[ParsedFile], scan_root: &Path) -> TopologyReport {
    let graph = build_service_graph(parsed_files, scan_root);
    let mut findings = Vec::new();

    check_dangling_depends_on(&graph, &mut findings);
    check_host_port_conflicts(&graph, &mut findings);
    check_env_port_mismatches(&graph, &mut findings);
    check_k8s_service_port_mismatch(&graph, &mut findings);
    check_k8s_dangling_selectors(&graph, &mut findings);

    TopologyReport { graph, findings }
}

/// TOPO001: depends_on references a non-existent service.
fn check_dangling_depends_on(graph: &ServiceGraph, findings: &mut Vec<TopologyFinding>) {
    let compose_names: HashSet<String> = graph
        .nodes
        .iter()
        .filter(|(k, _)| k.starts_with("compose:"))
        .map(|(_, n)| n.name.clone())
        .collect();

    for (_, node) in graph.nodes.iter().filter(|(k, _)| k.starts_with("compose:")) {
        for dep in &node.depends_on {
            if !compose_names.contains(dep) {
                findings.push(TopologyFinding {
                    rule_id: "TOPO001".into(),
                    severity: TopologySeverity::Error,
                    message: format!(
                        "Service '{}' depends on '{}', but '{}' is not defined in {}",
                        node.name,
                        dep,
                        dep,
                        node.file.display()
                    ),
                    file: node.file.clone(),
                    line: node.line,
                    related_file: None,
                    related_line: None,
                });
            }
        }
    }
}

/// TOPO002: Multiple compose services map the same host port.
fn check_host_port_conflicts(graph: &ServiceGraph, findings: &mut Vec<TopologyFinding>) {
    // Group by file to only check services in the same compose file
    let mut by_file: HashMap<&Path, Vec<&ServiceNode>> = HashMap::new();
    for (key, node) in &graph.nodes {
        if key.starts_with("compose:") {
            by_file.entry(node.file.as_path()).or_default().push(node);
        }
    }

    for (file, nodes) in &by_file {
        let mut host_ports: HashMap<u16, Vec<&str>> = HashMap::new();
        for node in nodes {
            for &(host, _container) in &node.port_mappings {
                host_ports.entry(host).or_default().push(&node.name);
            }
        }

        for (port, services) in &host_ports {
            if services.len() > 1 {
                findings.push(TopologyFinding {
                    rule_id: "TOPO002".into(),
                    severity: TopologySeverity::Error,
                    message: format!(
                        "Host port {} is mapped by multiple services: {}",
                        port,
                        services.join(", ")
                    ),
                    file: file.to_path_buf(),
                    line: 1,
                    related_file: None,
                    related_line: None,
                });
            }
        }
    }
}

/// TOPO003: Env var references a service with a port that doesn't match.
fn check_env_port_mismatches(graph: &ServiceGraph, findings: &mut Vec<TopologyFinding>) {
    // For each env-reference edge, check if the port in the env value matches
    // the target service's exposed ports
    let port_re = regex::Regex::new(r":(\d{2,5})(?:[/\s]|$)").unwrap();

    for edge in &graph.edges {
        if edge.edge_kind != EdgeKind::EnvReference {
            continue;
        }

        let Some(from_node) = graph.nodes.get(&edge.from) else {
            continue;
        };
        let Some(to_node) = graph.nodes.get(&edge.to) else {
            continue;
        };

        if to_node.container_ports.is_empty() {
            continue;
        }

        // Find env values that reference the target service and contain a port
        for (_env_key, env_val) in &from_node.environment {
            if !env_val.contains(&to_node.name) {
                continue;
            }

            for cap in port_re.captures_iter(env_val) {
                if let Ok(port) = cap[1].parse::<u16>() {
                    if !to_node.container_ports.contains(&port) {
                        findings.push(TopologyFinding {
                            rule_id: "TOPO003".into(),
                            severity: TopologySeverity::Warning,
                            message: format!(
                                "Service '{}' references '{}' on port {}, but '{}' exposes port(s) {}",
                                from_node.name,
                                to_node.name,
                                port,
                                to_node.name,
                                to_node
                                    .container_ports
                                    .iter()
                                    .map(|p| p.to_string())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                            file: from_node.file.clone(),
                            line: from_node.line,
                            related_file: Some(to_node.file.clone()),
                            related_line: Some(to_node.line),
                        });
                    }
                }
            }
        }
    }
}

/// TOPO004: K8s Service targetPort doesn't match any Deployment containerPort.
fn check_k8s_service_port_mismatch(graph: &ServiceGraph, findings: &mut Vec<TopologyFinding>) {
    for edge in &graph.edges {
        if edge.edge_kind != EdgeKind::SelectorMatch {
            continue;
        }

        let Some(svc_node) = graph.nodes.get(&edge.from) else {
            continue;
        };
        let Some(wl_node) = graph.nodes.get(&edge.to) else {
            continue;
        };

        if wl_node.container_ports.is_empty() || svc_node.container_ports.is_empty() {
            continue;
        }

        for &target_port in &svc_node.container_ports {
            if !wl_node.container_ports.contains(&target_port) {
                findings.push(TopologyFinding {
                    rule_id: "TOPO004".into(),
                    severity: TopologySeverity::Error,
                    message: format!(
                        "Service '{}' targets port {} via selector, but {} '{}' exposes port(s) {}",
                        svc_node.name,
                        target_port,
                        wl_node.kind,
                        wl_node.name,
                        wl_node
                            .container_ports
                            .iter()
                            .map(|p| p.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    file: svc_node.file.clone(),
                    line: svc_node.line,
                    related_file: Some(wl_node.file.clone()),
                    related_line: Some(wl_node.line),
                });
            }
        }
    }
}

/// TOPO005: K8s Service selector doesn't match any workload.
fn check_k8s_dangling_selectors(graph: &ServiceGraph, findings: &mut Vec<TopologyFinding>) {
    let service_keys: Vec<&String> = graph
        .nodes
        .keys()
        .filter(|k| k.starts_with("k8s:service:"))
        .collect();

    let matched_services: HashSet<&str> = graph
        .edges
        .iter()
        .filter(|e| e.edge_kind == EdgeKind::SelectorMatch)
        .map(|e| e.from.as_str())
        .collect();

    for key in service_keys {
        let node = &graph.nodes[key];
        if !node.selector.is_empty() && !matched_services.contains(key.as_str()) {
            let selector_str = node
                .selector
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(", ");

            findings.push(TopologyFinding {
                rule_id: "TOPO005".into(),
                severity: TopologySeverity::Warning,
                message: format!(
                    "Service '{}' selector ({}) does not match any workload",
                    node.name, selector_str
                ),
                file: node.file.clone(),
                line: node.line,
                related_file: None,
                related_line: None,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

pub fn render_topology_report(report: &TopologyReport, no_color: bool) -> String {
    use owo_colors::OwoColorize;

    let mut out = String::new();

    // Graph summary
    let service_count = report.graph.nodes.len();
    let edge_count = report.graph.edges.len();

    if no_color {
        out.push_str(&format!(
            "Service Topology: {} services, {} dependencies\n",
            service_count, edge_count
        ));
    } else {
        out.push_str(&format!(
            "{} {} services, {} dependencies\n",
            "Service Topology:".bold(),
            service_count,
            edge_count
        ));
    }

    if service_count > 0 {
        out.push('\n');

        // List services with their ports
        for (_, node) in &report.graph.nodes {
            let ports = if node.container_ports.is_empty() {
                String::new()
            } else {
                format!(
                    " [ports: {}]",
                    node.container_ports
                        .iter()
                        .map(|p| p.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };

            if no_color {
                out.push_str(&format!(
                    "  {} ({}){}\n",
                    node.name,
                    node.kind,
                    ports
                ));
            } else {
                out.push_str(&format!(
                    "  {} ({}){}\n",
                    node.name.bold(),
                    node.kind.dimmed(),
                    ports
                ));
            }
        }

        // List edges
        if !report.graph.edges.is_empty() {
            out.push('\n');
            if no_color {
                out.push_str("  Dependencies:\n");
            } else {
                out.push_str(&format!("  {}\n", "Dependencies:".dimmed()));
            }
            for edge in &report.graph.edges {
                let from_name = report
                    .graph
                    .nodes
                    .get(&edge.from)
                    .map(|n| n.name.as_str())
                    .unwrap_or(&edge.from);
                let to_name = report
                    .graph
                    .nodes
                    .get(&edge.to)
                    .map(|n| n.name.as_str())
                    .unwrap_or(&edge.to);

                out.push_str(&format!(
                    "    {} -> {} ({})\n",
                    from_name, to_name, edge.edge_kind
                ));
            }
        }
    }

    // Findings
    if report.findings.is_empty() {
        out.push('\n');
        if no_color {
            out.push_str("No topology issues found.\n");
        } else {
            out.push_str(&format!("{}\n", "No topology issues found.".green()));
        }
    } else {
        out.push('\n');
        let errors = report
            .findings
            .iter()
            .filter(|f| f.severity == TopologySeverity::Error)
            .count();
        let warnings = report
            .findings
            .iter()
            .filter(|f| f.severity == TopologySeverity::Warning)
            .count();

        if no_color {
            out.push_str(&format!(
                "Topology issues: {} error(s), {} warning(s)\n\n",
                errors, warnings
            ));
        } else {
            out.push_str(&format!(
                "{} {} error(s), {} warning(s)\n\n",
                "Topology issues:".bold(),
                errors,
                warnings
            ));
        }

        for finding in &report.findings {
            let severity_str = match finding.severity {
                TopologySeverity::Error => {
                    if no_color {
                        "ERROR".to_string()
                    } else {
                        format!("{}", "ERROR".red().bold())
                    }
                }
                TopologySeverity::Warning => {
                    if no_color {
                        "WARNING".to_string()
                    } else {
                        format!("{}", "WARNING".yellow().bold())
                    }
                }
            };

            out.push_str(&format!(
                "  [{}] {} ({})\n",
                finding.rule_id, severity_str, finding.message
            ));
            out.push_str(&format!("    at {}:{}\n", finding.file.display(), finding.line));

            if let (Some(rf), Some(rl)) = (&finding.related_file, finding.related_line) {
                out.push_str(&format!("    see {}:{}\n", rf.display(), rl));
            }
            out.push('\n');
        }
    }

    out
}

pub fn render_topology_json(report: &TopologyReport) -> String {
    let nodes: Vec<serde_json::Value> = report
        .graph
        .nodes
        .values()
        .map(|n| {
            serde_json::json!({
                "name": n.name,
                "kind": n.kind,
                "file": n.file.display().to_string(),
                "line": n.line,
                "container_ports": n.container_ports,
                "port_mappings": n.port_mappings.iter().map(|(h, c)| {
                    serde_json::json!({"host": h, "container": c})
                }).collect::<Vec<_>>(),
                "depends_on": n.depends_on,
            })
        })
        .collect();

    let edges: Vec<serde_json::Value> = report
        .graph
        .edges
        .iter()
        .map(|e| {
            serde_json::json!({
                "from": report.graph.nodes.get(&e.from).map(|n| &n.name).unwrap_or(&e.from),
                "to": report.graph.nodes.get(&e.to).map(|n| &n.name).unwrap_or(&e.to),
                "kind": e.edge_kind.to_string(),
            })
        })
        .collect();

    let findings: Vec<serde_json::Value> = report
        .findings
        .iter()
        .map(|f| {
            let mut obj = serde_json::json!({
                "rule_id": f.rule_id,
                "severity": f.severity.to_string(),
                "message": f.message,
                "file": f.file.display().to_string(),
                "line": f.line,
            });
            if let Some(ref rf) = f.related_file {
                obj["related_file"] = serde_json::json!(rf.display().to_string());
            }
            if let Some(rl) = f.related_line {
                obj["related_line"] = serde_json::json!(rl);
            }
            obj
        })
        .collect();

    let result = serde_json::json!({
        "services": nodes,
        "edges": edges,
        "findings": findings,
        "summary": {
            "service_count": report.graph.nodes.len(),
            "edge_count": report.graph.edges.len(),
            "error_count": report.findings.iter().filter(|f| f.severity == TopologySeverity::Error).count(),
            "warning_count": report.findings.iter().filter(|f| f.severity == TopologySeverity::Warning).count(),
        }
    });

    serde_json::to_string_pretty(&result).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn find_line(raw_text: &str, needle: &str) -> usize {
    for (idx, line) in raw_text.lines().enumerate() {
        if line.contains(needle) {
            return idx + 1;
        }
    }
    1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use std::path::PathBuf;

    fn make_parsed_file(path: &str, content: &str) -> ParsedFile {
        let path = PathBuf::from(path);
        let scan_root = PathBuf::from(".");
        parse::parse_file_with_content(&path, &scan_root, content.to_string()).unwrap()
    }

    #[test]
    fn test_compose_service_extraction() {
        let yaml = r#"
services:
  web:
    image: node:20
    ports:
      - "8080:3000"
    depends_on:
      - db
  db:
    image: postgres:15
    ports:
      - "5432:5432"
"#;
        let file = make_parsed_file("docker-compose.yml", yaml);
        let graph = build_service_graph(&[file], Path::new("."));

        assert_eq!(graph.nodes.len(), 2);
        assert!(graph.nodes.contains_key("compose:web"));
        assert!(graph.nodes.contains_key("compose:db"));

        let web = &graph.nodes["compose:web"];
        assert_eq!(web.container_ports, vec![3000]);
        assert_eq!(web.port_mappings, vec![(8080, 3000)]);
        assert_eq!(web.depends_on, vec!["db"]);

        let db = &graph.nodes["compose:db"];
        assert_eq!(db.container_ports, vec![5432]);
    }

    #[test]
    fn test_dangling_depends_on() {
        let yaml = r#"
services:
  web:
    image: node:20
    depends_on:
      - redis
"#;
        let file = make_parsed_file("docker-compose.yml", yaml);
        let report = analyze_topology(&[file], Path::new("."));

        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].rule_id, "TOPO001");
        assert!(report.findings[0].message.contains("redis"));
        assert!(report.findings[0].message.contains("not defined"));
    }

    #[test]
    fn test_host_port_conflict() {
        let yaml = r#"
services:
  web:
    image: node:20
    ports:
      - "8080:3000"
  api:
    image: node:20
    ports:
      - "8080:4000"
"#;
        let file = make_parsed_file("docker-compose.yml", yaml);
        let report = analyze_topology(&[file], Path::new("."));

        let topo002: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.rule_id == "TOPO002")
            .collect();
        assert_eq!(topo002.len(), 1);
        assert!(topo002[0].message.contains("8080"));
        assert!(topo002[0].message.contains("web"));
        assert!(topo002[0].message.contains("api"));
    }

    #[test]
    fn test_env_port_mismatch() {
        let yaml = r#"
services:
  web:
    image: node:20
    ports:
      - "3000"
    environment:
      DATABASE_URL: "postgres://db:5433/mydb"
    depends_on:
      - db
  db:
    image: postgres:15
    ports:
      - "5432"
"#;
        let file = make_parsed_file("docker-compose.yml", yaml);
        let report = analyze_topology(&[file], Path::new("."));

        let topo003: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.rule_id == "TOPO003")
            .collect();
        assert_eq!(topo003.len(), 1);
        assert!(topo003[0].message.contains("5433"));
        assert!(topo003[0].message.contains("5432"));
    }

    #[test]
    fn test_no_false_positive_when_ports_match() {
        let yaml = r#"
services:
  web:
    image: node:20
    environment:
      DATABASE_URL: "postgres://db:5432/mydb"
    depends_on:
      - db
  db:
    image: postgres:15
    ports:
      - "5432"
"#;
        let file = make_parsed_file("docker-compose.yml", yaml);
        let report = analyze_topology(&[file], Path::new("."));

        let topo003: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.rule_id == "TOPO003")
            .collect();
        assert!(topo003.is_empty(), "Should not flag matching ports");
    }

    #[test]
    fn test_k8s_service_port_mismatch() {
        let deployment = r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: web
spec:
  template:
    metadata:
      labels:
        app: web
    spec:
      containers:
        - name: app
          image: node:20
          ports:
            - containerPort: 3000
"#;
        let service = r#"
apiVersion: v1
kind: Service
metadata:
  name: web-svc
spec:
  selector:
    app: web
  ports:
    - port: 80
      targetPort: 8080
"#;
        let dep_file = make_parsed_file("deployment.yaml", deployment);
        let svc_file = make_parsed_file("service.yaml", service);
        let report = analyze_topology(&[dep_file, svc_file], Path::new("."));

        let topo004: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.rule_id == "TOPO004")
            .collect();
        assert_eq!(topo004.len(), 1);
        assert!(topo004[0].message.contains("8080"));
        assert!(topo004[0].message.contains("3000"));
    }

    #[test]
    fn test_k8s_service_matching_port_no_finding() {
        let deployment = r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: web
spec:
  template:
    metadata:
      labels:
        app: web
    spec:
      containers:
        - name: app
          image: node:20
          ports:
            - containerPort: 3000
"#;
        let service = r#"
apiVersion: v1
kind: Service
metadata:
  name: web-svc
spec:
  selector:
    app: web
  ports:
    - port: 80
      targetPort: 3000
"#;
        let dep_file = make_parsed_file("deployment.yaml", deployment);
        let svc_file = make_parsed_file("service.yaml", service);
        let report = analyze_topology(&[dep_file, svc_file], Path::new("."));

        let topo004: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.rule_id == "TOPO004")
            .collect();
        assert!(topo004.is_empty(), "Matching ports should produce no finding");
    }

    #[test]
    fn test_k8s_dangling_selector() {
        let service = r#"
apiVersion: v1
kind: Service
metadata:
  name: orphan-svc
spec:
  selector:
    app: nonexistent
  ports:
    - port: 80
      targetPort: 3000
"#;
        let svc_file = make_parsed_file("service.yaml", service);
        let report = analyze_topology(&[svc_file], Path::new("."));

        let topo005: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.rule_id == "TOPO005")
            .collect();
        assert_eq!(topo005.len(), 1);
        assert!(topo005[0].message.contains("orphan-svc"));
        assert!(topo005[0].message.contains("does not match"));
    }

    #[test]
    fn test_compose_environment_list_format() {
        let yaml = r#"
services:
  web:
    image: node:20
    environment:
      - REDIS_HOST=cache
      - REDIS_PORT=6379
    depends_on:
      - cache
  cache:
    image: redis:7
    ports:
      - "6379"
"#;
        let file = make_parsed_file("docker-compose.yml", yaml);
        let graph = build_service_graph(&[file], Path::new("."));

        let web = &graph.nodes["compose:web"];
        assert_eq!(web.environment.get("REDIS_HOST").unwrap(), "cache");
        assert_eq!(web.environment.get("REDIS_PORT").unwrap(), "6379");
    }

    #[test]
    fn test_compose_depends_on_object_form() {
        let yaml = r#"
services:
  web:
    image: node:20
    depends_on:
      db:
        condition: service_healthy
      redis:
        condition: service_started
  db:
    image: postgres:15
  redis:
    image: redis:7
"#;
        let file = make_parsed_file("docker-compose.yml", yaml);
        let graph = build_service_graph(&[file], Path::new("."));

        let web = &graph.nodes["compose:web"];
        assert!(web.depends_on.contains(&"db".to_string()));
        assert!(web.depends_on.contains(&"redis".to_string()));
    }

    #[test]
    fn test_empty_graph_no_findings() {
        let report = analyze_topology(&[], Path::new("."));
        assert!(report.findings.is_empty());
        assert!(report.graph.nodes.is_empty());
    }

    #[test]
    fn test_render_topology_json() {
        let yaml = r#"
services:
  web:
    image: node:20
    ports:
      - "3000"
"#;
        let file = make_parsed_file("docker-compose.yml", yaml);
        let report = analyze_topology(&[file], Path::new("."));
        let json = render_topology_json(&report);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["summary"]["service_count"], 1);
        assert_eq!(parsed["services"][0]["name"], "web");
    }
}
