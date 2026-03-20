pub mod custom;
pub mod dotnet_version;
pub mod go_version;
pub mod java_version;
pub mod node_version;
pub mod port;
pub mod python_version;
pub mod ruby_version;
mod runtime_version;
pub mod ts_strict;

use crate::config::ConflicConfig;
use crate::model::{ConfigAssertion, ParseDiagnostic};
use crate::parse::ParsedFile;
use std::path::Path;

/// Trait for extracting semantic assertions from parsed config files.
pub trait Extractor: Send + Sync {
    /// Unique identifier.
    fn id(&self) -> &str;

    /// Human-readable description.
    fn description(&self) -> &str;

    /// Semantic concept IDs this extractor can emit.
    fn concept_ids(&self) -> Vec<String> {
        inferred_concept_ids(self.id())
    }

    /// Filenames this extractor cares about (exact matches or prefix matches).
    fn relevant_filenames(&self) -> Vec<&str>;

    /// Whether this extractor should process a file given its filename.
    fn matches_file(&self, filename: &str) -> bool {
        self.relevant_filenames()
            .into_iter()
            .any(|pattern| filename == pattern || filename.starts_with(pattern))
    }

    /// Whether this extractor should process a file given both filename and path.
    fn matches_path(&self, filename: &str, _path: &Path) -> bool {
        self.matches_file(filename)
    }

    /// Extract assertions from a parsed file.
    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion>;
}

fn inferred_concept_ids(extractor_id: &str) -> Vec<String> {
    const EXTRACTOR_PREFIXES: [(&str, &str); 8] = [
        ("node-version-", "node-version"),
        ("python-version-", "python-version"),
        ("go-version-", "go-version"),
        ("java-version-", "java-version"),
        ("ruby-version-", "ruby-version"),
        ("dotnet-version-", "dotnet-version"),
        ("port-", "app-port"),
        ("ts-strict-", "ts-strict-mode"),
    ];

    let concept_id = EXTRACTOR_PREFIXES
        .iter()
        .find_map(|(prefix, concept_id)| extractor_id.starts_with(prefix).then_some(*concept_id))
        .unwrap_or(extractor_id);

    vec![concept_id.to_string()]
}

pub struct ExtractorBuild {
    pub extractors: Vec<Box<dyn Extractor>>,
    pub diagnostics: Vec<ParseDiagnostic>,
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
pub fn build_extractors(config: &ConflicConfig) -> ExtractorBuild {
    let mut extractors = default_extractors();
    let (compiled_custom_extractors, diagnostics) = config.compiled_custom_extractors();

    for custom_extractor in compiled_custom_extractors {
        extractors.push(Box::new(custom_extractor));
    }

    ExtractorBuild {
        extractors,
        diagnostics,
    }
}
