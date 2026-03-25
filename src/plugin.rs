//! Wasm Plugin API for programmatic extractors and solvers.
//!
//! Plugins are `.wasm` modules that export specific functions following the
//! conflic plugin ABI. Communication between host and guest uses JSON-encoded
//! messages passed through linear memory.
//!
//! # Plugin ABI
//!
//! ## Extractor plugins
//! Export: `conflic_extract(ptr: i32, len: i32) -> i64`
//!   - Input: JSON `{ "filename": "...", "content": "..." }`
//!   - Output: packed (ptr, len) as i64. Points to JSON:
//!     `[{ "concept_id": "...", "display_name": "...", "raw_value": "...",
//!          "line": N, "key_path": "...", "authority": "declared" }]`
//!
//! ## Solver plugins
//! Export: `conflic_solve(ptr: i32, len: i32) -> i64`
//!   - Input: JSON `{ "concept_id": "...", "left": "...", "right": "..." }`
//!   - Output: packed (ptr, len) as i64. Points to JSON:
//!     `{ "compatible": true/false, "explanation": "..." }`
//!
//! ## Memory management
//! Guest must export: `conflic_alloc(len: i32) -> i32`
//! Guest must export: `conflic_dealloc(ptr: i32, len: i32)`

use std::path::Path;

#[cfg(feature = "wasm")]
use crate::extract::Extractor;
#[cfg(feature = "wasm")]
use crate::model::*;
#[cfg(feature = "wasm")]
use crate::parse::ParsedFile;

// ---------------------------------------------------------------------------
// Plugin manifest
// ---------------------------------------------------------------------------

/// Configuration for a Wasm plugin declared in `.conflic.toml`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PluginConfig {
    /// Human-readable name for this plugin.
    pub name: String,
    /// Path to the .wasm module (relative to config file).
    pub path: String,
    /// What this plugin provides: "extractor", "solver", or "both".
    #[serde(default = "default_plugin_kind")]
    pub kind: String,
    /// Concept IDs this plugin handles (for solvers).
    #[serde(default)]
    pub concepts: Vec<String>,
    /// File patterns this plugin should match (for extractors).
    #[serde(default)]
    pub file_patterns: Vec<String>,
}

fn default_plugin_kind() -> String {
    "extractor".into()
}

// ---------------------------------------------------------------------------
// Protocol types (JSON messages)
// ---------------------------------------------------------------------------

#[cfg(any(feature = "wasm", test))]
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
struct ExtractInput<'a> {
    filename: &'a str,
    content: &'a str,
}

#[cfg(any(feature = "wasm", test))]
#[derive(serde::Serialize, serde::Deserialize)]
struct ExtractOutput {
    concept_id: String,
    #[serde(default)]
    display_name: String,
    raw_value: String,
    #[serde(default = "default_line")]
    line: usize,
    #[serde(default)]
    key_path: String,
    #[serde(default = "default_authority")]
    authority: String,
}

#[cfg(any(feature = "wasm", test))]
fn default_line() -> usize {
    1
}

#[cfg(any(feature = "wasm", test))]
fn default_authority() -> String {
    "declared".into()
}

#[cfg(any(feature = "wasm", test))]
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
struct SolveInput<'a> {
    concept_id: &'a str,
    left: &'a str,
    right: &'a str,
}

#[cfg(any(feature = "wasm", test))]
#[derive(serde::Serialize, serde::Deserialize)]
struct SolveOutput {
    compatible: bool,
    #[serde(default)]
    explanation: String,
}

// ---------------------------------------------------------------------------
// Wasm runtime (behind feature gate)
// ---------------------------------------------------------------------------

#[cfg(feature = "wasm")]
mod runtime {

    use wasmi::{Engine, Linker, Memory, Module, Store};

    pub struct WasmPlugin {
        store: Store<HostState>,
        instance: wasmi::Instance,
        memory: Memory,
    }

    struct HostState;

