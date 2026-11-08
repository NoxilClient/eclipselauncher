//! Mojang version manifest, libraries, assets, natives and launch arguments.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use futures_util::stream::{self, StreamExt};
use serde_json::Value;

use super::util::{self, Progress};

const MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const RESOURCES: &str = "https://resources.download.minecraft.net/";
const LIBRARIES: &str = "https://libraries.minecraft.net/";

pub fn os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

pub fn os_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86" => "x86",
        "aarch64" => "arm64",
        _ => "x86_64",
    }
}

fn feature_enabled(key: &str) -> bool {
    // what the launcher asks for: plain vanilla, no demo, no custom resolution
    matches!(
        key,
        "is_demo_user" | "has_custom_resolution" | "has_quick_plays_support" | "is_quick_play_singleplayer"
    ) && false
}

fn rule_applies(rule: &Value) -> bool {
    if let Some(os) = rule.get("os") {
        if let Some(name) = os.get("name").and_then(|v| v.as_str()) {
            if name != os_name() {
                return false;
            }
        }
        if let Some(arch) = os.get("arch").and_then(|v| v.as_str()) {
            if arch != "x86" && arch != os_arch() {
                return false;
            }
        }
    }
    if let Some(features) = rule.get("features").and_then(|v| v.as_object()) {
        for (key, value) in features {
            let wanted = value.as_bool().unwrap_or(false);
            if wanted != feature_enabled(key) {
                return false;
            }
        }
    }
    true
}

/// Mojang rules: allowed unless an allow/disallow chain says otherwise.
pub fn allowed(rules: Option<&Value>) -> bool {
    let Some(list) = rules.and_then(|value| value.as_array()) else {
        return true;
    };
    if list.is_empty() {
        return true;
    }
    let mut allow = false;
    for rule in list {
        if rule_applies(rule) {
            allow = rule.get("action").and_then(|v| v.as_str()) == Some("allow");
        }
    }
    allow
}

