// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Content-Length framed message I/O for DAP.
//!
//! DAP uses the same wire format as LSP: `Content-Length: N\r\n\r\n<JSON payload>`.
//! The JSON envelope differs (seq/type/command instead of JSON-RPC id/method) but
//! the framing is identical.

use std::io;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::types::{DapEvent, DapRequest, DapResponse};

/// Maximum accepted DAP payload size.
///
/// DAP requests are normally small JSON documents. Keeping an explicit limit prevents a
/// malformed `Content-Length` header from forcing the adapter to reserve arbitrary amounts of
/// memory before it has received a payload.
const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;

/// Maximum size of the complete DAP header block.
const MAX_HEADER_SIZE: usize = 8 * 1024;

/// Read a single DAP message from a Content-Length framed stream.
/// Returns the raw JSON string, or `None` on EOF.
pub async fn read_message(reader: &mut (impl AsyncBufRead + Unpin)) -> io::Result<Option<String>> {
    let mut content_length: Option<usize> = None;
    let mut header_size = 0;

    // Parse headers.
    loop {
        let mut line = String::new();
        // Cap the read itself, not just the post-read validation, so an unterminated header line
        // cannot grow `line` without bound.
        let n = reader
            .take((MAX_HEADER_SIZE + 1) as u64)
            .read_line(&mut line)
            .await?;
        if n == 0 {
            return if header_size == 0 {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "DAP headers ended before the terminating blank line",
                ))
            };
        }

        header_size += n;
        if header_size > MAX_HEADER_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("DAP headers exceed {MAX_HEADER_SIZE} bytes"),
            ));
        }

        if !line.ends_with('\n') {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete DAP header line",
            ));
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            break; // End of headers
        }

        let Some((name, value)) = trimmed.split_once(':') else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "malformed DAP header",
            ));
        };

        if name.eq_ignore_ascii_case("Content-Length") {
            if content_length.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "duplicate Content-Length header",
                ));
            }
            content_length = Some(value.trim().parse().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid Content-Length header")
            })?);
        }
        // Ignore other headers (e.g., Content-Type).
    }

    let length = content_length.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length header")
    })?;

    if length > MAX_MESSAGE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("DAP message exceeds {MAX_MESSAGE_SIZE} bytes"),
        ));
    }

    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).await?;

    String::from_utf8(body)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Write a JSON-serializable message with Content-Length framing.
pub async fn write_message(writer: &mut (impl AsyncWrite + Unpin), json: &str) -> io::Result<()> {
    let header = format!("Content-Length: {}\r\n\r\n", json.len());
    writer.write_all(header.as_bytes()).await?;
    writer.write_all(json.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

/// Parse a raw JSON string into a DAP request.
pub fn parse_request(json: &str) -> serde_json::Result<DapRequest> {
    #[derive(serde::Deserialize)]
    struct RequestEnvelope {
        seq: u64,
        #[serde(rename = "type")]
        message_type: String,
        command: String,
        #[serde(default)]
        arguments: serde_json::Value,
    }

    let envelope: RequestEnvelope = serde_json::from_str(json)?;
    if envelope.message_type != "request" {
        return Err(<serde_json::Error as serde::de::Error>::custom(format!(
            "expected a DAP request, got {:?}",
            envelope.message_type
        )));
    }

    Ok(DapRequest {
        seq: envelope.seq,
        command: envelope.command,
        arguments: envelope.arguments,
    })
}

/// Serialize a DAP response to JSON.
pub fn serialize_response(response: &DapResponse) -> String {
    serde_json::to_string(response).expect("DapResponse must be serializable")
}

/// Serialize a DAP event to JSON.
pub fn serialize_event(event: &DapEvent) -> String {
    serde_json::to_string(event).expect("DapEvent must be serializable")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn read_write_roundtrip() {
        let json_body = r#"{"seq":1,"type":"request","command":"initialize","arguments":{}}"#;

        // Write a framed message into a buffer.
        let mut buf: Vec<u8> = Vec::new();
        write_message(&mut buf, json_body).await.unwrap();

        // Read it back.
        let mut cursor = io::Cursor::new(&buf);
        let result = read_message(&mut cursor).await.unwrap().unwrap();
        assert_eq!(result, json_body);
    }

    #[test]
    fn parse_initialize_request() {
        let json = r#"{"seq":1,"type":"request","command":"initialize","arguments":{"clientID":"example-editor"}}"#;
        let req = parse_request(json).unwrap();
        assert_eq!(req.seq, 1);
        assert_eq!(req.command, "initialize");
        assert_eq!(req.arguments["clientID"], "example-editor");
    }

    #[test]
    fn parse_request_rejects_missing_required_fields() {
        assert!(parse_request(r#"{"type":"request","command":"initialize"}"#).is_err());
        assert!(parse_request(r#"{"seq":1,"type":"request"}"#).is_err());
    }

    #[test]
    fn parse_request_rejects_non_request_messages() {
        let event = r#"{"seq":1,"type":"event","command":"initialize"}"#;
        assert!(parse_request(event).is_err());
    }

    #[tokio::test]
    async fn read_rejects_duplicate_or_invalid_content_length() {
        for input in [
            "Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
            "Content-Length: nope\r\n\r\n{}",
        ] {
            let mut cursor = io::Cursor::new(input.as_bytes());
            let error = read_message(&mut cursor).await.unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        }
    }

    #[tokio::test]
    async fn read_distinguishes_clean_eof_from_truncated_headers() {
        let mut empty = io::Cursor::new(&[]);
        assert!(read_message(&mut empty).await.unwrap().is_none());

        let mut truncated = io::Cursor::new(b"Content-Length: 2\r\n".as_slice());
        let error = read_message(&mut truncated).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn read_accepts_case_insensitive_content_length() {
        let mut cursor = io::Cursor::new(b"content-length: 2\r\n\r\n{}".as_slice());
        assert_eq!(
            read_message(&mut cursor).await.unwrap().as_deref(),
            Some("{}")
        );
    }

    #[tokio::test]
    async fn read_rejects_oversized_message_before_allocating_body() {
        let input = format!("Content-Length: {}\r\n\r\n", MAX_MESSAGE_SIZE + 1);
        let mut cursor = io::Cursor::new(input.as_bytes());
        let error = read_message(&mut cursor).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
