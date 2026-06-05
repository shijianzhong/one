use std::io::ErrorKind;
use std::process::Command;

#[derive(Debug, serde::Serialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub memory_mb: f64,
    pub mem_percent: f64,
    pub cpu_percent: f64,
    pub command: String,
}

pub fn list_processes() -> Result<Vec<ProcessInfo>, String> {
    let output = Command::new("ps")
        .args(["aux"])
        .output()
        .map_err(|e| {
            if e.kind() == ErrorKind::PermissionDenied {
                None
            } else {
                Some(e.to_string())
            }
        });

    let output = match output {
        Ok(output) => output,
        Err(None) => return Ok(Vec::new()),
        Err(Some(error)) => return Err(error),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut processes = Vec::new();

    for line in stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 11 {
            let pid: u32 = parts[1].parse().unwrap_or(0);
            let cpu = parts[2].parse().unwrap_or(0.0);
            let mem_pct = parts[3].parse().unwrap_or(0.0);
            // RSS (parts[5]) 单位 KB，转为 MB
            let rss_kb: f64 = parts[5].parse().unwrap_or(0.0);
            let mem_mb = rss_kb / 1024.0;
            let command = parts[10..].join(" ");
            let name = parts[10].split('/').last().unwrap_or(parts[10]).to_string();

            processes.push(ProcessInfo {
                pid,
                name,
                memory_mb: (mem_mb * 100.0).round() / 100.0, // 保留两位小数
                mem_percent: mem_pct,
                cpu_percent: cpu,
                command,
            });
        }
    }

    Ok(processes)
}

pub fn kill_process(pid: u32) -> Result<(), String> {
    Command::new("kill")
        .args(["-9", &pid.to_string()])
        .output()
        .map_err(|e| e.to_string())?;

    Ok(())
}

pub fn top_memory_procs(n: usize) -> Result<Vec<ProcessInfo>, String> {
    let mut procs = list_processes()?;
    procs.sort_by(|a, b| b.memory_mb.partial_cmp(&a.memory_mb).unwrap());
    procs.truncate(n);
    Ok(procs)
}

pub fn get_process_cmd(pid: u32) -> Result<String, String> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command"])
        .output()
        .map_err(|e| e.to_string())?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
