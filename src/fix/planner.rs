use crate::model::*;
use crate::parse::FileFormat;

use super::FixOperation;
use super::render::{
    ExactFileFlavor, render_declared_version_string, render_docker_expose_token,
    render_docker_from_arguments, render_dotnet_target_framework, render_exact_file_version,
    render_exactish_version_token, render_port_value,
};

pub(super) fn build_fix_operation(
    winner: &ConfigAssertion,
    target: &ConfigAssertion,
    format: FileFormat,
) -> Result<(String, FixOperation), String> {
    if target.is_matrix {
        return Err("matrix values are not auto-fixed".into());
    }

    match target.extractor_id.as_str() {
        "node-version-nvmrc" => {
            let value =
                render_exact_file_version(winner, &target.raw_value, ExactFileFlavor::Node)?;
            Ok((value.clone(), FixOperation::ReplaceWholeFileValue { value }))
        }
        "python-version-file" => {
            let value =
                render_exact_file_version(winner, &target.raw_value, ExactFileFlavor::Plain)?;
            Ok((value.clone(), FixOperation::ReplaceWholeFileValue { value }))
        }
        "ruby-version-file" => {
            let value =
                render_exact_file_version(winner, &target.raw_value, ExactFileFlavor::Ruby)?;
            Ok((value.clone(), FixOperation::ReplaceWholeFileValue { value }))
        }
        "node-version-package-json" => build_json_string_fix(winner, &["engines", "node"]),
        "dotnet-version-global-json" => build_exact_json_string_fix(winner, &["sdk", "version"]),
        "go-version-gomod" => {
            let value = render_exactish_version_token(winner).ok_or_else(|| {
                "winner cannot be represented safely in go.mod; manual update required".to_string()
            })?;
            Ok((value.clone(), FixOperation::ReplaceGoModVersion { value }))
        }
        "node-version-tool-versions"
        | "ruby-version-tool-versions"
        | "java-version-tool-versions" => {
            let value = render_exactish_version_token(winner).ok_or_else(|| {
                "winner cannot be represented safely in .tool-versions".to_string()
            })?;
            Ok((
                value.clone(),
                FixOperation::ReplaceToolVersionsValue { value },
            ))
        }
        "ruby-version-gemfile" => {
            let value = render_exactish_version_token(winner).ok_or_else(|| {
                "winner cannot be represented safely in Gemfile; manual update required".to_string()
            })?;
            Ok((
                value.clone(),
                FixOperation::ReplaceGemfileRubyVersion { value },
            ))
        }
        "java-version-pom" => {
            let value = render_exactish_version_token(winner).ok_or_else(|| {
                "winner cannot be represented safely in pom.xml; manual update required".to_string()
            })?;
            let span = target
                .span
                .ok_or_else(|| "missing exact XML source span for pom.xml value".to_string())?;
            Ok((
                value.clone(),
                FixOperation::ReplaceTextRange {
                    start: span.start,
                    end: span.end,
                    value,
                },
            ))
        }
        "dotnet-version-csproj" => {
            let value =
                render_dotnet_target_framework(winner, &target.raw_value).ok_or_else(|| {
                    "winner cannot be rendered safely as a TargetFramework value".to_string()
                })?;
            let span = target
                .span
                .ok_or_else(|| "missing exact XML source span for .csproj value".to_string())?;
            Ok((
                value.clone(),
                FixOperation::ReplaceTextRange {
                    start: span.start,
                    end: span.end,
                    value,
                },
            ))
        }
        "node-version-dockerfile"
        | "python-version-dockerfile"
        | "ruby-version-dockerfile"
        | "java-version-dockerfile"
        | "go-version-dockerfile"
        | "dotnet-version-dockerfile" => {
            let arguments =
                render_docker_from_arguments(winner, &target.raw_value).ok_or_else(|| {
                    "winner cannot be rendered safely as a Docker image tag".to_string()
                })?;
            Ok((
                arguments.clone(),
                FixOperation::ReplaceDockerFromArguments { arguments },
            ))
        }
        "port-env" => {
            if target.raw_value.contains("${") {
                return Err(
                    "environment-default expressions are not auto-rewritten; manual update required"
                        .into(),
                );
            }
            let value = render_port_value(&winner.value).ok_or_else(|| {
                "winner cannot be represented safely as an env port value".to_string()
            })?;
            Ok((
                value.clone(),
                FixOperation::ReplaceEnvValue {
                    key: target.source.key_path.clone(),
                    value,
                },
            ))
        }
        "port-dockerfile" => {
            let value =
                render_docker_expose_token(&winner.value, &target.raw_value).ok_or_else(|| {
                    "winner cannot be represented safely in Dockerfile EXPOSE".to_string()
                })?;
            Ok((
                value.clone(),
                FixOperation::ReplaceDockerExposeToken {
                    current: target.raw_value.clone(),
                    value,
                },
            ))
        }
        _ => Err(format!(
            "auto-fix is not supported for {} targets",
            extractor_target_name(target, &format)
        )),
    }
}

