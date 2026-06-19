//! MCP 传输层实现
//!
//! 支持两种传输方式：
//! - Stdio: 启动子进程，通过 stdin/stdout 通信
//! - HTTP:  通过 HTTP/SSE 与远程 MCP Server 通信

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use anyhow::{Context, Result};

use crate::mcp::protocol::{
    JsonRpcRequest, JsonRpcResponse, LineProtocolParser, RequestIdGenerator,
};

// ── Transport trait ─────────────────────────────────────────────────────────────

/// MCP 传输层抽象
pub trait McpTransport: Send {
    /// 发送 JSON-RPC 请求并等待响应
    fn request(&mut self, req: &JsonRpcRequest) -> Result<JsonRpcResponse>;

    /// 关闭连接
    fn shutdown(&mut self) -> Result<()>;
}

// ── Stdio 传输 ──────────────────────────────────────────────────────────────────

/// Stdio 传输：启动子进程通过 stdin/stdout 通信
pub struct StdioTransport {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    parser: LineProtocolParser,
    id_gen: RequestIdGenerator,
}

impl StdioTransport {
    /// 启动一个子进程作为 MCP Server
    pub fn spawn(command: &str, args: &[String], env: &[(String, String)]) -> Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()); // stderr 继承到父进程便于调试

        for (key, value) in env {
            cmd.env(key, value);
        }

        let mut child = cmd
            .spawn()
            .context(format!("Failed to spawn MCP server: {}", command))?;

        let stdin = BufWriter::new(
            child
                .stdin
                .take()
                .context("Failed to open stdin for MCP server")?,
        );
        let stdout = BufReader::new(
            child
                .stdout
                .take()
                .context("Failed to open stdout for MCP server")?,
        );

        Ok(Self {
            child,
            stdin,
            stdout,
            parser: LineProtocolParser::new(),
            id_gen: RequestIdGenerator::new(),
        })
    }

    /// 发送初始化通知（客户端就绪）
    pub fn send_initialized(&mut self) -> Result<()> {
        let req = JsonRpcRequest::new(
            self.id_gen.next(),
            crate::mcp::protocol::methods::NOTIFICATIONS_INITIALIZED,
            None,
        );
        self.send_raw(&req)?;
        Ok(())
    }

    fn send_raw(&mut self, req: &JsonRpcRequest) -> Result<()> {
        let mut json = serde_json::to_string(req)?;
        json.push('\n');
        self.stdin.write_all(json.as_bytes())?;
        self.stdin.flush()?;
        Ok(())
    }

    fn read_response(&mut self) -> Result<JsonRpcResponse> {
        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line)?;
            if n == 0 {
                anyhow::bail!("MCP server closed connection");
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let resp: JsonRpcResponse = serde_json::from_str(trimmed)?;
            return Ok(resp);
        }
    }
}

impl McpTransport for StdioTransport {
    fn request(&mut self, req: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        self.send_raw(req)?;
        self.read_response()
    }

    fn shutdown(&mut self) -> Result<()> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
}

// ── HTTP 传输 ───────────────────────────────────────────────────────────────────

/// HTTP 传输：通过 HTTP POST 与 MCP Server 通信
pub struct HttpTransport {
    client: reqwest::blocking::Client,
    url: String,
    headers: std::collections::HashMap<String, String>,
    id_gen: RequestIdGenerator,
}

impl HttpTransport {
    pub fn new(
        url: String,
        headers: std::collections::HashMap<String, String>,
        timeout_secs: u64,
    ) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs.max(1)))
            .build()?;

        Ok(Self {
            client,
            url,
            headers,
            id_gen: RequestIdGenerator::new(),
        })
    }
}

impl McpTransport for HttpTransport {
    fn request(&mut self, req: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        let mut request_builder = self.client.post(&self.url).json(req);

        for (key, value) in &self.headers {
            request_builder = request_builder.header(key.as_str(), value.as_str());
        }

        let response = request_builder.send()?;
        let body: JsonRpcResponse = response.json()?;
        Ok(body)
    }

    fn shutdown(&mut self) -> Result<()> {
        // HTTP 无状态，无需关闭
        Ok(())
    }
}

// ── BufWriter wrapper for ChildStdin ────────────────────────────────────────────

impl Drop for StdioTransport {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
