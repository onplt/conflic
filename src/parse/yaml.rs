use super::FileContent;

pub fn parse_yaml(raw: &str) -> Result<FileContent, String> {
    let value: serde_yml::Value =
        serde_yml::from_str(raw).map_err(|e| format!("Failed to parse YAML: {}", e))?;
    Ok(FileContent::Yaml(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_yaml() {
        let input = "name: test\nversion: 1.0.0\n";
        let result = parse_yaml(input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_yaml_with_anchors() {
        let input = "defaults: &defaults\n  timeout: 30\nproduction:\n  <<: *defaults\n  port: 8080\n";
        let result = parse_yaml(input);
        assert!(result.is_ok());
    }
}
