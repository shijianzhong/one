use std::fs;
use std::path::Path;

pub fn delete_file(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if p.is_dir() {
        fs::remove_dir_all(p).map_err(|e| e.to_string())
    } else {
        fs::remove_file(p).map_err(|e| e.to_string())
    }
}

pub fn list_dir(path: &str) -> Result<Vec<String>, String> {
    let entries = fs::read_dir(Path::new(path))
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    Ok(entries)
}

pub fn file_info(path: &str) -> Result<String, String> {
    let metadata = fs::metadata(Path::new(path))
        .map_err(|e| e.to_string())?;

    let mut info = format!("Size: {} bytes\n", metadata.len());
    if let Ok(modified) = metadata.modified() {
        info.push_str(&format!("Modified: {:?}\n", modified));
    }
    info.push_str(&format!("Is dir: {}\n", metadata.is_dir()));
    info.push_str(&format!("Is file: {}", metadata.is_file()));

    Ok(info)
}

pub fn open_app(bundle_id: &str) -> Result<(), String> {
    use std::process::Command;
    Command::new("open")
        .args(["-b", bundle_id])
        .output()
        .map_err(|e| e.to_string())?;
    Ok(())
}