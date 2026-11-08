//! Finds the Java runtime a version needs (8, 17, 21, 25).

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct JavaInfo {
    pub path: Option<PathBuf>,
    #[allow(dead_code)]
    pub major: u32,
    #[allow(dead_code)]
    pub source: &'static str,
}

fn is_file(path: &Path) -> bool {
    path.is_file()
}

fn list_dir(dir: &Path) -> Vec<PathBuf> {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries.flatten().map(|e| e.path()).collect(),
        Err(_) => Vec::new(),
    }
}

fn javas_in(dir: &Path, suffix: &str) -> Vec<PathBuf> {
    list_dir(dir)
        .into_iter()
        .map(|entry| entry.join(suffix))
        .filter(|candidate| is_file(candidate))
        .collect()
}

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Every Java runtime we know about, most likely first.
pub fn candidates() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "linux") {
        for root in ["/usr/lib/jvm", "/usr/java", "/opt/java"] {
            out.extend(javas_in(Path::new(root), "bin/java"));
        }
        out.extend(javas_in(&home_dir().join(".jdks"), "bin/java"));
    } else if cfg!(target_os = "macos") {
        for root in [
            PathBuf::from("/Library/Java/JavaVirtualMachines"),
            home_dir().join("Library/Java/JavaVirtualMachines"),
        ] {
            for entry in list_dir(&root) {
                out.push(entry.join("Contents/Home/bin/java"));
            }
        }
    } else if cfg!(target_os = "windows") {
        let mut roots: Vec<PathBuf> = Vec::new();
        if let Ok(program_files) = std::env::var("ProgramFiles") {
            roots.push(PathBuf::from(program_files).join("Java"));
        }
        if let Ok(program_files_x86) = std::env::var("ProgramFiles(x86)") {
            roots.push(PathBuf::from(program_files_x86).join("Java"));
        }
        for root in roots {
            for entry in list_dir(&root) {
                out.push(entry.join("bin/java.exe"));
            }
        }
    }
    out.sort_by_key(|path| path.as_os_str().len());
    out.dedup();
    out
}

/// Java major version read from `java -version` (None when it does not run).
pub fn major_of(java: &Path) -> Option<u32> {
    let output = Command::new(java).arg("-version").output().ok()?;
    let mut text = String::from_utf8_lossy(&output.stderr).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    let marker = text.find("version \"")? + "version \"".len();
    let rest = &text[marker..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let mut parts = digits.split('.');
    let first = parts.next()?.parse::<u32>().ok()?;
    if first == 1 {
        parts.next()?.parse::<u32>().ok()
    } else {
        Some(first)
    }
}

fn pattern_hint(name: &str, major: u32) -> bool {
    let lower = name.to_ascii_lowercase();
    if major == 8 {
        lower.contains("1.8")
            || lower.contains("java-8")
            || lower.contains("jdk8")
            || lower.contains("jre8")
            || lower.contains("jdk1.8")
    } else {
        let needle = major.to_string();
        lower
            .split(|c: char| !c.is_ascii_digit())
            .any(|part| part == needle)
    }
}

/// Picks the right Java: user override first, then a system runtime with the wanted major.
pub fn find_java(major: u32, override_path: Option<&str>) -> JavaInfo {
    if let Some(raw) = override_path {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let path = PathBuf::from(trimmed);
            return JavaInfo {
                major: major_of(&path).unwrap_or(major),
                path: Some(path),
                source: "settings",
            };
        }
    }
    let all = candidates();
    // first the ones whose name already looks right, then everything else
    let mut ordered: Vec<PathBuf> = all.iter().filter(|p| pattern_hint(&p.to_string_lossy(), major)).cloned().collect();
    ordered.extend(all.iter().filter(|p| !pattern_hint(&p.to_string_lossy(), major)).cloned());
    for candidate in ordered {
        if major_of(&candidate) == Some(major) {
            return JavaInfo {
                path: Some(candidate),
                major,
                source: "system",
            };
        }
    }
    let on_path = if cfg!(target_os = "windows") { "java.exe" } else { "java" };
    if major_of(Path::new(on_path)) == Some(major) {
        return JavaInfo {
            path: Some(PathBuf::from(on_path)),
            major,
            source: "path",
        };
    }
    JavaInfo {
        path: None,
        major,
        source: "missing",
    }
}