fn arg_values(items: Option<&Value>) -> Vec<String> {
    let mut out = Vec::new();
    let Some(list) = items.and_then(|value| value.as_array()) else {
        return out;
    };
    for item in list {
        match item {
            Value::String(text) => out.push(text.clone()),
            Value::Object(_) => {
                if !allowed(item.get("rules")) {
                    continue;
                }
                match item.get("value") {
                    Some(Value::String(text)) => out.push(text.clone()),
                    Some(Value::Array(values)) => {
                        for value in values {
                            if let Some(text) = value.as_str() {
                                out.push(text.to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

struct Download {
    path: PathBuf,
    url: String,
    size: Option<u64>,
    sha1: Option<String>,
}

fn artifact(lib: &Value) -> Option<Download> {
    if let Some(downloads) = lib.get("downloads") {
        let artifact = downloads.get("artifact")?;
        let path = artifact.get("path")?.as_str()?;
        let url = artifact.get("url")?.as_str()?;
        return Some(Download {
            path: PathBuf::from(path),
            url: url.to_string(),
            size: artifact.get("size").and_then(|v| v.as_u64()),
            sha1: artifact.get("sha1").and_then(|v| v.as_str()).map(str::to_string),
        });
    }
    let name = lib.get("name")?.as_str()?;
    let base = lib.get("url").and_then(|v| v.as_str()).unwrap_or(LIBRARIES);
    let relative = util::maven_path(name);
    Some(Download {
        url: format!("{}{}", base, relative.to_string_lossy()),
        path: relative,
        size: None,
        sha1: None,
    })
}

fn natives_of(lib: &Value) -> Vec<Download> {
    let mut out = Vec::new();
    if !allowed(lib.get("rules")) {
        return out;
    }
    if let Some(natives) = lib.get("natives") {
        if let Some(key) = natives.get(os_name()).and_then(|v| v.as_str()) {
            let classifier = key.replace("${arch}", if cfg!(target_pointer_width = "32") { "32" } else { "64" });
            if let Some(dl) = lib
                .get("downloads")
                .and_then(|d| d.get("classifiers"))
                .and_then(|c| c.get(&classifier))
            {
                if let Some(path) = dl.get("path").and_then(|v| v.as_str()) {
                    out.push(Download {
                        path: PathBuf::from(path),
                        url: dl.get("url").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                        size: dl.get("size").and_then(|v| v.as_u64()),
                        sha1: dl.get("sha1").and_then(|v| v.as_str()).map(str::to_string),
                    });
                }
            }
        }
    } else if let Some(name) = lib.get("name").and_then(|v| v.as_str()) {
        if name.contains(":natives-") {
            let base = lib.get("url").and_then(|v| v.as_str()).unwrap_or(LIBRARIES);
            let relative = util::maven_path(name);
            out.push(Download {
                url: format!("{}{}", base, relative.to_string_lossy()),
                path: relative,
                size: None,
                sha1: None,
            });
        }
    }
    out
}

/// Version manifest, cached for an hour.
pub async fn version_manifest(data_dir: &Path) -> Result<Value, String> {
    let meta_dir = data_dir.join("meta");
    util::mkdirp(&meta_dir).map_err(|e| e.to_string())?;
    let cache = meta_dir.join("version_manifest_v2.json");
    if util::exists(&cache) {
        let fresh = std::fs::metadata(&cache)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|time| time.elapsed().ok())
            .map(|age| age.as_secs() < 3600)
            .unwrap_or(false);
        if fresh {
            if let Ok(text) = std::fs::read_to_string(&cache) {
                if let Ok(json) = serde_json::from_str::<Value>(&text) {
                    return Ok(json);
                }
            }
        }
    }
    let json = util::get_json(MANIFEST).await?;
    if let Ok(text) = serde_json::to_string(&json) {
        let _ = std::fs::write(&cache, text);
    }
    Ok(json)
}

/// Vanilla version JSON + client jar, stored in `versions/<mc>/`.
pub async fn vanilla_version(
    data_dir: &Path,
    mc: &str,
    progress: &Progress,
) -> Result<(Value, PathBuf), String> {
    let manifest = version_manifest(data_dir).await?;
    let entry = manifest
        .get("versions")
        .and_then(|v| v.as_array())
        .and_then(|list| list.iter().find(|item| item.get("id").and_then(|id| id.as_str()) == Some(mc)))
        .ok_or_else(|| format!("Minecraft {} is not in the Mojang manifest", mc))?;
    let url = entry.get("url").and_then(|v| v.as_str()).ok_or("Version has no url")?;
    let dir = data_dir.join("versions").join(mc);
    util::mkdirp(&dir).map_err(|e| e.to_string())?;
    let json = util::get_json(url).await?;
    let json_file = dir.join(format!("{}.json", mc));
    if let Ok(text) = serde_json::to_string_pretty(&json) {
        std::fs::write(&json_file, text).map_err(|e| e.to_string())?;
    }
    let jar_file = dir.join(format!("{}.jar", mc));
    if let Some(client) = json.get("downloads").and_then(|d| d.get("client")) {
        let client_url = client.get("url").and_then(|v| v.as_str()).ok_or("client has no url")?;
        let size = client.get("size").and_then(|v| v.as_u64());
        let sha1 = client.get("sha1").and_then(|v| v.as_str());
        if !util::exists(&jar_file) || size.map(|s| util::size_of(&jar_file) != s).unwrap_or(false) {
            progress(&format!("Client {}.jar", mc));
            util::download(client_url, &jar_file, size, sha1).await?;
        }
    }
    Ok((json, jar_file))
}

/// Libraries + natives of a version chain (child -> vanilla). Returns the classpath jars.
pub async fn install_libraries(
    data_dir: &Path,
    chain: &[Value],
    natives_dir: &Path,
    progress: &Progress,
) -> Result<Vec<PathBuf>, String> {
    let libs_dir = data_dir.join("libraries");
    let mut artifacts: Vec<Download> = Vec::new();
    let mut paths: HashSet<String> = HashSet::new();
    let mut natives: Vec<Download> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for version in chain {
        let Some(libraries) = version.get("libraries").and_then(|v| v.as_array()) else {
            continue;
        };
        for lib in libraries {
            if !allowed(lib.get("rules")) {
                continue;
            }
            if let Some(download) = artifact(lib) {
                let key = lib
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(|name| {
                        let parts: Vec<&str> = name.split(':').collect();
                        format!(
                            "{}:{}:{}",
                            parts.first().copied().unwrap_or(""),
                            parts.get(1).copied().unwrap_or(""),
                            parts.get(3).copied().unwrap_or("")
                        )
                    })
                    .unwrap_or_else(|| download.path.to_string_lossy().to_string());
                if seen.insert(key) && paths.insert(download.path.to_string_lossy().to_string()) {
                    artifacts.push(download);
                }
            }
            natives.extend(natives_of(lib));
        }
    }

    let classpath: Vec<PathBuf> = artifacts.iter().map(|item| libs_dir.join(&item.path)).collect();
    let total = artifacts.len();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let failures = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    stream::iter(artifacts.into_iter())
        .map(|download| {
            let done = done.clone();
            let failures = failures.clone();
            let progress = progress.clone();
            let dest = libs_dir.join(&download.path);
            async move {
                if let Err(err) = util::download(&download.url, &dest, download.size, download.sha1.as_deref()).await {
                    failures.lock().unwrap().push(format!("{}: {}", download.path.to_string_lossy(), err));
                    return;
                }
                let count = done.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                if count % 8 == 0 || count == total {
                    progress(&format!("Libraries {}/{}", count, total));
                }
            }
        })
        .buffer_unordered(8)
        .collect::<Vec<()>>()
        .await;
    let errors = failures.lock().unwrap().clone();
    if let Some(first) = errors.first() {
        return Err(format!("{} libraries failed ({})", errors.len(), first));
    }

    util::mkdirp(natives_dir).map_err(|e| e.to_string())?;
    for native in natives {
        let dest = libs_dir.join(&native.path);
        if !util::exists(&dest) || util::size_of(&dest) == 0 {
            util::download(&native.url, &dest, native.size, native.sha1.as_deref()).await?;
        }
        let _ = util::extract_natives(&dest, natives_dir);
    }

    Ok(classpath)
}

/// Asset index + all its objects (shared between versions).
pub async fn install_assets(
    data_dir: &Path,
    version: &Value,
    progress: &Progress,
) -> Result<(PathBuf, String), String> {
    let assets_dir = data_dir.join("assets");
    let Some(index) = version.get("assetIndex").filter(|value| value.get("url").is_some()) else {
        let id = version
            .get("assets")
            .and_then(|v| v.as_str())
            .unwrap_or("legacy")
            .to_string();
        return Ok((assets_dir, id));
    };
    let index_id = index.get("id").and_then(|v| v.as_str()).ok_or("asset index has no id")?;
    let index_url = index.get("url").and_then(|v| v.as_str()).ok_or("asset index has no url")?;
    let indexes_dir = assets_dir.join("indexes");
    util::mkdirp(&indexes_dir).map_err(|e| e.to_string())?;
    let index_file = indexes_dir.join(format!("{}.json", index_id));
    if !util::exists(&index_file) {
        util::download(
            index_url,
            &index_file,
            index.get("size").and_then(|v| v.as_u64()),
            index.get("sha1").and_then(|v| v.as_str()),
        )
        .await?;
    }
    let json: Value = match std::fs::read_to_string(&index_file).ok().and_then(|text| serde_json::from_str(&text).ok()) {
        Some(json) => json,
        None => return Ok((assets_dir, index_id.to_string())),
    };
    let Some(objects) = json.get("objects").and_then(|v| v.as_object()) else {
        return Ok((assets_dir, index_id.to_string()));
    };

    let mut pending: Vec<(String, Option<u64>, PathBuf)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for object in objects.values() {
        let Some(hash) = object.get("hash").and_then(|v| v.as_str()) else {
            continue;
        };
        if !seen.insert(hash.to_string()) {
            continue;
        }
        let size = object.get("size").and_then(|v| v.as_u64());
        let sub = &hash[..2.min(hash.len())];
        let dest = assets_dir.join("objects").join(sub).join(hash);
        if util::exists(&dest) && size.map(|s| util::size_of(&dest) == s).unwrap_or(true) {
            continue;
        }
        pending.push((hash.to_string(), size, dest));
    }

    let total = pending.len();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let failures = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    stream::iter(pending.into_iter().map(Some))
        .map(|item| {
            let done = done.clone();
            let failures = failures.clone();
            let progress = progress.clone();
            async move {
                let Some((hash, size, dest)) = item else { return };
                let sub = &hash[..2.min(hash.len())];
                let url = format!("{}{}/{}", RESOURCES, sub, hash);
                if let Err(err) = util::download(&url, &dest, size, None).await {
                    failures.lock().unwrap().push(format!("{}: {}", hash, err));
                    return;
                }
                let count = done.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                if count % 200 == 0 || count == total {
                    progress(&format!("Assets {}/{}", count, total));
                }
            }
        })
        .buffer_unordered(12)
        .collect::<Vec<()>>()
        .await;
    let errors = failures.lock().unwrap().clone();
    if let Some(first) = errors.first() {
        return Err(format!("{} assets failed ({})", errors.len(), first));
    }
    Ok((assets_dir, index_id.to_string()))
}

/// Version chain: child first, vanilla last (to merge arguments and libraries).
pub fn chain_for(data_dir: &Path, id: &str) -> Result<Vec<Value>, String> {
    fn walk(data_dir: &Path, id: &str, guard: &mut HashSet<String>) -> Result<Vec<Value>, String> {
        if !guard.insert(id.to_string()) {
            return Ok(Vec::new());
        }
        let file = data_dir.join("versions").join(id).join(format!("{}.json", id));
        if !util::exists(&file) {
            return Err(format!("Version {} is not installed", id));
        }
        let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
        let json: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let mut chain = vec![json.clone()];
        if let Some(parent) = json.get("inheritsFrom").and_then(|v| v.as_str()) {
            chain.extend(walk(data_dir, parent, guard)?);
        }
        Ok(chain)
    }
    walk(data_dir, id, &mut HashSet::new())
}

pub fn client_jar(data_dir: &Path, chain: &[Value]) -> PathBuf {
    let root_id = chain
        .last()
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    data_dir.join("versions").join(&root_id).join(format!("{}.jar", root_id))
}

fn substitute(value: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'$' && index + 1 < bytes.len() && bytes[index + 1] == b'{' {
            if let Some(end) = value[index + 2..].find('}') {
                let key = &value[index + 2..index + 2 + end];
                match vars.get(key) {
                    Some(replacement) => out.push_str(replacement),
                    None => out.push_str(&value[index..=index + 1 + end + 1]),
                }
                index = index + 2 + end + 1;
                continue;
            }
        }
        let ch = value[index..].chars().next().unwrap();
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

pub struct CommandParts {
    pub jvm: Vec<String>,
    pub game: Vec<String>,
    pub main_class: String,
}

/// Complete JVM + game arguments with placeholders filled in.
pub fn build_command(chain: &[Value], vars: &HashMap<String, String>) -> CommandParts {
    let mut jvm: Vec<String> = Vec::new();
    let mut game: Vec<String> = Vec::new();
    for version in chain.iter().rev() {
        if let Some(arguments) = version.get("arguments") {
            jvm.extend(arg_values(arguments.get("jvm")));
            game.extend(arg_values(arguments.get("game")));
        }
    }
    if game.is_empty() {
        let legacy = chain
            .first()
            .and_then(|v| v.get("minecraftArguments"))
            .or_else(|| chain.last().and_then(|v| v.get("minecraftArguments")))
            .and_then(|v| v.as_str());
        if let Some(text) = legacy {
            game.extend(text.split(' ').map(str::to_string));
        }
    }
    if !jvm
        .iter()
        .any(|arg| arg == "-cp" || arg == "-classpath" || arg == "--class-path")
    {
        jvm.push("-Djava.library.path=${natives_directory}".to_string());
        jvm.push("-cp".to_string());
        jvm.push("${classpath}".to_string());
    }
    let main_class = chain
        .first()
        .and_then(|v| v.get("mainClass"))
        .or_else(|| chain.last().and_then(|v| v.get("mainClass")))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    CommandParts {
        jvm: jvm.iter().map(|arg| substitute(arg, vars)).collect(),
        game: game.iter().map(|arg| substitute(arg, vars)).collect(),
        main_class,
    }
}
