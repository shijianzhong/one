use std::path::PathBuf;

pub(crate) fn titlebar_leading_inset() -> f32 {
    if cfg!(target_os = "macos") {
        86.0
    } else {
        16.0
    }
}

pub(crate) fn pick_folder_dialog() -> Option<(PathBuf, String)> {
    use std::process::Command;
    let output = Command::new("osascript")
        .args(["-e", "POSIX path of (choose folder)"])
        .output()
        .ok()?;
    let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path_str.is_empty() {
        return None;
    }
    let path = PathBuf::from(&path_str);
    let name = path.file_name()?.to_string_lossy().to_string();
    Some((path, name))
}

pub(crate) fn collect_html_files(root: &std::path::Path) -> Vec<PathBuf> {
    fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>, depth: usize) {
        if depth > 4 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if [".git", "node_modules", "target"].contains(&name) {
                        continue;
                    }
                }
                walk(&path, out, depth + 1);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("html"))
                .unwrap_or(false)
            {
                out.push(path);
            }
        }
    }

    let mut out = Vec::new();
    walk(root, &mut out, 0);
    out
}

pub(crate) fn extract_html_hints(text: &str) -> Vec<String> {
    let mut hints = Vec::new();
    let mut token = String::new();

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' || ch == '/' {
            token.push(ch);
        } else if !token.is_empty() {
            if token.to_ascii_lowercase().ends_with(".html") {
                hints.push(token.clone());
            }
            token.clear();
        }
    }
    if !token.is_empty() && token.to_ascii_lowercase().ends_with(".html") {
        hints.push(token);
    }

    hints
}