pub(super) fn all_values_mutually_equivalent(assertions: &[&ConfigAssertion]) -> bool {
    for (index, left) in assertions.iter().enumerate() {
        for right in assertions.iter().skip(index + 1) {
            if !winner_values_equivalent(&left.value, &right.value) {
                return false;
            }
        }
    }
    true
}

pub(super) fn values_equivalent(a: &SemanticType, b: &SemanticType) -> bool {
    use crate::solve::Compatibility;

    match (a, b) {
        (SemanticType::Version(va), SemanticType::Version(vb)) => {
            matches!(
                crate::solve::version::versions_compatible(va, vb),
                Compatibility::Compatible
            )
        }
        (SemanticType::Port(pa), SemanticType::Port(pb)) => {
            matches!(
                crate::solve::port::ports_compatible(pa, pb),
                Compatibility::Compatible
            )
        }
        (SemanticType::Boolean(a), SemanticType::Boolean(b)) => a == b,
        (SemanticType::StringValue(a), SemanticType::StringValue(b)) => a == b,
        (SemanticType::Number(a), SemanticType::Number(b)) => (a - b).abs() < f64::EPSILON,
        _ => false,
    }
}

fn winner_values_equivalent(a: &SemanticType, b: &SemanticType) -> bool {
    match (a, b) {
        (SemanticType::Version(left), SemanticType::Version(right)) => {
            winner_versions_equivalent(left, right)
        }
        (SemanticType::Port(left), SemanticType::Port(right)) => {
            normalized_port_interval(left) == normalized_port_interval(right)
        }
        (SemanticType::Boolean(left), SemanticType::Boolean(right)) => left == right,
        (SemanticType::StringValue(left), SemanticType::StringValue(right)) => left == right,
        (SemanticType::Number(left), SemanticType::Number(right)) => {
            (left - right).abs() < f64::EPSILON
        }
        _ => false,
    }
}

fn winner_versions_equivalent(a: &VersionSpec, b: &VersionSpec) -> bool {
    match (a, b) {
        (VersionSpec::Exact(left), VersionSpec::Exact(right)) => left == right,
        (
            VersionSpec::Partial {
                major: left_major,
                minor: left_minor,
            },
            VersionSpec::Partial {
                major: right_major,
                minor: right_minor,
            },
        ) => left_major == right_major && left_minor == right_minor,
        (VersionSpec::Range(left), VersionSpec::Range(right)) => {
            left.to_string() == right.to_string()
        }
        (
            VersionSpec::DockerTag {
                version: left_version,
                variant: left_variant,
            },
            VersionSpec::DockerTag {
                version: right_version,
                variant: right_variant,
            },
        ) => left_version == right_version && left_variant == right_variant,
        (VersionSpec::Unparsed(left), VersionSpec::Unparsed(right)) => left.trim() == right.trim(),
        _ => false,
    }
}

fn normalized_port_interval(port: &PortSpec) -> (u16, u16) {
    match port {
        PortSpec::Single(value) => (*value, *value),
        PortSpec::Range(start, end) => ((*start).min(*end), (*start).max(*end)),
        PortSpec::Mapping { container, .. } => (*container, *container),
    }
}

fn build_json_string_fix(
    winner: &ConfigAssertion,
    path: &[&str],
) -> Result<(String, FixOperation), String> {
    let value = render_declared_version_string(winner).ok_or_else(|| {
        "winner cannot be represented safely in this JSON field; manual update required".to_string()
    })?;
    Ok((
        value.clone(),
        FixOperation::ReplaceJsonString {
            path: path.iter().map(|segment| (*segment).to_string()).collect(),
            value,
        },
    ))
}

fn build_exact_json_string_fix(
    winner: &ConfigAssertion,
    path: &[&str],
) -> Result<(String, FixOperation), String> {
    let value = render_exactish_version_token(winner).ok_or_else(|| {
        "winner cannot be represented safely in this JSON field; manual update required".to_string()
    })?;
    Ok((
        value.clone(),
        FixOperation::ReplaceJsonString {
            path: path.iter().map(|segment| (*segment).to_string()).collect(),
            value,
        },
    ))
}

fn extractor_target_name(target: &ConfigAssertion, format: &FileFormat) -> String {
    if !target.source.key_path.is_empty() {
        format!("{} {}", format_name(format), target.source.key_path)
    } else {
        format_name(format).to_string()
    }
}

fn format_name(format: &FileFormat) -> &'static str {
    match format {
        FileFormat::Json => "JSON",
        FileFormat::Yaml => "YAML",
        FileFormat::Toml => "TOML",
        FileFormat::Dockerfile => "Dockerfile",
        FileFormat::Env => "env",
        FileFormat::PlainText => "plain-text",
    }
}
