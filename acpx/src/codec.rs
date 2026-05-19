//! ACP Message Codec
//!
//! Newline-delimited JSON codec for stdio transport.
//!
//! Per the ACP spec, messages are separated by newline characters (\n).
//! No embedded newlines are allowed in messages.

use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::AcpError;
use crate::protocol::Message;

/// Encode a message to JSON bytes with newline suffix
pub fn encode(msg: &Message) -> Result<String> {
    let json = serde_json::to_string(msg)?;
    Ok(json)
}

/// Encode a value to JSON string
pub fn encode_value<T: Serialize>(value: &T) -> Result<String> {
    let json = serde_json::to_string(value)?;
    Ok(json)
}

/// Decode a message from JSON string
pub fn decode(msg: &str) -> Result<Message> {
    let trimmed = msg.trim();
    if trimmed.is_empty() {
        return Err(AcpError::CodecError("Empty message".into()).into());
    }
    let msg: Message = serde_json::from_str(trimmed)?;
    Ok(msg)
}

/// Decode a value from JSON string
pub fn decode_value<T: DeserializeOwned>(json: &str) -> Result<T> {
    let value: T = serde_json::from_str(json.trim())?;
    Ok(value)
}

/// Split a stream buffer into messages
pub fn split_messages(buffer: &str) -> Vec<&str> {
    buffer.split('\n').filter(|s| !s.trim().is_empty()).collect()
}

/// Validate that a message doesn't contain embedded newlines
pub fn validate_message(msg: &str) -> Result<()> {
    let trimmed = msg.trim();
    // Count actual newlines in the raw string
    let newline_count = msg.matches('\n').count();
    if newline_count > 0 {
        return Err(AcpError::CodecError(format!(
            "Message contains {} embedded newline(s), expected 0",
            newline_count
        )).into());
    }
    // Also ensure the message isn't all whitespace
    if trimmed.is_empty() {
        return Err(AcpError::CodecError("Message is empty or whitespace only".into()).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Id, Message, Notification, Request};

    #[test]
    fn test_encode_decode_request() {
        let msg = Message::Request(Request {
            jsonrpc: "2.0".to_string(),
            method: "ping".to_string(),
            params: None,
            id: Id::Number(1),
        });

        let encoded = encode(&msg).unwrap();
        let decoded = decode(&encoded).unwrap();

        match decoded {
            Message::Request(r) => {
                assert_eq!(r.jsonrpc, "2.0");
                assert_eq!(r.method, "ping");
                assert!(r.params.is_none());
            }
            _ => panic!("Expected Request"),
        }
    }

    #[test]
    fn test_encode_decode_notification() {
        let msg = Message::Notification(Notification {
            jsonrpc: "2.0".to_string(),
            method: "initialized".to_string(),
            params: None,
        });

        let encoded = encode(&msg).unwrap();
        let decoded = decode(&encoded).unwrap();

        match decoded {
            Message::Notification(n) => {
                assert_eq!(n.jsonrpc, "2.0");
                assert_eq!(n.method, "initialized");
            }
            _ => panic!("Expected Notification"),
        }
    }

    #[test]
    fn test_split_messages() {
        let buffer = r#"{"jsonrpc":"2.0","method":"ping","id":1}
{"jsonrpc":"2.0","method":"initialized","params":{}}
{"jsonrpc":"2.0","result":{},"id":2}"#;

        let messages = split_messages(buffer);
        assert_eq!(messages.len(), 3);
    }

    #[test]
    fn test_validate_message_valid() {
        let msg = r#"{"jsonrpc":"2.0","method":"ping","id":1}"#;
        validate_message(msg).unwrap();
    }

    #[test]
    fn test_validate_message_with_embedded_newline() {
        let msg = "{\"jsonrpc\":\"2.0\",\"method\":\"test\",\"params\":{\"text\":\"hello\nworld\"}}";
        let result = validate_message(msg);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_message_empty() {
        let result = validate_message("");
        assert!(result.is_err());
        let result = validate_message("   ");
        assert!(result.is_err());
    }
}