    impl WasmPlugin {
        pub fn load(wasm_bytes: &[u8]) -> Result<Self, String> {
            let engine = Engine::default();
            let module =
                Module::new(&engine, wasm_bytes).map_err(|e| format!("Wasm compile error: {e}"))?;

            let mut store = Store::new(&engine, HostState);
            let linker = Linker::new(&engine);

            let instance = linker
                .instantiate(&mut store, &module)
                .map_err(|e| format!("Wasm instantiate error: {e}"))?
                .start(&mut store)
                .map_err(|e| format!("Wasm start error: {e}"))?;

            let memory = instance
                .get_memory(&store, "memory")
                .ok_or_else(|| "Plugin must export 'memory'".to_string())?;

            Ok(WasmPlugin {
                store,
                instance,
                memory,
            })
        }

        /// Write bytes into guest memory using the guest's allocator.
        fn write_to_guest(&mut self, data: &[u8]) -> Result<i32, String> {
            let alloc_fn = self
                .instance
                .get_typed_func::<i32, i32>(&self.store, "conflic_alloc")
                .map_err(|e| format!("Missing conflic_alloc export: {e}"))?;

            let len = data.len() as i32;
            let ptr = alloc_fn
                .call(&mut self.store, len)
                .map_err(|e| format!("conflic_alloc failed: {e}"))?;

            self.memory
                .write(&mut self.store, ptr as usize, data)
                .map_err(|e| format!("Memory write failed: {e}"))?;

            Ok(ptr)
        }

        /// Read bytes from guest memory.
        fn read_from_guest(&self, ptr: i32, len: i32) -> Result<Vec<u8>, String> {
            let mut buf = vec![0u8; len as usize];
            self.memory
                .read(&self.store, ptr as usize, &mut buf)
                .map_err(|e| format!("Memory read failed: {e}"))?;
            Ok(buf)
        }

        /// Unpack a (ptr, len) pair from an i64 return value.
        fn unpack_result(packed: i64) -> (i32, i32) {
            let ptr = (packed >> 32) as i32;
            let len = (packed & 0xFFFF_FFFF) as i32;
            (ptr, len)
        }

        /// Call the extract function with JSON input and return JSON output.
        pub fn call_extract(&mut self, input_json: &str) -> Result<String, String> {
            let input_bytes = input_json.as_bytes();
            let ptr = self.write_to_guest(input_bytes)?;

            let extract_fn = self
                .instance
                .get_typed_func::<(i32, i32), i64>(&self.store, "conflic_extract")
                .map_err(|e| format!("Missing conflic_extract export: {e}"))?;

            let result = extract_fn
                .call(&mut self.store, (ptr, input_bytes.len() as i32))
                .map_err(|e| format!("conflic_extract failed: {e}"))?;

            let (out_ptr, out_len) = Self::unpack_result(result);
            if out_len == 0 {
                return Ok("[]".into());
            }

            let out_bytes = self.read_from_guest(out_ptr, out_len)?;
            String::from_utf8(out_bytes).map_err(|e| format!("Invalid UTF-8 output: {e}"))
        }

