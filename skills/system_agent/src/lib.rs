pub mod tools;

use tools::{process, disk, file};

#[derive(Debug, Clone)]
pub struct SystemAgent;

impl SystemAgent {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum Tool {
    ListProcesses,
    TopMemoryProcs(usize),
    KillProcess(u32),
    GetProcessDetail(u32),
    DiskUsage(String),
    DiskFree,
    DeleteFile(String),
    ListDir(String),
    FileInfo(String),
    OpenApp(String),
}

impl Tool {
    pub fn name(&self) -> &'static str {
        match self {
            Tool::ListProcesses => "list_processes",
            Tool::TopMemoryProcs(_) => "top_memory_procs",
            Tool::KillProcess(_) => "kill_process",
            Tool::GetProcessDetail(_) => "get_process_detail",
            Tool::DiskUsage(_) => "disk_usage",
            Tool::DiskFree => "disk_free",
            Tool::DeleteFile(_) => "delete_file",
            Tool::ListDir(_) => "list_dir",
            Tool::FileInfo(_) => "file_info",
            Tool::OpenApp(_) => "open_app",
        }
    }

    pub fn is_dangerous(&self) -> bool {
        matches!(self, Tool::KillProcess(_) | Tool::DeleteFile(_))
    }

    pub fn description(&self) -> &str {
        match self {
            Tool::ListProcesses => "List all running processes with PID, name, memory and CPU usage",
            Tool::TopMemoryProcs(_) => "Get top N processes by memory usage",
            Tool::KillProcess(_) => "Force kill a process by PID",
            Tool::GetProcessDetail(_) => "Get detailed info about a specific process",
            Tool::DiskUsage(_) => "Show disk usage for a directory path",
            Tool::DiskFree => "Show available disk space on all mounted volumes",
            Tool::DeleteFile(_) => "Delete a file or directory (irreversible)",
            Tool::ListDir(_) => "List contents of a directory",
            Tool::FileInfo(_) => "Get information about a file (size, modified date, type)",
            Tool::OpenApp(_) => "Open an application by its bundle identifier on macOS",
        }
    }

    pub fn execute(&self) -> Result<String, String> {
        match self {
            Tool::ListProcesses => {
                let procs = process::list_processes()?;
                Ok(serde_json::to_string_pretty(&procs).unwrap_or_default())
            }
            Tool::TopMemoryProcs(n) => {
                let procs = process::top_memory_procs(*n)?;
                Ok(serde_json::to_string_pretty(&procs).unwrap_or_default())
            }
            Tool::KillProcess(pid) => {
                process::kill_process(*pid)?;
                Ok(format!("Process {} killed", pid))
            }
            Tool::GetProcessDetail(pid) => {
                let cmd = process::get_process_cmd(*pid)?;
                Ok(cmd)
            }
            Tool::DiskUsage(path) => {
                disk::disk_usage_detailed(path, 1)
            }
            Tool::DiskFree => disk::disk_free(),
            Tool::DeleteFile(path) => {
                file::delete_file(path)?;
                Ok(format!("Deleted: {}", path))
            }
            Tool::ListDir(path) => {
                let entries = file::list_dir(path)?;
                Ok(entries.join("\n"))
            }
            Tool::FileInfo(path) => file::file_info(path),
            Tool::OpenApp(bundle_id) => {
                file::open_app(bundle_id)?;
                Ok(format!("Opened: {}", bundle_id))
            }
        }
    }

