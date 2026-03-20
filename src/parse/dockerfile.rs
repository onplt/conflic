use super::DockerInstruction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DockerFromImageReference<'a> {
    pub prefix: &'a str,
    pub image: &'a str,
    pub tag: &'a str,
    pub digest: Option<&'a str>,
    pub token: &'a str,
    pub suffix: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LogicalDockerLine {
    text: String,
    start_line: usize,
    start_offset: usize,
    end_offset: usize,
}

/// Parse a Dockerfile into a list of instructions with stage tracking.
pub fn parse_dockerfile(raw: &str) -> Vec<DockerInstruction> {
    let mut instructions = Vec::new();
    let mut stage_index: usize = 0;
    let mut stage_name: Option<String> = None;
    let logical_lines = logical_docker_lines(raw);
    let mut from_lines: Vec<usize> = Vec::new();

    // First pass: find all FROM lines to determine which is final
    for (i, line) in logical_lines.iter().enumerate() {
        let trimmed = line.text.trim();
        if trimmed.to_uppercase().starts_with("FROM ") {
            from_lines.push(i);
        }
    }

    let total_stages = from_lines.len();
    let mut current_from_index = 0;

    for line in logical_lines {
        let trimmed = line.text.trim();

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
            line: line.start_line,
            stage_index,
            stage_name: stage_name.clone(),
            is_final_stage,
        });
    }

    instructions
}

pub fn docker_instruction_offsets(raw: &str, start_line: usize) -> Option<(usize, usize)> {
    logical_docker_lines(raw)
        .into_iter()
        .find(|line| line.start_line == start_line)
        .map(|line| (line.start_offset, line.end_offset))
}

fn logical_docker_lines(raw: &str) -> Vec<LogicalDockerLine> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut start_line = 1;
    let mut start_offset = 0;
    let mut end_offset = 0;
    let mut byte_offset = 0;

    for (index, raw_line) in raw.split_inclusive('\n').enumerate() {
        let line_number = index + 1;
        let raw_line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let raw_line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let trimmed_end = raw_line.trim_end();
        let continued = trimmed_end.ends_with('\\');
        let segment = if continued {
            trimmed_end[..trimmed_end.len() - 1].trim_end()
        } else {
            trimmed_end
        }
        .trim();
        let physical_end_offset = byte_offset + raw_line.len();

        if current.is_empty() {
            start_line = line_number;
            start_offset = byte_offset + first_non_whitespace_byte(raw_line);
        }

        if !segment.is_empty() {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(segment);
        }

        if !current.is_empty() || !segment.is_empty() {
            end_offset = physical_end_offset;
        }

        if !continued && !current.is_empty() {
            lines.push(LogicalDockerLine {
                text: std::mem::take(&mut current),
                start_line,
                start_offset,
                end_offset,
            });
        }

        byte_offset += raw_line.len();
        if raw.as_bytes().get(byte_offset) == Some(&b'\r') {
            byte_offset += 1;
        }
        if raw.as_bytes().get(byte_offset) == Some(&b'\n') {
            byte_offset += 1;
        }
    }

    if !current.is_empty() {
        lines.push(LogicalDockerLine {
            text: current,
            start_line,
            start_offset,
            end_offset,
        });
    }

    lines
}

fn first_non_whitespace_byte(raw: &str) -> usize {
    raw.char_indices()
        .find_map(|(index, ch)| (!ch.is_whitespace()).then_some(index))
        .unwrap_or(raw.len())
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

pub fn docker_from_image_reference(arguments: &str) -> Option<DockerFromImageReference<'_>> {
    let bytes = arguments.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() {
            return None;
        }

        let token_start = index;
        while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let token_end = index;
        let token = &arguments[token_start..token_end];

        if token.starts_with("--") {
            if !token.contains('=') {
                while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                    index += 1;
                }
                while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
                    index += 1;
                }
            }
            continue;
        }

        let (image, tag, digest) = split_image_and_tag(token)?;
        return Some(DockerFromImageReference {
            prefix: &arguments[..token_start],
            image,
            tag,
            digest,
            token,
            suffix: &arguments[token_end..],
        });
    }

    None
}

fn split_image_and_tag(image_reference: &str) -> Option<(&str, &str, Option<&str>)> {
    let (without_digest, digest) = image_reference
        .split_once('@')
        .map(|(image, digest)| (image, Some(digest)))
        .unwrap_or((image_reference, None));
    let last_slash = without_digest
        .rfind('/')
        .map(|index| index + 1)
        .unwrap_or(0);
    let tag_index = without_digest[last_slash..].rfind(':')? + last_slash;
    let tag = without_digest.get(tag_index + 1..)?;

    (!tag.is_empty()).then_some((
        &without_digest[..tag_index],
        tag,
        digest.filter(|digest| !digest.is_empty()),
    ))
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

    #[test]
    fn test_docker_from_image_reference_skips_platform_flags() {
        let reference =
            docker_from_image_reference("--platform=linux/amd64 node:20-alpine AS build")
                .expect("docker image reference should be parsed");

        assert_eq!(reference.prefix, "--platform=linux/amd64 ");
        assert_eq!(reference.image, "node");
        assert_eq!(reference.tag, "20-alpine");
        assert_eq!(reference.token, "node:20-alpine");
        assert_eq!(reference.suffix, " AS build");
    }

    #[test]
    fn test_docker_from_image_reference_handles_registry_ports() {
        let reference = docker_from_image_reference("localhost:5000/node:20-alpine")
            .expect("docker image reference should be parsed");

        assert_eq!(reference.image, "localhost:5000/node");
        assert_eq!(reference.tag, "20-alpine");
    }

    #[test]
    fn test_docker_from_image_reference_preserves_digest_component() {
        let reference = docker_from_image_reference("node:20-alpine@sha256:deadbeef AS build")
            .expect("docker image reference should be parsed");

        assert_eq!(reference.image, "node");
        assert_eq!(reference.tag, "20-alpine");
        assert_eq!(reference.digest, Some("sha256:deadbeef"));
        assert_eq!(reference.token, "node:20-alpine@sha256:deadbeef");
        assert_eq!(reference.suffix, " AS build");
    }

    #[test]
    fn test_parse_dockerfile_joins_line_continuations() {
        let input = "FROM --platform=linux/amd64 \\\n  node:20-alpine AS build\n";
        let instructions = parse_dockerfile(input);

        assert_eq!(instructions.len(), 1);
        assert_eq!(instructions[0].instruction, "FROM");
        assert_eq!(
            instructions[0].arguments,
            "--platform=linux/amd64 node:20-alpine AS build"
        );
        assert_eq!(instructions[0].line, 1);
    }

    #[test]
    fn test_docker_instruction_offsets_cover_multiline_from_instruction() {
        let input =
            "  FROM --platform=linux/amd64 \\\r\n    node:20-alpine AS build\r\nRUN npm ci\r\n";
        let (start, end) =
            docker_instruction_offsets(input, 1).expect("instruction offsets should exist");

        assert_eq!(
            &input[start..end],
            "FROM --platform=linux/amd64 \\\r\n    node:20-alpine AS build"
        );
    }
}