        /// Call the solve function with JSON input and return JSON output.
        pub fn call_solve(&mut self, input_json: &str) -> Result<String, String> {
            let input_bytes = input_json.as_bytes();
            let ptr = self.write_to_guest(input_bytes)?;

            let solve_fn = self
                .instance
                .get_typed_func::<(i32, i32), i64>(&self.store, "conflic_solve")
                .map_err(|e| format!("Missing conflic_solve export: {e}"))?;

            let result = solve_fn
                .call(&mut self.store, (ptr, input_bytes.len() as i32))
                .map_err(|e| format!("conflic_solve failed: {e}"))?;

            let (out_ptr, out_len) = Self::unpack_result(result);
            if out_len == 0 {
                return Ok(r#"{"compatible":true}"#.into());
            }

            let out_bytes = self.read_from_guest(out_ptr, out_len)?;
            String::from_utf8(out_bytes).map_err(|e| format!("Invalid UTF-8 output: {e}"))
        }

        pub fn has_extract(&self) -> bool {
            self.instance
                .get_typed_func::<(i32, i32), i64>(&self.store, "conflic_extract")
                .is_ok()
        }

        pub fn has_solve(&self) -> bool {
            self.instance
                .get_typed_func::<(i32, i32), i64>(&self.store, "conflic_solve")
                .is_ok()
        }
    }
}

// ---------------------------------------------------------------------------
// Wasm extractor adapter
// ---------------------------------------------------------------------------

#[cfg(feature = "wasm")]
pub struct WasmExtractor {
    name: String,
    plugin: std::sync::Mutex<runtime::WasmPlugin>,
    concept_ids: Vec<String>,
    file_patterns: Vec<String>,
}

#[cfg(feature = "wasm")]
impl WasmExtractor {
    pub fn new(
        name: String,
        plugin: runtime::WasmPlugin,
        concept_ids: Vec<String>,
        file_patterns: Vec<String>,
    ) -> Self {
        Self {
            name,
            plugin: std::sync::Mutex::new(plugin),
            concept_ids,
            file_patterns,
        }
    }
}

#[cfg(feature = "wasm")]
impl Extractor for WasmExtractor {
    fn id(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Wasm plugin extractor"
    }

    fn concept_ids(&self) -> Vec<String> {
        self.concept_ids.clone()
    }

    fn relevant_filenames(&self) -> Vec<&str> {
        self.file_patterns.iter().map(|s| s.as_str()).collect()
    }

    fn matches_file(&self, filename: &str) -> bool {
        if self.file_patterns.is_empty() {
            return true; // match all files if no patterns specified
        }
        self.file_patterns.iter().any(|pattern| {
            if pattern.contains('*') {
                globset::Glob::new(pattern)
                    .ok()
                    .and_then(|g| g.compile_matcher().is_match(filename).then_some(()))
                    .is_some()
            } else {
                filename == pattern || filename.starts_with(pattern)
            }
        })
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        let input = ExtractInput {
            filename: &file.path.display().to_string(),
            content: &file.raw_text,
        };

        let input_json = match serde_json::to_string(&input) {
            Ok(j) => j,
            Err(_) => return vec![],
        };

        let mut plugin = match self.plugin.lock() {
            Ok(p) => p,
            Err(_) => return vec![],
        };

        let output_json = match plugin.call_extract(&input_json) {
            Ok(j) => j,
            Err(e) => {
                eprintln!("Wasm plugin '{}' extract error: {}", self.name, e);
                return vec![];
            }
        };

        let outputs: Vec<ExtractOutput> = match serde_json::from_str(&output_json) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("Wasm plugin '{}' output parse error: {}", self.name, e);
                return vec![];
            }
        };