    pub fn from_task_llm(task: &str, base_url: &str, api_key: &str, model: &str) -> Result<Vec<(Tool, Option<String>)>, String> {
        let tools_json = serde_json::json!([
            {"name": "list_processes", "description": "List all running processes with PID, name, memory and CPU usage", "params": []},
            {"name": "top_memory_procs", "description": "Get top N processes by memory usage", "params": ["n: number (default 10)"]},
            {"name": "kill_process", "description": "Force kill a process by PID", "params": ["pid: number"], "dangerous": true},
            {"name": "get_process_detail", "description": "Get detailed info about a specific process", "params": ["pid: number"]},
            {"name": "disk_usage", "description": "Show disk usage for a directory path", "params": ["path: string (default '.')"]},
            {"name": "disk_free", "description": "Show available disk space on all mounted volumes", "params": []},
            {"name": "delete_file", "description": "Delete a file or directory (irreversible)", "params": ["path: string"], "dangerous": true},
            {"name": "list_dir", "description": "List contents of a directory", "params": ["path: string (default '.')"]},
            {"name": "file_info", "description": "Get information about a file", "params": ["path: string"]},
            {"name": "open_app", "description": "Open an application by its bundle identifier on macOS", "params": ["bundle_id: string (e.g., com.apple.Safari)"]}
        ]);

        let prompt = format!(
            r#"你是一个系统管理助手。当用户询问关于电脑信息或操作时，你需要理解用户意图并决定调用哪些工具。

用户问题: {}
可用工具:
{}

请以JSON格式返回你要调用的工具列表，格式如下:
{{
  "tools": [
    {{"name": "工具名", "params": {{"参数名": "参数值"}}}},
    ...
  ],
  "reasoning": "你的推理过程"
}}

注意:
1. 只返回JSON，不要有其他文字
2. 如果用户问题不需要任何工具（如只是打招呼），返回空的tools数组
3. 如果需要多个工具，按顺序执行
4. 对于危险操作（kill_process, delete_file），即使被请求，也只在params中包含明确的参数值（不是变量）
5. path参数支持中文路径描述如"桌面"、"下载"、"文档"等
6. 如果问题涉及内存、CPU、运行中的应用等，优先使用list_processes或top_memory_procs
7. 如果问题涉及磁盘空间、硬盘占用，使用disk_usage或disk_free
8. 如果问题涉及文件/目录操作，使用list_dir、file_info或delete_file
9. 如果要打开应用，使用open_app，需要bundle_id如com.apple.Safari
10. 从问题中提取尽可能精确的参数值"#, task, tools_json);

        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "你是一个系统管理助手，根据用户的问题决定调用哪些工具。".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: prompt,
            },
        ];

        let response = call_llm_sync(base_url, api_key, model, &messages)?;

        parse_llm_response(&response)
    }

    pub async fn from_task_llm_async(task: &str, base_url: &str, api_key: &str, model: &str) -> Result<Vec<(Tool, Option<String>)>, String> {
        let tools_json = serde_json::json!([
            {"name": "list_processes", "description": "List all running processes with PID, name, memory and CPU usage", "params": []},
            {"name": "top_memory_procs", "description": "Get top N processes by memory usage", "params": ["n: number (default 10)"]},
            {"name": "kill_process", "description": "Force kill a process by PID", "params": ["pid: number"], "dangerous": true},
            {"name": "get_process_detail", "description": "Get detailed info about a specific process", "params": ["pid: number"]},
            {"name": "disk_usage", "description": "Show disk usage for a directory path", "params": ["path: string (default '.')"]},
            {"name": "disk_free", "description": "Show available disk space on all mounted volumes", "params": []},
            {"name": "delete_file", "description": "Delete a file or directory (irreversible)", "params": ["path: string"], "dangerous": true},
            {"name": "list_dir", "description": "List contents of a directory", "params": ["path: string (default '.')"]},
            {"name": "file_info", "description": "Get information about a file", "params": ["path: string"]},
            {"name": "open_app", "description": "Open an application by its bundle identifier on macOS", "params": ["bundle_id: string (e.g., com.apple.Safari)"]}
        ]);

        let prompt = format!(r#"你是一个系统管理助手。当用户询问关于电脑信息或操作时，你需要理解用户意图并决定调用哪些工具。

用户问题: {}
可用工具:
{}

请以JSON格式返回你要调用的工具列表，格式如下:
{{
  "tools": [
    {{"name": "工具名", "params": {{"参数名": "参数值"}}}},
    ...
  ],
  "reasoning": "你的推理过程"
}}

注意:
1. 只返回JSON，不要有其他文字
2. 如果用户问题不需要任何工具（如只是打招呼），返回空的tools数组
3. 如果需要多个工具，按顺序执行
4. 对于危险操作（kill_process, delete_file），即使被请求，也只在params中包含明确的参数值（不是变量）
5. path参数支持中文路径描述如"桌面"、"下载"、"文档"等
6. 如果问题涉及内存、CPU、运行中的应用等，优先使用list_processes或top_memory_procs
7. 如果问题涉及磁盘空间、硬盘占用，使用disk_usage或disk_free
8. 如果问题涉及文件/目录操作，使用list_dir、file_info或delete_file
9. 如果要打开应用，使用open_app，需要bundle_id如com.apple.Safari
10. 从问题中提取尽可能精确的参数值"#, task, tools_json);

        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "你是一个系统管理助手，根据用户的问题决定调用哪些工具。".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: prompt,
            },
        ];

        let response = call_llm_async(base_url, api_key, model, &messages).await?;

        parse_llm_response(&response)
    }

    pub fn from_task(task: &str) -> Vec<Tool> {
        let task_lower = task.to_lowercase();
        let mut tools = Vec::new();

        if task_lower.contains("进程") || task_lower.contains("运行中") || task_lower.contains("应用") {
            if task_lower.contains("内存") || task_lower.contains("占") || task_lower.contains("top") {
                tools.push(Tool::TopMemoryProcs(10));
            } else {
                tools.push(Tool::ListProcesses);
            }
        }

        if task_lower.contains("杀") || task_lower.contains("关闭") || task_lower.contains("终止") {
            if let Some(pid) = extract_pid(&task_lower) {
                tools.push(Tool::KillProcess(pid));
            }
        }

        if task_lower.contains("硬盘") || task_lower.contains("磁盘") || task_lower.contains("空间") || task_lower.contains("占用") {
            if task_lower.contains("剩余") || task_lower.contains("可用") {
                tools.push(Tool::DiskFree);
            } else {
                let path = extract_path(&task_lower).unwrap_or_else(|| ".".to_string());
                tools.push(Tool::DiskUsage(path));
            }
        }

        if task_lower.contains("删除") || task_lower.contains("删") {
            if let Some(path) = extract_path(&task_lower) {
                tools.push(Tool::DeleteFile(path));
            }
        }

        if task_lower.contains("目录") || task_lower.contains("文件夹") || task_lower.contains("列出") {
            let path = extract_path(&task_lower).unwrap_or_else(|| ".".to_string());
            tools.push(Tool::ListDir(path));
        }

        if task_lower.contains("打开") && task_lower.contains("应用") {
            if let Some(bundle) = extract_bundle_id(&task_lower) {
                tools.push(Tool::OpenApp(bundle));
            }
        }

        tools
    }
}

