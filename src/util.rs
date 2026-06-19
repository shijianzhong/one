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

pub(crate) fn strip_think_tags(text: &str) -> String {
    let mut result = String::new();
    let mut inside_think = false;
    let mut temp = String::new();

    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if !inside_think && ch == '<' {
            // Check for <think>
            let mut matched = false;
            if let Some(&'t') = chars.peek() {
                let remaining: String = text
                    .chars()
                    .skip(text.chars().count() - chars.clone().count())
                    .take(6)
                    .collect();
                if remaining == "think>" {
                    inside_think = true;
                    for _ in 0..6 {
                        chars.next();
                    }
                    matched = true;
                }
            }
            if !matched {
                result.push(ch);
            }
        } else if inside_think && ch == '<' {
            // Check for </think>
            let mut matched = false;
            if let Some(&'/') = chars.peek() {
                let remaining: String = text
                    .chars()
                    .skip(text.chars().count() - chars.clone().count())
                    .take(7)
                    .collect();
                if remaining == "/think>" {
                    inside_think = false;
                    for _ in 0..7 {
                        chars.next();
                    }
                    matched = true;
                }
            }
            if !matched {
                // Stay inside think
            }
        } else if !inside_think {
            result.push(ch);
        }
    }

    result.trim().to_string()
}
