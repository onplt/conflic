use regex::Regex;

pub(super) fn navigate_json(value: &serde_json::Value, path: &str) -> Option<String> {
    let mut current = value;
    for key in path.split('.') {
        current = current.get(key)?;
    }
    match current {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        _ => Some(current.to_string()),
    }
}

pub(super) fn navigate_yaml(value: &crate::parse::YamlValue, path: &str) -> Option<String> {
    navigate_json(value, path)
}

pub(super) fn navigate_toml(value: &toml::Value, path: &str) -> Option<String> {
    let mut current = value;
    for key in path.split('.') {
        current = current.get(key)?;
    }
    match current {
        toml::Value::String(value) => Some(value.clone()),
        toml::Value::Integer(value) => Some(value.to_string()),
        toml::Value::Float(value) => Some(value.to_string()),
        toml::Value::Boolean(value) => Some(value.to_string()),
        _ => Some(current.to_string()),
    }
}

pub(super) fn apply_pattern(raw: &str, pattern: Option<&Regex>) -> Option<String> {
    match pattern {
        None => Some(raw.to_string()),
        Some(regex) => {
            let captures = regex.captures(raw)?;
            if let Some(value) = captures.get(1) {
                Some(value.as_str().to_string())
            } else {
                Some(captures.get(0)?.as_str().to_string())
            }
        }
    }
}
