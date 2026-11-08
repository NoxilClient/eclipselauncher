//! Download, cache and filesystem helpers shared by the whole engine.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use futures_util::StreamExt;
use serde_json::Value;
use sha1::{Digest, Sha1};
use tokio::io::AsyncWriteExt;

pub const UA: &str = "EclipseClient/0.3 (standalone launcher)";

pub type Progress = Arc<dyn Fn(&str) + Send + Sync>;

pub fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(UA)
            .build()
            .expect("http client")
    })
}

pub async fn get_json(url: &str) -> Result<Value, String> {
    let res = client().get(url).send().await.map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("HTTP {} {}", res.status().as_u16(), url));
    }
    res.json::<Value>().await.map_err(|e| e.to_string())
}

pub fn exists(path: &Path) -> bool {
    path.is_file()
}

pub fn size_of(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

pub fn mkdirp(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}

fn sha1_of(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = Sha1::new();
    hasher.update(&bytes);
    Some(hasher.finalize().iter().map(|b| format!("{:02x}", b)).collect())
}

/// Downloads `url` into `dest`. Skips files that are already there with the expected
/// size (or any non-empty content when the size is unknown), writes to `<dest>.part`
/// and renames only when the download is complete.
pub async fn download(
    url: &str,
    dest: &Path,
    size: Option<u64>,
    sha1: Option<&str>,
) -> Result<(), String> {
    if exists(dest) {
        let current = size_of(dest);
        let ok = match size {
            Some(expected) => current == expected,
            None => current > 0,
        };
        if ok {
            return Ok(());
        }
    }
    if let Some(parent) = dest.parent() {
        mkdirp(parent).map_err(|e| e.to_string())?;
    }
    let mut tmp_name = dest.as_os_str().to_os_string();
    tmp_name.push(".part");
    let tmp = PathBuf::from(tmp_name);

    let mut last_error = String::from("download failed");
    for attempt in 0..3u32 {
        match fetch_to(url, &tmp).await {
            Ok(()) => {
                if let Some(expected) = sha1 {
                    match sha1_of(&tmp) {
                        Some(actual) if actual.eq_ignore_ascii_case(expected) => {}
                        _ => {
                            let _ = std::fs::remove_file(&tmp);
                            last_error = format!("checksum mismatch: {}", url);
                            tokio::time::sleep(std::time::Duration::from_millis(
                                400 * (attempt as u64 + 1),
                            ))
                            .await;
                            continue;
                        }
                    }
                }
                std::fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
                return Ok(());
            }
            Err(err) => {
                let _ = std::fs::remove_file(&tmp);
                last_error = err;
                tokio::time::sleep(std::time::Duration::from_millis(400 * (attempt as u64 + 1))).await;
            }
        }
    }
    Err(last_error)
}

async fn fetch_to(url: &str, tmp: &Path) -> Result<(), String> {
    let res = client().get(url).send().await.map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("HTTP {} {}", res.status().as_u16(), url));
    }
    let mut file = tokio::fs::File::create(tmp).await.map_err(|e| e.to_string())?;
    let mut body = res.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
    }
    file.flush().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// ``group:artifact:version[:classifier]`` -> path inside the libraries folder.
pub fn maven_path(name: &str) -> PathBuf {
    let parts: Vec<&str> = name.split(':').collect();
    let group = parts.first().copied().unwrap_or("");
    let artifact = parts.get(1).copied().unwrap_or("");
    let version = parts.get(2).copied().unwrap_or("");
    let classifier = parts.get(3).copied();
    let file = match classifier {
        Some(c) => format!("{}-{}-{}.jar", artifact, version, c),
        None => format!("{}-{}.jar", artifact, version),
    };
    PathBuf::from(group.replace('.', "/"))
        .join(artifact)
        .join(version)
        .join(file)
}

/// Extracts the native libraries of a jar into `target`.
pub fn extract_natives(jar: &Path, target: &Path) -> std::io::Result<()> {
    let file = std::fs::File::open(jar)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::create_dir_all(target)?;
    for index in 0..archive.len() {
        let mut entry = match archive.by_index(index) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let name = entry.name().to_string();
        if entry.is_dir() || name.starts_with("META-INF/") {
            continue;
        }
        let out = target.join(&name);
        if out.exists() {
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut sink = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut sink)?;
        #[cfg(unix)]
        if name.ends_with(".so") || name.ends_with(".dylib") || name.ends_with(".jnilib") {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755));
        }
    }
    Ok(())
}
