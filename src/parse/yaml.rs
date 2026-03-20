use super::{FileContent, YamlValue};

pub fn parse_yaml(raw: &str) -> Result<FileContent, String> {
    let value: YamlValue =
        serde_saphyr::from_str(raw).map_err(|e| format!("Failed to parse YAML: {}", e))?;
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
        let input =
            "defaults: &defaults\n  timeout: 30\nproduction:\n  <<: *defaults\n  port: 8080\n";
        let result = parse_yaml(input).unwrap();

        let FileContent::Yaml(value) = result else {
            panic!("expected YAML content");
        };

        assert_eq!(value["production"]["timeout"], 30);
        assert_eq!(value["production"]["port"], 8080);
    }

    #[test]
    fn test_parse_yaml_preserves_multiline_scalars_and_comment_like_content() {
        let input = "script: |\n  echo \"# not a comment\"\n  echo done\n# real comment\n";
        let result = parse_yaml(input).unwrap();

        let FileContent::Yaml(value) = result else {
            panic!("expected YAML content");
        };

        assert_eq!(
            value["script"].as_str(),
            Some("echo \"# not a comment\"\necho done\n")
        );
    }
}
