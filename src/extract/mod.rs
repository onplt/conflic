pub mod custom;
pub mod dotnet_version;
pub mod go_version;
pub mod java_version;
pub mod node_version;
pub mod port;
pub mod python_version;
pub mod ruby_version;
pub mod ts_strict;

use crate::config::ConflicConfig;
use crate::model::ConfigAssertion;
use crate::parse::ParsedFile;

/// Trait for extracting semantic assertions from parsed config files.
pub trait Extractor: Send + Sync {
    /// Unique identifier.
    fn id(&self) -> &str;

    /// Human-readable description.
    fn description(&self) -> &str;

    /// Filenames this extractor cares about (exact matches or prefix matches).
    fn relevant_filenames(&self) -> Vec<&str>;

    /// Whether this extractor should process a file given its filename.
    fn matches_file(&self, filename: &str) -> bool {
        for pattern in self.relevant_filenames() {
            if filename == pattern || filename.starts_with(pattern) {
                return true;
            }
        }
        false
    }

    /// Extract assertions from a parsed file.
    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion>;
}

/// Create the default set of extractors.
pub fn default_extractors() -> Vec<Box<dyn Extractor>> {
    vec![
        // Node.js version
        Box::new(node_version::NvmrcExtractor),
        Box::new(node_version::PackageJsonNodeExtractor),
        Box::new(node_version::DockerfileNodeExtractor),
        Box::new(node_version::CiNodeExtractor),
        Box::new(node_version::ToolVersionsNodeExtractor),
        // Python version
        Box::new(python_version::PythonVersionFileExtractor),
        Box::new(python_version::PyprojectExtractor),
        Box::new(python_version::DockerfilePythonExtractor),
        Box::new(python_version::CiPythonExtractor),
        // Go version
        Box::new(go_version::GoModExtractor),
        Box::new(go_version::DockerfileGoExtractor),
        // Ports
        Box::new(port::EnvPortExtractor),
        Box::new(port::DockerComposePortExtractor),
        Box::new(port::DockerfilePortExtractor),
        // .NET version
        Box::new(dotnet_version::CsprojExtractor),
        Box::new(dotnet_version::GlobalJsonExtractor),
        Box::new(dotnet_version::DockerfileDotnetExtractor),
        // Java version
        Box::new(java_version::PomXmlExtractor),
        Box::new(java_version::DockerfileJavaExtractor),
        Box::new(java_version::SdkmanrcExtractor),
        Box::new(java_version::ToolVersionsJavaExtractor),
        Box::new(java_version::CiJavaExtractor),
        // Ruby version
        Box::new(ruby_version::RubyVersionFileExtractor),
        Box::new(ruby_version::GemfileExtractor),
        Box::new(ruby_version::DockerfileRubyExtractor),
        Box::new(ruby_version::CiRubyExtractor),
        Box::new(ruby_version::ToolVersionsRubyExtractor),
        // TypeScript strict mode
        Box::new(ts_strict::TsconfigStrictExtractor),
        Box::new(ts_strict::EslintStrictExtractor),
    ]
}

/// Build extractors from defaults plus any custom extractors defined in config.
pub fn build_extractors(config: &ConflicConfig) -> Vec<Box<dyn Extractor>> {
    let mut extractors = default_extractors();
    for custom_config in &config.custom_extractor {
        extractors.push(Box::new(custom::CustomExtractor::new(custom_config.clone())));
    }
    extractors
}
