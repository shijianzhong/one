#!/usr/bin/env python3
"""
Claude Code MCP Server

ONE 内置的 MCP Server，通过 stdio 与 ONE 通信。
包装 claude -p 命令，将 Claude Code 的能力暴露为 MCP Tool。

协议: JSON-RPC 2.0 over stdio
方法:
  - tools/list  → 列出支持的工具
  - tools/call  → 调用指定工具
  - notifications/initialized → 初始化确认

启动方式: ONE 直接启动此脚本作为子进程
"""

import json
import os
import shlex
import shutil
import subprocess
import sys
from typing import Any


def find_claude() -> str:
    """查找 claude 可执行文件路径"""
    claude = shutil.which("claude")
    if claude:
        return claude
    # 常见安装路径
    candidates = [
        os.path.expanduser("~/.npm-global/bin/claude"),
        "/usr/local/bin/claude",
        "/opt/homebrew/bin/claude",
        os.path.expanduser("~/AppData/Roaming/npm/claude"),
    ]
    for c in candidates:
        if os.path.isfile(c):
            return c
    return "claude"


# ── JSON-RPC 2.0 工具定义 ─────────────────────────────────────────────────────

TOOLS = [
    {
        "name": "claude_code_run",
        "description": "通过 Claude Code 执行编码任务。支持代码编写、重构、代码审查、Bug修复等。",
        "inputSchema": {
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "要执行的编码任务描述（自然语言）"
                },
                "work_dir": {
                    "type": "string",
                    "description": "工作目录，默认为当前目录",
                    "default": "."
                },
                "max_turns": {
                    "type": "integer",
                    "description": "最大执行步数（Claude Code 内部步骤）",
                    "default": 15
                },
                "allowed_tools": {
                    "type": "string",
                    "description": "允许 Claude Code 使用的工具列表，逗号分隔",
                    "default": "Read,Edit,Write,Bash,Grep,Glob"
                },
                "require_approval": {
                    "type": "boolean",
                    "description": "是否需要用户批准每个操作",
                    "default": False
                }
            },
            "required": ["task"]
        }
    },
    {
        "name": "claude_code_status",
        "description": "检查 Claude Code 是否可用（是否已安装并认证）",
        "inputSchema": {
            "type": "object",
            "properties": {}
        }
    }
]


def handle_tools_list(request_id: int) -> dict:
    """处理 tools/list 请求"""
    return {
        "jsonrpc": "2.0",
        "id": request_id,
        "result": {"tools": TOOLS}
    }


def handle_tools_call(request_id: int, params: dict) -> dict:
    """处理 tools/call 请求"""
    tool_name = params.get("name", "")
    arguments = params.get("arguments", {})

    if tool_name == "claude_code_status":
        return handle_status(request_id)
    elif tool_name == "claude_code_run":
        return handle_run(request_id, arguments)
    else:
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {
                "code": -32601,
                "message": f"Tool not found: {tool_name}"
            }
        }


def handle_status(request_id: int) -> dict:
    """检查 Claude Code 是否可用"""
    claude_path = find_claude()
    if not os.path.isfile(claude_path):
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "content": [{
                    "type": "text",
                    "text": json.dumps({
                        "available": False,
                        "error": "claude 命令未找到，请先安装: npm install -g @anthropic-ai/claude-code"
                    })
                }]
            }
        }

    try:
        result = subprocess.run(
            [claude_path, "--version"],
            capture_output=True, text=True, timeout=10
        )
        version = result.stdout.strip() or result.stderr.strip()
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "content": [{
                    "type": "text",
                    "text": json.dumps({
                        "available": True,
                        "version": version or "unknown",
                        "path": claude_path
                    })
                }]
            }
        }
    except Exception as e:
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "content": [{
                    "type": "text",
                    "text": json.dumps({
                        "available": False,
                        "error": str(e)
                    })
                }]
            }
        }


def handle_run(request_id: int, arguments: dict) -> dict:
    """执行 claude -p 命令"""
    task = arguments.get("task", "")
    if not task:
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {
                "code": -32602,
                "message": "Missing required parameter: task"
            }
        }

    work_dir = arguments.get("work_dir", os.getcwd())
    max_turns = arguments.get("max_turns", 15)
    allowed_tools = arguments.get("allowed_tools", "Read,Edit,Write,Bash,Grep,Glob")
    require_approval = arguments.get("require_approval", False)

    claude_path = find_claude()

    # 构造 claude 命令
    cmd = [
        claude_path, "-p", task,
        "--allowedTools", allowed_tools,
        "--max-turns", str(max_turns),
        "--print",
    ]

    if require_approval:
        # 默认行为（允许审批）
        pass
    else:
        # 跳过审批（使用环境变量或 flag）
        os.environ["CLAUDE_CODE_AUTO_MODE"] = "1"

    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=300,  # 5 分钟超时
            cwd=work_dir,
            env={**os.environ}
        )

        output = result.stdout or result.stderr or ""

        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "content": [{
                    "type": "text",
                    "text": output
                }],
                "is_error": result.returncode != 0
            }
        }
    except subprocess.TimeoutExpired:
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "content": [{
                    "type": "text",
                    "text": "Claude Code 执行超时（超过 5 分钟）"
                }],
                "is_error": True
            }
        }
    except FileNotFoundError:
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {
                "code": -32000,
                "message": "claude 命令未找到。请先安装: npm install -g @anthropic-ai/claude-code"
            }
        }
    except Exception as e:
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {
                "code": -32000,
                "message": str(e)
            }
        }


def main():
    """主循环：通过 stdin 读取 JSON-RPC 请求，写入 stdout 返回响应"""
    # 初始化通知（客户端已就绪）
    sys.stderr.write("[ClaudeCode MCP] Server started\n")
    sys.stderr.flush()

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue

        try:
            request = json.loads(line)
        except json.JSONDecodeError as e:
            sys.stderr.write(f"[ClaudeCode MCP] Invalid JSON: {e}\n")
            sys.stderr.flush()
            continue

        method = request.get("method", "")
        req_id = request.get("id")

        # 通知类消息（没有 id）不需要响应
        if method == "notifications/initialized":
            sys.stderr.write("[ClaudeCode MCP] Initialized\n")
            sys.stderr.flush()
            continue

        sys.stderr.write(f"[ClaudeCode MCP] Request: {method} (id={req_id})\n")
        sys.stderr.flush()

        if method == "tools/list":
            response = handle_tools_list(req_id)
        elif method == "tools/call":
            response = handle_tools_call(req_id, request.get("params", {}))
        else:
            response = {
                "jsonrpc": "2.0",
                "id": req_id,
                "error": {
                    "code": -32601,
                    "message": f"Method not found: {method}"
                }
            }

        # 写入 stdout
        sys.stdout.write(json.dumps(response) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()