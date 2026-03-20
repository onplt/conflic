#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct JsonStringSpan {
    pub(super) token_start: usize,
    pub(super) token_end: usize,
    pub(super) quote: char,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct JsonValueSpan {
    pub(super) start: usize,
    pub(super) end: usize,
}

#[derive(Debug)]
struct ParsedStringToken {
    decoded: String,
    token_start: usize,
    token_end: usize,
    quote: char,
}

pub(super) struct JsonEditor<'a> {
    src: &'a str,
    bytes: &'a [u8],
}

impl<'a> JsonEditor<'a> {
    pub(super) fn new(src: &'a str) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
        }
    }

    pub(super) fn find_string_value_span(
        &self,
        path: &[String],
    ) -> Result<Option<JsonStringSpan>, String> {
        let mut index = 0;
        let result = self.find_in_value(&mut index, path)?;
        self.skip_ignored(&mut index)?;
        Ok(result)
    }

    pub(super) fn find_value_span(&self, path: &[String]) -> Result<Option<JsonValueSpan>, String> {
        let mut index = 0;
        let result = self.find_value_in_value(&mut index, path)?;
        self.skip_ignored(&mut index)?;
        Ok(result)
    }

    fn find_in_value(
        &self,
        index: &mut usize,
        path: &[String],
    ) -> Result<Option<JsonStringSpan>, String> {
        self.skip_ignored(index)?;

        match self.peek_byte(*index) {
            Some(b'{') if !path.is_empty() => self.find_in_object(index, path),
            Some(b'{') => {
                self.skip_object(index)?;
                Ok(None)
            }
            Some(b'[') => {
                self.skip_array(index)?;
                Ok(None)
            }
            Some(b'"') | Some(b'\'') => {
                let token = self.parse_string_token(index)?;
                Ok(path.is_empty().then_some(JsonStringSpan {
                    token_start: token.token_start,
                    token_end: token.token_end,
                    quote: token.quote,
                }))
            }
            Some(_) => {
                self.skip_primitive(index)?;
                Ok(None)
            }
            None => Err("Unexpected end of JSON input".into()),
        }
    }

    fn find_value_in_value(
        &self,
        index: &mut usize,
        path: &[String],
    ) -> Result<Option<JsonValueSpan>, String> {
        self.skip_ignored(index)?;

        if path.is_empty() {
            let start = *index;
            self.skip_value(index)?;
            return Ok(Some(JsonValueSpan { start, end: *index }));
        }

        match self.peek_byte(*index) {
            Some(b'{') => self.find_value_in_object(index, path),
            Some(b'[') => {
                self.skip_array(index)?;
                Ok(None)
            }
            Some(b'"') | Some(b'\'') => {
                self.parse_string_token(index)?;
                Ok(None)
            }
            Some(_) => {
                self.skip_primitive(index)?;
                Ok(None)
            }
            None => Err("Unexpected end of JSON input".into()),
        }
    }

    fn find_in_object(
        &self,
        index: &mut usize,
        path: &[String],
    ) -> Result<Option<JsonStringSpan>, String> {
        self.expect_byte(index, b'{')?;
        self.skip_ignored(index)?;

        if self.consume_byte(index, b'}') {
            return Ok(None);
        }

        let mut last_match = None;

        loop {
            self.skip_ignored(index)?;
            let key = self.parse_object_key(index)?;
            self.skip_ignored(index)?;
            self.expect_byte(index, b':')?;

            if key == path[0] {
                if let Some(span) = self.find_in_value(index, &path[1..])? {
                    last_match = Some(span);
                }
            } else {
                self.skip_value(index)?;
            }

            self.skip_ignored(index)?;
            if self.consume_byte(index, b',') {
                self.skip_ignored(index)?;
                if self.consume_byte(index, b'}') {
                    return Ok(last_match);
                }
                continue;
            }

            self.expect_byte(index, b'}')?;
            return Ok(last_match);
        }
    }

    fn find_value_in_object(
        &self,
        index: &mut usize,
        path: &[String],
    ) -> Result<Option<JsonValueSpan>, String> {
        self.expect_byte(index, b'{')?;
        self.skip_ignored(index)?;

        if self.consume_byte(index, b'}') {
            return Ok(None);
        }

        let mut last_match = None;

        loop {
            self.skip_ignored(index)?;
            let key = self.parse_object_key(index)?;
            self.skip_ignored(index)?;
            self.expect_byte(index, b':')?;

            if key == path[0] {
                if let Some(span) = self.find_value_in_value(index, &path[1..])? {
                    last_match = Some(span);
                }
            } else {
                self.skip_value(index)?;
            }

            self.skip_ignored(index)?;
            if self.consume_byte(index, b',') {
                self.skip_ignored(index)?;
                if self.consume_byte(index, b'}') {
                    return Ok(last_match);
                }
                continue;
            }

            self.expect_byte(index, b'}')?;
            return Ok(last_match);
        }
    }

    fn skip_value(&self, index: &mut usize) -> Result<(), String> {
        self.skip_ignored(index)?;

        match self.peek_byte(*index) {
            Some(b'{') => self.skip_object(index),
            Some(b'[') => self.skip_array(index),
            Some(b'"') | Some(b'\'') => {
                self.parse_string_token(index)?;
                Ok(())
            }
            Some(_) => self.skip_primitive(index),
            None => Err("Unexpected end of JSON input".into()),
        }
    }

    fn skip_object(&self, index: &mut usize) -> Result<(), String> {
        self.expect_byte(index, b'{')?;
        self.skip_ignored(index)?;

        if self.consume_byte(index, b'}') {
            return Ok(());
        }

        loop {
            self.skip_ignored(index)?;
            self.parse_object_key(index)?;
            self.skip_ignored(index)?;
            self.expect_byte(index, b':')?;
            self.skip_value(index)?;
            self.skip_ignored(index)?;

            if self.consume_byte(index, b',') {
                self.skip_ignored(index)?;
                if self.consume_byte(index, b'}') {
                    return Ok(());
                }
                continue;
            }

            self.expect_byte(index, b'}')?;
            return Ok(());
        }
    }

    fn skip_array(&self, index: &mut usize) -> Result<(), String> {
        self.expect_byte(index, b'[')?;
        self.skip_ignored(index)?;

        if self.consume_byte(index, b']') {
            return Ok(());
        }

        loop {
            self.skip_value(index)?;
            self.skip_ignored(index)?;

            if self.consume_byte(index, b',') {
                self.skip_ignored(index)?;
                if self.consume_byte(index, b']') {
                    return Ok(());
                }
                continue;
            }

            self.expect_byte(index, b']')?;
            return Ok(());
        }
    }

    fn skip_primitive(&self, index: &mut usize) -> Result<(), String> {
        let start = *index;

        while let Some(byte) = self.peek_byte(*index) {
            if byte.is_ascii_whitespace() || matches!(byte, b',' | b'}' | b']') {
                break;
            }

            if byte == b'/' && matches!(self.peek_byte(*index + 1), Some(b'/') | Some(b'*')) {
                break;
            }

            *index += 1;
        }

        if *index == start {
            Err(format!("Unexpected token at byte {}", start))
        } else {
            Ok(())
        }
    }

    fn parse_object_key(&self, index: &mut usize) -> Result<String, String> {
        match self.peek_byte(*index) {
            Some(b'"') | Some(b'\'') => Ok(self.parse_string_token(index)?.decoded),
            Some(byte) if is_identifier_start(byte) => Ok(self.parse_identifier(index)),
            Some(other) => Err(format!(
                "Expected JSON object key at byte {}, found '{}'",
                *index, other as char
            )),
            None => Err("Unexpected end of JSON input while reading object key".into()),
        }
    }

    fn parse_identifier(&self, index: &mut usize) -> String {
        let start = *index;
        while let Some(byte) = self.peek_byte(*index) {
            if is_identifier_continue(byte) {
                *index += 1;
            } else {
                break;
            }
        }
        self.src[start..*index].to_string()
    }

    fn parse_string_token(&self, index: &mut usize) -> Result<ParsedStringToken, String> {
        let token_start = *index;
        let quote = self
            .peek_byte(*index)
            .map(|byte| byte as char)
            .ok_or_else(|| "Unexpected end of JSON input while reading string".to_string())?;
        *index += 1;

        let mut decoded = String::new();

        while *index < self.bytes.len() {
            let current = self.next_char(*index)?;

            if current == quote {
                *index += current.len_utf8();
                return Ok(ParsedStringToken {
                    decoded,
                    token_start,
                    token_end: *index,
                    quote,
                });
            }

            if current == '\\' {
                *index += current.len_utf8();
                decoded.push(self.parse_escape_sequence(index)?);
                continue;
            }

            if current == '\n' || current == '\r' {
                return Err("Unterminated JSON string literal".into());
            }

            decoded.push(current);
            *index += current.len_utf8();
        }

        Err("Unterminated JSON string literal".into())
    }

    fn parse_escape_sequence(&self, index: &mut usize) -> Result<char, String> {
        let escape = self
            .next_char(*index)
            .map_err(|_| "Unexpected end of JSON escape sequence".to_string())?;
        *index += escape.len_utf8();

        match escape {
            '"' => Ok('"'),
            '\'' => Ok('\''),
            '\\' => Ok('\\'),
            '/' => Ok('/'),
            'b' => Ok('\u{0008}'),
            'f' => Ok('\u{000C}'),
            'n' => Ok('\n'),
            'r' => Ok('\r'),
            't' => Ok('\t'),
            'u' => self.parse_hex_escape(index, 4),
            'x' => self.parse_hex_escape(index, 2),
            other => Ok(other),
        }
    }

    fn parse_hex_escape(&self, index: &mut usize, digits: usize) -> Result<char, String> {
        let end = *index + digits;
        if end > self.bytes.len() {
            return Err("Unexpected end of JSON unicode escape".into());
        }

        let raw = &self.src[*index..end];
        let value = u32::from_str_radix(raw, 16)
            .map_err(|_| format!("Invalid hex escape sequence '{}'", raw))?;
        *index = end;
        char::from_u32(value).ok_or_else(|| format!("Invalid unicode scalar value {:X}", value))
    }

    fn skip_ignored(&self, index: &mut usize) -> Result<(), String> {
        while let Some(byte) = self.peek_byte(*index) {
            match byte {
                b' ' | b'\t' | b'\n' | b'\r' => *index += 1,
                b'/' if self.peek_byte(*index + 1) == Some(b'/') => {
                    *index += 2;
                    while let Some(next) = self.peek_byte(*index) {
                        *index += 1;
                        if next == b'\n' {
                            break;
                        }
                    }
                }
                b'/' if self.peek_byte(*index + 1) == Some(b'*') => {
                    *index += 2;
                    let mut closed = false;
                    while *index + 1 < self.bytes.len() {
                        if self.bytes[*index] == b'*' && self.bytes[*index + 1] == b'/' {
                            *index += 2;
                            closed = true;
                            break;
                        }
                        *index += 1;
                    }

                    if !closed {
                        return Err("Unterminated block comment in JSONC input".into());
                    }
                }
                _ => break,
            }
        }

        Ok(())
    }

    fn expect_byte(&self, index: &mut usize, expected: u8) -> Result<(), String> {
        match self.peek_byte(*index) {
            Some(byte) if byte == expected => {
                *index += 1;
                Ok(())
            }
            Some(byte) => Err(format!(
                "Expected '{}' at byte {}, found '{}'",
                expected as char, *index, byte as char
            )),
            None => Err(format!(
                "Expected '{}' at end of JSON input",
                expected as char
            )),
        }
    }

    fn consume_byte(&self, index: &mut usize, expected: u8) -> bool {
        if self.peek_byte(*index) == Some(expected) {
            *index += 1;
            true
        } else {
            false
        }
    }

    fn peek_byte(&self, index: usize) -> Option<u8> {
        self.bytes.get(index).copied()
    }

    fn next_char(&self, index: usize) -> Result<char, String> {
        self.src[index..]
            .chars()
            .next()
            .ok_or_else(|| "Unexpected end of JSON input".to_string())
    }
}

fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$')
}

fn is_identifier_continue(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit()
}