async fn call_llm_async(base_url: &str, api_key: &str, model: &str, messages: &[ChatMessage]) -> Result<String, String> {
    let client = reqwest::Client::new();

    let request_body = serde_json::json!({
        "model": model,
        "messages": messages,
        "temperature": 0.1,
        "max_tokens": 500
    });

    let resp = client
        .post(format!("{}/chat/completions", base_url))
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Failed to parse LLM response".to_string())
}

fn call_llm_sync(base_url: &str, api_key: &str, model: &str, messages: &[ChatMessage]) -> Result<String, String> {
    let client = reqwest::blocking::Client::new();

    let request_body = serde_json::json!({
        "model": model,
        "messages": messages,
        "temperature": 0.1,
        "max_tokens": 500
    });

    let resp = client
        .post(format!("{}/chat/completions", base_url))
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .map_err(|e| e.to_string())?;

    let json: serde_json::Value = resp.json().map_err(|e| e.to_string())?;

    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Failed to parse LLM response".to_string())
}

fn parse_llm_response(response: &str) -> Result<Vec<(Tool, Option<String>)>, String> {
    let response = response.trim();

    let json_start = response.find('{').unwrap_or(0);
    let json_str = &response[json_start..];

    let parsed: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| format!("Failed to parse JSON: {} - Response: {}", e, response))?;

    let tools_array = parsed["tools"]
        .as_array()
        .ok_or("Missing 'tools' field in response")?;

    let mut results = Vec::new();

    for tool_entry in tools_array {
        let name = tool_entry["name"].as_str().unwrap_or("");
        let params = &tool_entry["params"];

        let tool = match name {
            "list_processes" => Tool::ListProcesses,
            "top_memory_procs" => {
                let n = params["n"].as_u64().unwrap_or(10) as usize;
                Tool::TopMemoryProcs(n)
            }
            "kill_process" => {
                let pid = params["pid"].as_u64().unwrap_or(0) as u32;
                Tool::KillProcess(pid)
            }
            "get_process_detail" => {
                let pid = params["pid"].as_u64().unwrap_or(0) as u32;
                Tool::GetProcessDetail(pid)
            }
            "disk_usage" => {
                let path = params["path"].as_str().unwrap_or(".");
                Tool::DiskUsage(expand_path(path))
            }
            "disk_free" => Tool::DiskFree,
            "delete_file" => {
                let path = params["path"].as_str().unwrap_or("");
                if path.is_empty() {
                    continue;
                }
                Tool::DeleteFile(expand_path(path))
            }
            "list_dir" => {
                let path = params["path"].as_str().unwrap_or(".");
                Tool::ListDir(expand_path(path))
            }
            "file_info" => {
                let path = params["path"].as_str().unwrap_or("");
                if path.is_empty() {
                    continue;
                }
                Tool::FileInfo(expand_path(path))
            }
            "open_app" => {
                let bundle_id = params["bundle_id"].as_str().unwrap_or("");
                if bundle_id.is_empty() {
                    continue;
                }
                Tool::OpenApp(bundle_id.to_string())
            }
            _ => continue,
        };

        let dangerous = tool.is_dangerous();
        results.push((tool, dangerous.then_some("DANGEROUS".to_string())));
    }

    Ok(results)
}