        outputs
            .into_iter()
            .map(|o| {
                let authority = match o.authority.as_str() {
                    "enforced" => Authority::Enforced,
                    "advisory" => Authority::Advisory,
                    _ => Authority::Declared,
                };

                let display = if o.display_name.is_empty() {
                    o.concept_id.clone()
                } else {
                    o.display_name.clone()
                };

                let value = parse_version(&o.raw_value);

                ConfigAssertion::new(
                    SemanticConcept {
                        id: o.concept_id,
                        display_name: display,
                        category: concept::ConceptCategory::Custom("wasm-plugin".into()),
                    },
                    SemanticType::Version(value),
                    o.raw_value,
                    assertion::SourceLocation {
                        file: file.path.clone(),
                        line: o.line,
                        column: 0,
                        key_path: o.key_path,
                    },
                    authority,
                    &self.name,
                )
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Wasm solver adapter
// ---------------------------------------------------------------------------

#[cfg(feature = "wasm")]
pub struct WasmSolver {
    name: String,
    plugin: std::sync::Mutex<runtime::WasmPlugin>,
}

#[cfg(feature = "wasm")]
unsafe impl Send for WasmSolver {}
#[cfg(feature = "wasm")]
unsafe impl Sync for WasmSolver {}

#[cfg(feature = "wasm")]
impl WasmSolver {
    pub fn new(name: String, plugin: runtime::WasmPlugin) -> Self {
        Self {
            name,
            plugin: std::sync::Mutex::new(plugin),
        }
    }
}

#[cfg(feature = "wasm")]
impl crate::solve::solver_trait::Solver for WasmSolver {
    fn id(&self) -> &str {
        &self.name
    }

    fn rule_id(&self) -> &str {
        "WASM001"
    }

    fn compatible(&self, left: &str, right: &str) -> crate::solve::Compatibility {
        let concept_id = &self.name;
        let input = SolveInput {
            concept_id,
            left,
            right,
        };

        let input_json = match serde_json::to_string(&input) {
            Ok(j) => j,
            Err(_) => return crate::solve::Compatibility::Unknown,
        };

        let mut plugin = match self.plugin.lock() {
            Ok(p) => p,
            Err(_) => return crate::solve::Compatibility::Unknown,
        };

        let output_json = match plugin.call_solve(&input_json) {
            Ok(j) => j,
            Err(e) => {
                eprintln!("Wasm plugin '{}' solve error: {}", self.name, e);
                return crate::solve::Compatibility::Unknown;
            }
        };

        let output: SolveOutput = match serde_json::from_str(&output_json) {
            Ok(o) => o,
            Err(_) => return crate::solve::Compatibility::Unknown,
        };

        if output.compatible {
            crate::solve::Compatibility::Compatible
        } else {
            crate::solve::Compatibility::Incompatible(output.explanation)
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin loader
// ---------------------------------------------------------------------------

/// Load all configured Wasm plugins and register them as extractors/solvers.
#[cfg(feature = "wasm")]
pub fn load_plugins(plugin_configs: &[PluginConfig], config_dir: &Path) -> PluginLoadResult {
    let mut extractors: Vec<Box<dyn Extractor>> = Vec::new();
    let mut solvers: Vec<(String, Box<dyn crate::solve::solver_trait::Solver>)> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for config in plugin_configs {
        let wasm_path = config_dir.join(&config.path);
        let wasm_bytes = match std::fs::read(&wasm_path) {
            Ok(b) => b,
            Err(e) => {
                errors.push(format!(
                    "Failed to read plugin '{}' at {}: {}",
                    config.name,
                    wasm_path.display(),
                    e
                ));
                continue;
            }
        };

        let is_extractor = config.kind == "extractor" || config.kind == "both";
        let is_solver = config.kind == "solver" || config.kind == "both";

        if is_extractor {
            match runtime::WasmPlugin::load(&wasm_bytes) {
                Ok(plugin) => {
                    if plugin.has_extract() {
                        extractors.push(Box::new(WasmExtractor::new(
                            format!("wasm-{}", config.name),
                            plugin,
                            config.concepts.clone(),
                            config.file_patterns.clone(),
                        )));
                    } else {
                        errors.push(format!(
                            "Plugin '{}' does not export conflic_extract",
                            config.name
                        ));
                    }
                }
                Err(e) => {
                    errors.push(format!(
                        "Failed to load extractor plugin '{}': {}",
                        config.name, e
                    ));
                }
            }
        }

        if is_solver {
            match runtime::WasmPlugin::load(&wasm_bytes) {
                Ok(plugin) => {
                    if plugin.has_solve() {
                        for concept_id in &config.concepts {
                            // Each concept gets its own solver instance
                            match runtime::WasmPlugin::load(&wasm_bytes) {
                                Ok(solver_plugin) => {
                                    solvers.push((
                                        concept_id.clone(),
                                        Box::new(WasmSolver::new(
                                            concept_id.clone(),
                                            solver_plugin,
                                        )),
                                    ));
                                }
                                Err(e) => {
                                    errors.push(format!(
                                        "Failed to load solver plugin '{}' for concept '{}': {}",
                                        config.name, concept_id, e
                                    ));
                                }
                            }
                        }
                    } else {
                        errors.push(format!(
                            "Plugin '{}' does not export conflic_solve",
                            config.name
                        ));
                    }
                }
                Err(e) => {
                    errors.push(format!(
                        "Failed to load solver plugin '{}': {}",
                        config.name, e
                    ));
                }
            }
        }
    }

    PluginLoadResult {
        extractors,
        solvers,
        errors,
    }
}

/// Result of loading Wasm plugins.
pub struct PluginLoadResult {
    pub extractors: Vec<Box<dyn crate::extract::Extractor>>,
    pub solvers: Vec<(String, Box<dyn crate::solve::solver_trait::Solver>)>,
    pub errors: Vec<String>,
}

/// Stub loader when wasm feature is disabled.
#[cfg(not(feature = "wasm"))]
pub fn load_plugins(plugin_configs: &[PluginConfig], _config_dir: &Path) -> PluginLoadResult {
    let errors = if plugin_configs.is_empty() {
        vec![]
    } else {
        vec!["Wasm plugins configured but the 'wasm' feature is not enabled. Rebuild with `--features wasm`.".into()]
    };

    PluginLoadResult {
        extractors: vec![],
        solvers: vec![],
        errors,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_config_deserialize() {
        let toml_str = r#"
name = "custom-terraform"
path = "plugins/terraform.wasm"
kind = "extractor"
file_patterns = ["*.tf"]
concepts = ["terraform-version"]
"#;
        let config: PluginConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.name, "custom-terraform");
        assert_eq!(config.path, "plugins/terraform.wasm");
        assert_eq!(config.kind, "extractor");
        assert_eq!(config.file_patterns, vec!["*.tf"]);
        assert_eq!(config.concepts, vec!["terraform-version"]);
    }

    #[test]
    fn test_plugin_config_defaults() {
        let toml_str = r#"
name = "minimal"
path = "plugin.wasm"
"#;
        let config: PluginConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.kind, "extractor");
        assert!(config.concepts.is_empty());
        assert!(config.file_patterns.is_empty());
    }

    #[test]
    fn test_stub_loader_warns_when_plugins_configured() {
        #[cfg(not(feature = "wasm"))]
        {
            let configs = vec![PluginConfig {
                name: "test".into(),
                path: "test.wasm".into(),
                kind: "extractor".into(),
                concepts: vec![],
                file_patterns: vec![],
            }];

            let result = load_plugins(&configs, Path::new("."));
            assert!(!result.errors.is_empty());
            assert!(result.errors[0].contains("not enabled"));
        }
    }

    #[test]
    fn test_stub_loader_no_error_when_empty() {
        #[cfg(not(feature = "wasm"))]
        {
            let result = load_plugins(&[], Path::new("."));
            assert!(result.errors.is_empty());
        }
    }

    #[test]
    fn test_extract_output_deserialize() {
        let json = r#"[{
            "concept_id": "custom-version",
            "raw_value": "1.2.3",
            "line": 5,
            "key_path": "version",
            "authority": "enforced"
        }]"#;

        let outputs: Vec<ExtractOutput> = serde_json::from_str(json).unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].concept_id, "custom-version");
        assert_eq!(outputs[0].raw_value, "1.2.3");
        assert_eq!(outputs[0].line, 5);
        assert_eq!(outputs[0].authority, "enforced");
    }

    #[test]
    fn test_solve_output_deserialize() {
        let json = r#"{"compatible": false, "explanation": "versions differ"}"#;
        let output: SolveOutput = serde_json::from_str(json).unwrap();
        assert!(!output.compatible);
        assert_eq!(output.explanation, "versions differ");
    }

    #[test]
    fn test_extract_output_defaults() {
        let json = r#"[{"concept_id": "x", "raw_value": "v1"}]"#;
        let outputs: Vec<ExtractOutput> = serde_json::from_str(json).unwrap();
        assert_eq!(outputs[0].line, 1);
        assert_eq!(outputs[0].authority, "declared");
        assert!(outputs[0].display_name.is_empty());
    }
}
