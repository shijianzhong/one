//! ACP Transport Layer
//!
//! Stdio transport implementation for local agent communication.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ChildStdin, ChildStdout, ChildStderr, Command, Stdio};
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::codec::{decode, encode, validate_message};
use crate::error::AcpError;
use crate::protocol::Message;

/// Transport trait for ACP communication
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send a message
    async fn send(&mut self, msg: &Message) -> Result<()>;

    /// Receive a message
    async fn recv(&mut self) -> Result<Message>;

    /// Close the transport
    async fn close(&mut self) -> Result<()>;
}

/// Stdio transport for subprocess communication
pub struct StdioTransport {
    stdin: Arc<Mutex<ChildStdin>>,
    stdout: Arc<Mutex<BufReader<ChildStdout>>>,
    stderr: Arc<Mutex<ChildStderr>>,
}

impl StdioTransport {
    /// Create a new stdio transport from an existing child process
    pub fn new(
        stdin: ChildStdin,
        stdout: ChildStdout,
        stderr: ChildStderr,
    ) -> Self {
        Self {
            stdin: Arc::new(Mutex::new(stdin)),
            stdout: Arc::new(Mutex::new(BufReader::new(stdout))),
            stderr: Arc::new(Mutex::new(stderr)),
        }
    }

    /// Spawn an agent process and create transport
    pub async fn spawn(
        command: &str,
        args: &[&str],
        cwd: Option<&std::path::Path>,
    ) -> Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        }

        let mut child = cmd.spawn().context("Failed to spawn agent process")?;

        let stdin = child.stdin.take().context("Failed to take stdin")?;
        let stdout = child.stdout.take().context("Failed to take stdout")?;
        let stderr = child.stderr.take().context("Failed to take stderr")?;

        Ok(Self::new(stdin, stdout, stderr))
    }

    /// Send a message asynchronously
    pub async fn send_async(&mut self, msg: &Message) -> Result<()> {
        let encoded = encode(msg)?;
        validate_message(&encoded)?;

        let mut stdin = self.stdin.lock().await;
        // Use blocking write since ChildStdin is sync
        std::io::Write::write_all(&mut *stdin, encoded.as_bytes())
            .context("Failed to write to stdin")?;
        std::io::Write::write_all(&mut *stdin, b"\n")
            .context("Failed to write newline")?;
        std::io::Write::flush(&mut *stdin).context("Failed to flush stdin")?;

        Ok(())
    }

    /// Receive a message asynchronously
    pub async fn recv_async(&mut self) -> Result<Message> {
        let mut stdout = self.stdout.lock().await;
        let mut line = String::new();

        // Use blocking read since BufReader<ChildStdout> is sync
        std::io::BufRead::read_line(&mut *stdout, &mut line)
            .context("Failed to read from stdout")?;

        if line.is_empty() {
            return Err(AcpError::TransportError("EOF".to_string()).into());
        }

        let msg = decode(&line)?;
        Ok(msg)
    }

    /// Read stderr for logging/debugging
    pub async fn read_stderr(&self) -> Result<Option<String>> {
        let mut stderr = self.stderr.lock().await;
        let mut buf = [0u8; 1024];

        // Try non-blocking read
        match std::io::Read::read(&mut *stderr, &mut buf) {
            Ok(0) => Ok(None),
            Ok(n) => Ok(Some(String::from_utf8_lossy(&buf[..n]).to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

#[async_trait]
impl Transport for StdioTransport {
    async fn send(&mut self, msg: &Message) -> Result<()> {
        self.send_async(msg).await
    }

    async fn recv(&mut self) -> Result<Message> {
        self.recv_async().await
    }

    async fn close(&mut self) -> Result<()> {
        // Drop stdin to signal EOF
        let _stdin = self.stdin.lock().await;
        // stdin will be dropped here
        Ok(())
    }
}

/// Synchronous stdio transport for blocking operations
pub struct SyncStdioTransport {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr: ChildStderr,
}

impl SyncStdioTransport {
    /// Create a new synchronous stdio transport
    pub fn new(
        stdin: ChildStdin,
        stdout: ChildStdout,
        stderr: ChildStderr,
    ) -> Self {
        Self {
            stdin,
            stdout: BufReader::new(stdout),
            stderr,
        }
    }

    /// Spawn an agent process synchronously
    pub fn spawn(
        command: &str,
        args: &[&str],
        cwd: Option<&std::path::Path>,
    ) -> Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        }

        let mut child = cmd.spawn().context("Failed to spawn agent process")?;

        let stdin = child.stdin.take().context("Failed to take stdin")?;
        let stdout = child.stdout.take().context("Failed to take stdout")?;
        let stderr = child.stderr.take().context("Failed to take stderr")?;

        Ok(Self::new(stdin, stdout, stderr))
    }

    /// Send a message synchronously
    pub fn send(&mut self, msg: &Message) -> Result<()> {
        let encoded = encode(msg)?;
        validate_message(&encoded)?;

        self.stdin
            .write_all(encoded.as_bytes())
            .context("Failed to write to stdin")?;
        self.stdin
            .write_all(b"\n")
            .context("Failed to write newline")?;
        self.stdin.flush().context("Failed to flush stdin")?;

        Ok(())
    }

    /// Receive a message synchronously
    pub fn recv(&mut self) -> Result<Message> {
        let mut line = String::new();

        self.stdout
            .read_line(&mut line)
            .context("Failed to read from stdout")?;

        if line.is_empty() {
            return Err(AcpError::TransportError("EOF".to_string()).into());
        }

        let msg = decode(&line)?;
        Ok(msg)
    }

    /// Read stderr
    pub fn read_stderr(&mut self) -> Result<Option<String>> {
        let mut buf = [0u8; 1024];

        match self.stderr.read(&mut buf) {
            Ok(0) => Ok(None),
            Ok(n) => Ok(Some(String::from_utf8_lossy(&buf[..n]).to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

/// Message stream for handling multiple messages
pub struct MessageStream {
    buffer: String,
}

impl MessageStream {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    /// Feed data into the buffer
    pub fn feed(&mut self, data: &str) {
        self.buffer.push_str(data);
    }

    /// Extract complete messages from buffer
    pub fn extract_messages(&mut self) -> Vec<String> {
        let messages: Vec<String> = self
            .buffer
            .split('\n')
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string())
            .collect();

        // Keep only incomplete data in buffer
        self.buffer.clear();

        messages
    }

    /// Check if buffer has complete messages
    pub fn has_messages(&self) -> bool {
        self.buffer.contains('\n')
    }
}

impl Default for MessageStream {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Id, Message, Request};

    #[test]
    fn test_encode_decode_roundtrip() {
        let msg = Message::Request(Request {
            jsonrpc: "2.0".to_string(),
            method: "ping".to_string(),
            params: None,
            id: Id::Number(1),
        });

        let encoded = encode(&msg).unwrap();
        let decoded = decode(&encoded).unwrap();

        assert!(matches!(decoded, Message::Request(_)));
    }

    #[test]
    fn test_message_stream() {
        let mut stream = MessageStream::new();

        stream.feed("msg1\nmsg2\n");
        let messages = stream.extract_messages();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0], "msg1");
        assert_eq!(messages[1], "msg2");

        stream.feed("msg3\n");
        let messages = stream.extract_messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0], "msg3");
    }
}