fn expand_path(path: &str) -> String {
    let expanded = if path.starts_with("~/") || path == "~" {
        dirs::home_dir()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string())
    } else {
        path.to_string()
    };

    let chinese_map = [
        ("桌面", "Desktop"),
        ("桌面文件夹", "Desktop"),
        ("下载", "Downloads"),
        ("下载文件夹", "Downloads"),
        ("文档", "Documents"),
        ("文档文件夹", "Documents"),
        ("图片", "Pictures"),
        ("图片文件夹", "Pictures"),
        ("音乐", "Music"),
        ("音乐文件夹", "Music"),
        ("视频", "Movies"),
        ("视频文件夹", "Movies"),
    ];

    let mut result = expanded;
    for (cn, en) in &chinese_map {
        if result.contains(cn) {
            let home = dirs::home_dir().map(|h| h.to_string_lossy().to_string()).unwrap_or_default();
            result = result.replace(cn, &format!("{}/{}", home, en));
        }
    }

    result
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

fn extract_pid(task: &str) -> Option<u32> {
    let patterns = ["pid", "进程", "id"];
    for pat in patterns {
        if let Some(idx) = task.find(pat) {
            let after = &task[idx..];
            let nums: String = after.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
            if !nums.is_empty() {
                return nums.parse().ok();
            }
        }
    }
    None
}

fn extract_path(task: &str) -> Option<String> {
    let markers = ["/", "~/", " Desktop", " Documents", " Downloads"];
    for marker in markers {
        if let Some(idx) = task.find(marker) {
            let start = if marker.starts_with(' ') { idx } else { idx };
            let end = task[start..].find(' ').map(|i| start + i).unwrap_or(task.len());
            return Some(task[start..end].to_string());
        }
    }
    None
}

fn extract_bundle_id(task: &str) -> Option<String> {
    let patterns = ["-b", "bundle:", "app:"];
    for pat in patterns {
        if let Some(idx) = task.find(pat) {
            let after = &task[idx..];
            let id: String = after.chars().skip_while(|c| c.is_whitespace() || *c == ':' || *c == '=').take_while(|c| !c.is_whitespace() && *c != ',' && *c != '。' && *c != '，').collect();
            if !id.is_empty() {
                return Some(id);
            }
        }
    }
    None
}

pub fn requires_confirmation(tools: &[(Tool, Option<String>)]) -> bool {
    tools.iter().any(|(_, dangerous)| dangerous.is_some())
}

pub fn format_confirmation_request(tools: &[(Tool, Option<String>)]) -> String {
    let mut msg = "⚠️ 即将执行危险操作:\n".to_string();
    for (tool, _) in tools {
        match tool {
            Tool::KillProcess(pid) => msg.push_str(&format!("  - 终止进程 PID={}\n", pid)),
            Tool::DeleteFile(path) => msg.push_str(&format!("  - 删除文件 {}\n", path)),
            _ => {}
        }
    }
    msg.push_str("\n确认执行? (yes/no)");
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_processes() {
        let result = Tool::ListProcesses.execute();
        assert!(result.is_ok());
    }

    #[test]
    fn test_expand_path() {
        let path = expand_path("~/Desktop");
        assert!(path.contains("Desktop"));
    }
}