use crate::model::*;

pub(super) enum ExactFileFlavor {
    Plain,
    Node,
    Ruby,
}

pub(super) fn render_exact_file_version(
    winner: &ConfigAssertion,
    current_raw: &str,
    flavor: ExactFileFlavor,
) -> Result<String, String> {
    let base = render_exactish_version_token(winner).ok_or_else(|| {
        "winner is not an exact version token, so this file cannot be auto-fixed safely".to_string()
    })?;

    let value = match flavor {
        ExactFileFlavor::Plain => base,
        ExactFileFlavor::Node => {
            if current_raw.trim_start().starts_with('v') {
                format!("v{}", base)
            } else {
                base
            }
        }
        ExactFileFlavor::Ruby => {
            if current_raw.trim_start().starts_with("ruby-") {
                format!("ruby-{}", base)
            } else {
                base
            }
        }
    };

    Ok(value)
}

pub(super) fn render_exactish_version_token(winner: &ConfigAssertion) -> Option<String> {
    match &winner.value {
        SemanticType::Version(VersionSpec::Exact(version)) => Some(version.to_string()),
        SemanticType::Version(VersionSpec::Partial {
            major,
            minor: Some(minor),
        }) => Some(format!("{}.{}", major, minor)),
        SemanticType::Version(VersionSpec::Partial { major, minor: None }) => {
            Some(major.to_string())
        }
        SemanticType::Version(VersionSpec::DockerTag { version, .. }) => Some(version.clone()),
        SemanticType::Version(VersionSpec::Range(_)) => {
            let raw = winner.raw_value.trim();
            is_numeric_version_token(raw).then(|| raw.to_string())
        }
        SemanticType::Version(VersionSpec::Unparsed(raw)) => {
            let raw = raw.trim();
            is_numeric_version_token(raw).then(|| raw.to_string())
        }
        _ => None,
    }
}

pub(super) fn render_declared_version_string(winner: &ConfigAssertion) -> Option<String> {
    match &winner.value {
        SemanticType::Version(VersionSpec::Range(_)) => Some(winner.raw_value.trim().to_string()),
        SemanticType::Version(VersionSpec::DockerTag { version, .. }) => Some(version.clone()),
        SemanticType::Version(_) => render_exactish_version_token(winner),
        _ => None,
    }
}

pub(super) fn render_dotnet_target_framework(
    winner: &ConfigAssertion,
    current_raw: &str,
) -> Option<String> {
    let version = render_major_minor_version(winner)?;
    let suffix = current_raw
        .strip_prefix("net")
        .unwrap_or(current_raw)
        .split_once('-')
        .map(|(_, suffix)| format!("-{}", suffix))
        .unwrap_or_default();
    Some(format!("net{}{}", version, suffix))
}

fn render_major_minor_version(winner: &ConfigAssertion) -> Option<String> {
    match &winner.value {
        SemanticType::Version(VersionSpec::Exact(version)) => {
            Some(format!("{}.{}", version.major, version.minor))
        }
        SemanticType::Version(VersionSpec::Partial {
            major,
            minor: Some(minor),
        }) => Some(format!("{}.{}", major, minor)),
        SemanticType::Version(VersionSpec::Partial { major, minor: None }) => {
            Some(format!("{}.0", major))
        }
        SemanticType::Version(VersionSpec::DockerTag { version, .. })
        | SemanticType::Version(VersionSpec::Unparsed(version)) => {
            numeric_major_minor(version.trim())
        }
        SemanticType::Version(VersionSpec::Range(_)) => {
            numeric_major_minor(winner.raw_value.trim())
        }
        _ => None,
    }
}

fn numeric_major_minor(raw: &str) -> Option<String> {
    let mut parts = raw.split('.');
    let major = parts.next()?;
    if !major.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let minor = parts.next().unwrap_or("0");
    if !minor.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some(format!("{}.{}", major, minor))
}

pub(super) fn render_docker_from_arguments(
    winner: &ConfigAssertion,
    current_arguments: &str,
) -> Option<String> {
    let reference = crate::parse::dockerfile::docker_from_image_reference(current_arguments)?;
    let new_tag = render_docker_tag(winner, reference.tag)?;
    Some(format!(
        "{}{}:{}{}",
        reference.prefix, reference.image, new_tag, reference.suffix
    ))
}

fn render_docker_tag(winner: &ConfigAssertion, current_tag: &str) -> Option<String> {
    match &winner.value {
        SemanticType::Version(VersionSpec::DockerTag { version, variant }) => Some(match variant {
            Some(variant) => format!("{}-{}", version, variant),
            None => version.clone(),
        }),
        SemanticType::Version(_) => {
            let version = render_exactish_version_token(winner)?;
            let suffix = current_tag
                .split_once('-')
                .map(|(_, suffix)| format!("-{}", suffix))
                .unwrap_or_default();
            Some(format!("{}{}", version, suffix))
        }
        _ => None,
    }
}

pub(super) fn render_docker_expose_token(
    value: &SemanticType,
    current_token: &str,
) -> Option<String> {
    let base = render_port_value(value)?;
    let suffix = current_token
        .split_once('/')
        .map(|(_, suffix)| format!("/{}", suffix))
        .unwrap_or_default();
    Some(format!("{}{}", base, suffix))
}

pub(super) fn render_port_value(value: &SemanticType) -> Option<String> {
    match value {
        SemanticType::Port(PortSpec::Single(port)) => Some(port.to_string()),
        SemanticType::Port(PortSpec::Range(start, end)) => Some(format!("{}-{}", start, end)),
        SemanticType::Port(PortSpec::Mapping { container, .. }) => Some(container.to_string()),
        _ => None,
    }
}

fn is_numeric_version_token(raw: &str) -> bool {
    !raw.is_empty()
        && raw.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
        && raw.chars().any(|ch| ch.is_ascii_digit())
}
