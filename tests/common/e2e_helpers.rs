#![allow(dead_code)]
#[cfg(feature = "lsp")]
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
#[cfg(feature = "lsp")]
use std::sync::mpsc::{self, Receiver};
#[cfg(feature = "lsp")]
use std::thread;
#[cfg(feature = "lsp")]
use std::time::Duration;
#[cfg(feature = "lsp")]
use tower_lsp::lsp_types::Url;

pub fn all_line_endings_are_crlf(bytes: &[u8]) -> bool {
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' && (index == 0 || bytes[index - 1] != b'\r') {
            return false;
        }
    }
    true
}

pub fn file_name(path: &str) -> Option<&str> {
    Path::new(path).file_name().and_then(|name| name.to_str())
}

#[cfg(feature = "lsp")]
pub fn lsp_uri_matches_path(uri: &str, path: &Path) -> bool {
    let Ok(url) = Url::parse(uri) else {
        return false;
    };
    let Ok(uri_path) = url.to_file_path() else {
        return false;
    };

    let normalized_uri_path = std::fs::canonicalize(&uri_path).unwrap_or(uri_path);
    let normalized_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    normalized_uri_path == normalized_path
}

#[cfg(feature = "lsp")]
pub const LSP_MESSAGE_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(feature = "lsp")]
pub struct LspMessageReader {
    receiver: Receiver<String>,
}

#[cfg(feature = "lsp")]
impl LspMessageReader {
    pub fn spawn(stdout: std::process::ChildStdout) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Some(message) = read_lsp_message_from_bufread(&mut reader) {
                if sender.send(message).is_err() {
                    break;
                }
            }
        });

        Self { receiver }
    }

    pub fn read_message(&mut self) -> Option<String> {
        self.receiver.recv_timeout(LSP_MESSAGE_TIMEOUT).ok()
    }
}

#[cfg(feature = "lsp")]
pub fn write_lsp_message(stdin: &mut impl Write, payload: &str) {
    write!(
        stdin,
        "Content-Length: {}\r\n\r\n{}",
        payload.len(),
        payload
    )
    .expect("lsp message should be written");
    stdin.flush().expect("lsp stdin should flush");
}

#[cfg(feature = "lsp")]
pub fn read_lsp_message_from_bufread(reader: &mut impl BufRead) -> Option<String> {
    let length = read_lsp_content_length(reader)?;
    let mut body = vec![0_u8; length];
    reader
        .read_exact(&mut body)
        .expect("lsp body should be readable");
    Some(String::from_utf8(body).expect("lsp body should be utf-8"))
}

#[cfg(feature = "lsp")]
pub fn read_lsp_message(reader: &mut LspMessageReader) -> Option<String> {
    reader.read_message()
}

#[cfg(feature = "lsp")]
pub fn read_lsp_message_matching(
    reader: &mut LspMessageReader,
    predicate: impl Fn(&str) -> bool,
) -> Option<String> {
    for _ in 0..48 {
        let message = read_lsp_message(reader)?;
        if predicate(&message) {
            return Some(message);
        }
    }

    None
}

#[cfg(feature = "lsp")]
pub fn read_lsp_json_message_matching(
    reader: &mut LspMessageReader,
    predicate: impl Fn(&serde_json::Value) -> bool,
) -> Option<serde_json::Value> {
    for _ in 0..48 {
        let message = read_lsp_message(reader)?;
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&message) else {
            continue;
        };
        if predicate(&json) {
            return Some(json);
        }
    }

    None
}

#[cfg(feature = "lsp")]
pub fn read_lsp_content_length(reader: &mut impl BufRead) -> Option<usize> {
    let mut content_length = None;
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = reader
            .read_line(&mut line)
            .expect("lsp header should be readable");
        if bytes_read == 0 {
            return None;
        }

        if line == "\r\n" {
            return content_length;
        }

        if let Some(value) = line.strip_prefix("Content-Length:") {
            content_length = Some(value.trim().parse().expect("content length should parse"));
        }
    }
}
