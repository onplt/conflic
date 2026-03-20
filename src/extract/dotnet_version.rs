use crate::model::*;
use crate::parse::*;
use crate::parse::source_location::*;
use super::Extractor;

// --- .csproj TargetFramework extractor (regex-based) ---

pub struct CsprojExtractor;

impl Extractor for CsprojExtractor {
    fn id(&self) -> &str { "dotnet-version-csproj" }
    fn description(&self) -> &str { ".NET version from .csproj TargetFramework" }
    fn relevant_filenames(&self) -> Vec<&str> { vec![] }

    fn matches_file(&self, filename: &str) -> bool {
        filename.ends_with(".csproj")
    }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        let raw = &file.raw_text;
        let mut results = Vec::new();

        // Match <TargetFramework>net8.0</TargetFramework>
        let tf_re = regex::Regex::new(
            r"<TargetFramework>\s*(net\d+\.\d+[^<]*)\s*</TargetFramework>"
        ).unwrap();
        if let Some(caps) = tf_re.captures(raw) {
            let framework = caps[1].trim().to_string();
            let version = parse_dotnet_framework(&framework);
            let line = find_line_for_key(raw, "TargetFramework");
            results.push(ConfigAssertion::new(
                SemanticConcept::dotnet_version(),
                SemanticType::Version(version),
                framework,
                SourceLocation {
                    file: file.path.clone(),
                    line,
                    column: 0,
                    key_path: "TargetFramework".into(),
                },
                Authority::Declared,
                self.id(),
            ));
        }

        // Match <TargetFrameworks>net8.0;net7.0</TargetFrameworks> (multi-target)
        let tfs_re = regex::Regex::new(
            r"<TargetFrameworks>\s*([^<]+)\s*</TargetFrameworks>"
        ).unwrap();
        if let Some(caps) = tfs_re.captures(raw) {
            let frameworks_str = caps[1].trim();
            let line = find_line_for_key(raw, "TargetFrameworks");
            for framework in frameworks_str.split(';') {
                let framework = framework.trim();
                if framework.is_empty() {
                    continue;
                }
                let version = parse_dotnet_framework(framework);
                results.push(
                    ConfigAssertion::new(
                        SemanticConcept::dotnet_version(),
                        SemanticType::Version(version),
                        framework.to_string(),
                        SourceLocation {
                            file: file.path.clone(),
                            line,
                            column: 0,
                            key_path: "TargetFrameworks".into(),
                        },
                        Authority::Declared,
                        self.id(),
                    )
                    .with_matrix(true),
                );
            }
        }

        results
    }
}

// --- global.json sdk.version extractor ---

pub struct GlobalJsonExtractor;

impl Extractor for GlobalJsonExtractor {
    fn id(&self) -> &str { "dotnet-version-global-json" }
    fn description(&self) -> &str { ".NET SDK version from global.json" }
    fn relevant_filenames(&self) -> Vec<&str> { vec!["global.json"] }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::Json(ref value) = file.content {
            if let Some(sdk_version) = value
                .get("sdk")
                .and_then(|s| s.get("version"))
                .and_then(|v| v.as_str())
            {
                let version = parse_version(sdk_version);
                let line = find_line_for_json_key(&file.raw_text, "sdk.version");
                return vec![ConfigAssertion::new(
                    SemanticConcept::dotnet_version(),
                    SemanticType::Version(version),
                    sdk_version.to_string(),
                    SourceLocation {
                        file: file.path.clone(),
                        line,
                        column: 0,
                        key_path: "sdk.version".into(),
                    },
                    Authority::Enforced,
                    self.id(),
                )];
            }
        }
        vec![]
    }
}

// --- Dockerfile FROM dotnet images ---

pub struct DockerfileDotnetExtractor;

impl Extractor for DockerfileDotnetExtractor {
    fn id(&self) -> &str { "dotnet-version-dockerfile" }
    fn description(&self) -> &str { ".NET version from Dockerfile FROM dotnet images" }
    fn relevant_filenames(&self) -> Vec<&str> { vec!["Dockerfile"] }

    fn extract(&self, file: &ParsedFile) -> Vec<ConfigAssertion> {
        if let FileContent::Dockerfile(ref instructions) = file.content {
            let mut results = Vec::new();
            // Match mcr.microsoft.com/dotnet/sdk:8.0 or mcr.microsoft.com/dotnet/aspnet:8.0
            let re = regex::Regex::new(
                r"mcr\.microsoft\.com/dotnet/(?:sdk|aspnet|runtime):(\S+)"
            ).unwrap();

            for instr in instructions {
                if instr.instruction == "FROM" {
                    if let Some(caps) = re.captures(&instr.arguments) {
                        let tag = caps[1].to_string();
                        // Strip " AS name" from tag if present
                        let tag = tag.split_whitespace().next().unwrap_or(&tag).to_string();
                        let version = parse_version(&tag);
                        let authority = if instr.is_final_stage {
                            Authority::Enforced
                        } else {
                            Authority::Advisory
                        };
                        results.push(ConfigAssertion::new(
                            SemanticConcept::dotnet_version(),
                            SemanticType::Version(version),
                            instr.arguments.clone(),
                            SourceLocation {
                                file: file.path.clone(),
                                line: instr.line,
                                column: 0,
                                key_path: "FROM".into(),
                            },
                            authority,
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

/// Parse .NET framework moniker like "net8.0" into a version.
fn parse_dotnet_framework(framework: &str) -> VersionSpec {
    // net8.0 -> 8.0, net6.0-windows -> 6.0
    let version_part = framework
        .strip_prefix("net")
        .unwrap_or(framework)
        .split('-')
        .next()
        .unwrap_or(framework);
    parse_version(version_part)
}
