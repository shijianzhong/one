use std::process::Command;

pub fn disk_usage(path: &str) -> Result<String, String> {
    let output = Command::new("du")
        .args(["-sh", path])
        .output()
        .map_err(|e| e.to_string())?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn disk_usage_detailed(path: &str, depth: usize) -> Result<String, String> {
    let output = Command::new("du")
        .args(["-h", "--max-depth", &depth.to_string(), path])
        .output()
        .map_err(|e| e.to_string())?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn disk_free() -> Result<String, String> {
    let output = Command::new("df")
        .args(["-h", "."])
        .output()
        .map_err(|e| e.to_string())?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}