use super::FileContent;

pub fn parse_toml(raw: &str) -> Result<FileContent, String> {
    let value: toml::Value =
        toml::from_str(raw).map_err(|e| format!("Failed to parse TOML: {}", e))?;
    Ok(FileContent::Toml(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_toml() {
        let input = "[project]\nname = \"test\"\nrequires-python = \">=3.10\"\n";
        let result = parse_toml(input);
        assert!(result.is_ok());
    }
}
