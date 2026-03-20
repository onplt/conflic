use super::DockerInstruction;

/// Parse a Dockerfile into a list of instructions with stage tracking.
pub fn parse_dockerfile(raw: &str) -> Vec<DockerInstruction> {
    let mut instructions = Vec::new();
    let mut stage_index: usize = 0;
    let mut stage_name: Option<String> = None;
    let mut from_lines: Vec<usize> = Vec::new();

    // First pass: find all FROM lines to determine which is final
    for (i, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.to_uppercase().starts_with("FROM ") {
            from_lines.push(i);
        }
    }

    let total_stages = from_lines.len();
    let mut current_from_index = 0;

    for (line_num, line) in raw.lines().enumerate() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Parse instruction
        let (instruction, arguments) = if let Some(space_pos) = trimmed.find(char::is_whitespace) {
            (
                trimmed[..space_pos].to_uppercase(),
                trimmed[space_pos..].trim().to_string(),
            )
        } else {
            (trimmed.to_uppercase(), String::new())
        };

        // Track stages via FROM
        if instruction == "FROM" {
            if current_from_index > 0 {
                stage_index += 1;
            }
            stage_name = parse_stage_name(&arguments);
            current_from_index += 1;
        }

        let is_final_stage = current_from_index == total_stages;

        instructions.push(DockerInstruction {
            instruction,
            arguments,
            line: line_num + 1, // 1-based
            stage_index,
            stage_name: stage_name.clone(),
            is_final_stage,
        });
    }

    instructions
}

fn parse_stage_name(from_args: &str) -> Option<String> {
    // FROM image:tag AS name
    let upper = from_args.to_uppercase();
    if let Some(as_pos) = upper.find(" AS ") {
        let name = from_args[as_pos + 4..].trim().to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_dockerfile() {
        let input = "FROM node:20-alpine\nWORKDIR /app\nCOPY . .\nEXPOSE 3000\n";
        let instructions = parse_dockerfile(input);
        assert_eq!(instructions.len(), 4);
        assert_eq!(instructions[0].instruction, "FROM");
        assert_eq!(instructions[0].arguments, "node:20-alpine");
        assert!(instructions[0].is_final_stage);
        assert_eq!(instructions[3].instruction, "EXPOSE");
        assert_eq!(instructions[3].arguments, "3000");
    }

    #[test]
    fn test_parse_multistage_dockerfile() {
        let input = "FROM node:22 AS builder\nRUN npm build\nFROM node:20-alpine\nCOPY --from=builder /app .\n";
        let instructions = parse_dockerfile(input);

        // First FROM - not final stage
        assert_eq!(instructions[0].stage_index, 0);
        assert_eq!(instructions[0].stage_name, Some("builder".to_string()));
        assert!(!instructions[0].is_final_stage);

        // Second FROM - final stage
        assert_eq!(instructions[2].stage_index, 1);
        assert!(instructions[2].is_final_stage);
    }
}
