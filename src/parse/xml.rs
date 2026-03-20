use quick_xml::Reader;
use quick_xml::events::Event;

use crate::model::SourceSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlValueMatch {
    pub raw_value: String,
    pub span: SourceSpan,
}

pub fn find_tag_value_matches(raw: &str, tag: &str) -> Result<Vec<XmlValueMatch>, String> {
    let mut reader = Reader::from_str(raw);
    reader.config_mut().trim_text(false);
    let tag_bytes = tag.as_bytes();
    let mut matches = Vec::new();
    let mut last_position = 0_usize;
    let mut open_tags = Vec::new();

    loop {
        let event = reader
            .read_event()
            .map_err(|error| format!("Failed to parse XML: {}", error))?;
        let event_end = usize::try_from(reader.buffer_position())
            .map_err(|_| "XML parser reported an out-of-range buffer position".to_string())?;
        let event_start = last_position;
        last_position = event_end;

        match event {
            Event::Start(start) => {
                if start.name().as_ref() == tag_bytes {
                    open_tags.push(event_end);
                }
            }
            Event::End(end) => {
                if end.name().as_ref() == tag_bytes
                    && let Some(content_start) = open_tags.pop()
                {
                    push_trimmed_match(raw, content_start, event_start, &mut matches);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(matches)
}

pub fn find_plugin_tag_value_matches(
    raw: &str,
    plugin_artifact_id: &str,
    tag: &str,
) -> Result<Vec<XmlValueMatch>, String> {
    let mut reader = Reader::from_str(raw);
    reader.config_mut().trim_text(false);
    let mut matches = Vec::new();
    let mut last_position = 0_usize;
    let mut open_plugin_offsets = Vec::new();

    loop {
        let event = reader
            .read_event()
            .map_err(|error| format!("Failed to parse XML: {}", error))?;
        let event_end = usize::try_from(reader.buffer_position())
            .map_err(|_| "XML parser reported an out-of-range buffer position".to_string())?;
        let event_start = last_position;
        last_position = event_end;

        match event {
            Event::Start(start) => {
                if start.name().as_ref() == b"plugin" {
                    open_plugin_offsets.push(event_start);
                }
            }
            Event::End(end) => {
                if end.name().as_ref() == b"plugin"
                    && let Some(plugin_start) = open_plugin_offsets.pop()
                {
                    let plugin_end = event_end;
                    collect_plugin_tag_matches(
                        raw,
                        plugin_start,
                        plugin_end,
                        plugin_artifact_id,
                        tag,
                        &mut matches,
                    )?;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(matches)
}

fn collect_plugin_tag_matches(
    raw: &str,
    plugin_start: usize,
    plugin_end: usize,
    plugin_artifact_id: &str,
    tag: &str,
    matches: &mut Vec<XmlValueMatch>,
) -> Result<(), String> {
    if plugin_start >= plugin_end || plugin_end > raw.len() {
        return Ok(());
    }

    let plugin_raw = &raw[plugin_start..plugin_end];
    let artifact_matches = find_tag_value_matches(plugin_raw, "artifactId")?;
    if !artifact_matches
        .iter()
        .any(|artifact| artifact.raw_value.trim() == plugin_artifact_id)
    {
        return Ok(());
    }

    for plugin_match in find_tag_value_matches(plugin_raw, tag)? {
        let absolute_start = plugin_start + plugin_match.span.start;
        let absolute_end = plugin_start + plugin_match.span.end;
        matches.push(XmlValueMatch {
            raw_value: plugin_match.raw_value,
            span: crate::parse::source_location::span_from_offsets(
                raw,
                absolute_start,
                absolute_end,
            ),
        });
    }

    Ok(())
}

fn push_trimmed_match(
    raw: &str,
    content_start: usize,
    content_end: usize,
    matches: &mut Vec<XmlValueMatch>,
) {
    if content_start >= content_end || content_end > raw.len() {
        return;
    }

    let value = &raw[content_start..content_end];
    let leading = value.len() - value.trim_start().len();
    let trailing = value.len() - value.trim_end().len();
    let start = content_start + leading;
    let end = content_end.saturating_sub(trailing);
    if start >= end {
        return;
    }

    matches.push(XmlValueMatch {
        raw_value: raw[start..end].to_string(),
        span: crate::parse::source_location::span_from_offsets(raw, start, end),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_tag_value_matches_preserves_multiline_value_span() {
        let raw =
            "<TargetFramework>\n  net8.0\n</TargetFramework>\n<Description>net8.0</Description>\n";
        let matches = find_tag_value_matches(raw, "TargetFramework").unwrap();

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].raw_value, "net8.0");
        assert_eq!(matches[0].span.line, 2);
        assert_eq!(matches[0].span.column, 3);
    }

    #[test]
    fn test_find_tag_value_matches_supports_attributes() {
        let raw = "<TargetFramework Condition=\"'$(Configuration)' == 'Debug'\">net8.0</TargetFramework>\n";
        let matches = find_tag_value_matches(raw, "TargetFramework").unwrap();

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].raw_value, "net8.0");
        assert_eq!(matches[0].span.line, 1);
    }

    #[test]
    fn test_find_plugin_tag_value_matches_filters_by_plugin_artifact_id() {
        let raw = r#"
<project>
  <build>
    <plugins>
      <plugin>
        <artifactId>maven-compiler-plugin</artifactId>
        <configuration>
          <release>17</release>
        </configuration>
      </plugin>
      <plugin>
        <artifactId>build-helper-maven-plugin</artifactId>
        <configuration>
          <release>2024.03</release>
        </configuration>
      </plugin>
    </plugins>
  </build>
</project>
"#;

        let matches =
            find_plugin_tag_value_matches(raw, "maven-compiler-plugin", "release").unwrap();

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].raw_value, "17");
    }
}
