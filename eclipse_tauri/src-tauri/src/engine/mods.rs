//! Mod browser: Modrinth and CurseForge search, downloads, installed list, manifest.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::util::{self, Progress};

const CURSEFORGE: &str = "https://api.curseforge.com/v1";

fn loader_slug(loader: &str) -> &'static str {
    if loader.eq_ignore_ascii_case("fabric") {
        "fabric"
    } else {
        "forge"
    }
}

fn curseforge_loader_type(loader: &str) -> u32 {
    match loader.to_ascii_lowercase().as_str() {
        "fabric" | "quilt" => 4,
        "neoforge" => 5,
        _ => 1,
    }
}

fn manifest_file(instance_dir: &Path) -> PathBuf {
    instance_dir.join("mods-manifest.json")
}

fn read_manifest(instance_dir: &Path) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(manifest_file(instance_dir)) else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    json.get("mods")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default()
}

fn write_manifest(instance_dir: &Path, mods: &[Value]) {
    let _ = util::mkdirp(instance_dir);
    let body = json!({ "mods": mods });
    if let Ok(text) = serde_json::to_string_pretty(&body) {
        let _ = std::fs::write(manifest_file(instance_dir), text);
    }
}

fn is_jar(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".jar")
}

fn jars_in(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if is_jar(&name) {
                out.push(name);
            }
        }
    }
    out
}

/// Mods of an instance: the jars the client ships plus the ones the user downloaded.
pub fn get_mods(instance_dir: &Path, bundled_dir: Option<&Path>) -> Vec<Value> {
    let manifest = read_manifest(instance_dir);
    let by_file: HashMap<String, Value> = manifest
        .iter()
        .filter_map(|entry| {
            let file = entry.get("file")?.as_str()?.to_string();
            Some((file, entry.clone()))
        })
        .collect();

    let mut found: Vec<Value> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    if let Some(dir) = bundled_dir {
        for name in jars_in(dir) {
            let size = util::size_of(&dir.join(&name));
            seen.insert(name.clone());
            found.push(json!({
                "name": name,
                "size": size,
                "bundled": true,
                "origin": "eclipse",
                "project": Value::Null,
                "removable": false
            }));
        }
    }
    let mods_dir = instance_dir.join("mods");
    for name in jars_in(&mods_dir) {
        if seen.contains(&name) {
            continue;
        }
        let size = util::size_of(&mods_dir.join(&name));
        let project = by_file.get(&name).and_then(|entry| {
            let source = entry.get("source").and_then(|v| v.as_str()).unwrap_or("modrinth");
            let id = entry.get("id").map(|v| v.to_string().trim_matches('"').to_string())?;
            Some(format!("{}:{}", source, id))
        });
        found.push(json!({
            "name": name,
            "size": size,
            "bundled": false,
            "origin": "downloaded",
            "project": project,
            "removable": true
        }));
    }
    found.sort_by(|a, b| {
        a.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(b.get("name").and_then(|v| v.as_str()).unwrap_or(""))
    });
    found
}

/// Project ids ("source:id") already installed in an instance.
pub fn installed_projects(instance_dir: &Path) -> Vec<String> {
    read_manifest(instance_dir)
        .iter()
        .filter_map(|entry| {
            let source = entry.get("source").and_then(|v| v.as_str()).unwrap_or("modrinth");
            let id = entry.get("id")?.to_string().trim_matches('"').to_string();
            Some(format!("{}:{}", source, id))
        })
        .collect()
}

pub fn remove_mod(instance_dir: &Path, name: &str) -> Result<bool, String> {
    let file = Path::new(name)
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    if !is_jar(&file) {
        return Err("Not a mod file".into());
    }
    let full = instance_dir.join("mods").join(&file);
    if !util::exists(&full) {
        return Ok(false);
    }
    std::fs::remove_file(&full).map_err(|e| e.to_string())?;
    let manifest = read_manifest(instance_dir);
    let rest: Vec<Value> = manifest
        .into_iter()
        .filter(|entry| entry.get("file").and_then(|v| v.as_str()) != Some(file.as_str()))
        .collect();
    write_manifest(instance_dir, &rest);
    Ok(true)
}

pub async fn search(
    source: &str,
    query: &str,
    version: &str,
    loader: &str,
    api_key: &str,
) -> Result<Vec<Value>, String> {
    if version.is_empty() {
        return Err("Missing Minecraft version".into());
    }
    if source == "curseforge" {
        search_curseforge(query, version, loader, api_key).await
    } else {
        search_modrinth(query, version, loader).await
    }
}

async fn search_modrinth(query: &str, version: &str, loader: &str) -> Result<Vec<Value>, String> {
    let facets = json!([
        ["project_type:mod"],
        [format!("versions:{}", version)],
        [format!("categories:{}", loader_slug(loader))]
    ]);
    let url = format!(
        "https://api.modrinth.com/v2/search?query={}&facets={}&limit=30&index=relevance",
        encode(query),
        encode(&facets.to_string())
    );
    let json = util::get_json(&url).await.map_err(|e| format!("Modrinth: {}", e))?;
    let hits = json
        .get("hits")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(hits
        .iter()
        .map(|hit| {
            json!({
                "id": hit.get("project_id").cloned().unwrap_or(Value::Null),
                "slug": hit.get("slug").cloned().unwrap_or(Value::Null),
                "name": hit.get("title").cloned().unwrap_or(Value::Null),
                "description": hit.get("description").cloned().unwrap_or(Value::Null),
                "icon": hit.get("icon_url").cloned().unwrap_or(Value::Null),
                "downloads": hit.get("downloads").cloned().unwrap_or(Value::Null),
                "source": "modrinth"
            })
        })
        .collect())
}

async fn search_curseforge(
    query: &str,
    version: &str,
    loader: &str,
    api_key: &str,
) -> Result<Vec<Value>, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("CURSEFORGE_KEY_MISSING".into());
    }
    let mut params = vec![
        ("gameId".to_string(), "432".to_string()),
        ("classId".to_string(), "6".to_string()),
        ("gameVersion".to_string(), version.to_string()),
        (
            "modLoaderType".to_string(),
            curseforge_loader_type(loader).to_string(),
        ),
        ("pageSize".to_string(), "30".to_string()),
        ("sortField".to_string(), "2".to_string()),
        ("sortOrder".to_string(), "desc".to_string()),
    ];
    if !query.trim().is_empty() {
        params.push(("searchFilter".to_string(), query.to_string()));
    }
    let url = format!("{}/mods/search?{}", CURSEFORGE, query_string(&params));
    let res = util::client()
        .get(&url)
        .header("x-api-key", key)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    match res.status().as_u16() {
        403 | 401 => return Err("CURSEFORGE_KEY_INVALID".into()),
        status if !res.status().is_success() => return Err(format!("CurseForge HTTP {}", status)),
        _ => {}
    }
    let json: Value = res.json().await.map_err(|e| e.to_string())?;
    let data = json
        .get("data")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(data
        .iter()
        .map(|item| {
            json!({
                "id": item.get("id").cloned().unwrap_or(Value::Null),
                "slug": item.get("slug").cloned().unwrap_or(Value::Null),
                "name": item.get("name").cloned().unwrap_or(Value::Null),
                "description": item.get("summary").cloned().unwrap_or(Value::Null),
                "icon": item.get("logo").and_then(|logo| logo.get("thumbnailUrl").or_else(|| logo.get("url"))).cloned().unwrap_or(Value::Null),
                "downloads": item.get("downloadCount").cloned().unwrap_or(Value::Null),
                "source": "curseforge"
            })
        })
        .collect())
}

struct ModFile {
    url: String,
    name: String,
    deps: Vec<(String, Value)>,
}

async fn resolve_modrinth(project_id: &str, version: &str, loader: &str) -> Result<ModFile, String> {
    // Modrinth wants literal brackets in the query string
    let game_versions = format!("[{}]", encode(&format!("\"{}\"", version)));
    let loaders = format!("[{}]", encode(&format!("\"{}\"", loader_slug(loader))));
    let url = format!(
        "https://api.modrinth.com/v2/project/{}/version?game_versions={}&loaders={}",
        encode(project_id),
        game_versions,
        loaders
    );
    let json = util::get_json(&url).await?;
    let list = json
        .as_array()
        .ok_or("Modrinth returned no versions")?
        .clone();
    let first = list
        .first()
        .ok_or_else(|| format!("Nessuna versione per {}", version))?;
    let files = first
        .get("files")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let file = files
        .iter()
        .find(|file| file.get("primary").and_then(|v| v.as_bool()).unwrap_or(false))
        .or_else(|| files.first())
        .ok_or("Download non disponibile")?;
    let url = file
        .get("url")
        .and_then(|value| value.as_str())
        .ok_or("Download non disponibile")?;
    let name = file
        .get("filename")
        .and_then(|value| value.as_str())
        .map(|value| {
            Path::new(value)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "mod.jar".to_string())
        })
        .unwrap_or_else(|| "mod.jar".to_string());
    let mut deps = Vec::new();
    if let Some(dependencies) = first.get("dependencies").and_then(|value| value.as_array()) {
        for dep in dependencies {
            let required = dep.get("dependency_type").and_then(|v| v.as_str()) == Some("required");
            if let Some(project) = dep.get("project_id") {
                if required && !project.is_null() {
                    deps.push(("modrinth".to_string(), project.clone()));
                }
            }
        }
    }
    Ok(ModFile {
        url: url.to_string(),
        name,
        deps,
    })
}

async fn resolve_curseforge(
    mod_id: &Value,
    version: &str,
    loader: &str,
    api_key: &str,
) -> Result<ModFile, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("CURSEFORGE_KEY_MISSING".into());
    }
    let id = match mod_id {
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        _ => return Err("Missing mod id".into()),
    };
    let url = format!(
        "{}/mods/{}/files?{}",
        CURSEFORGE,
        id,
        query_string(&[
            ("gameVersion".to_string(), version.to_string()),
            (
                "modLoaderType".to_string(),
                curseforge_loader_type(loader).to_string()
            ),
        ])
    );
    let res = util::client()
        .get(&url)
        .header("x-api-key", key)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    match res.status().as_u16() {
        403 | 401 => return Err("CURSEFORGE_KEY_INVALID".into()),
        status if !res.status().is_success() => return Err(format!("CurseForge HTTP {}", status)),
        _ => {}
    }
    let json: Value = res.json().await.map_err(|e| e.to_string())?;
    let mut files = json
        .get("data")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    files.sort_by(|a, b| {
        b.get("fileDate")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(a.get("fileDate").and_then(|v| v.as_str()).unwrap_or(""))
    });
    let file = files.first().ok_or_else(|| format!("Nessuna versione per {}", version))?;
    let url = file
        .get("downloadUrl")
        .and_then(|value| value.as_str())
        .ok_or("L'autore ha disattivato i download esterni")?;
    let name = file
        .get("fileName")
        .and_then(|value| value.as_str())
        .unwrap_or("mod.jar")
        .to_string();
    Ok(ModFile {
        url: url.to_string(),
        name,
        deps: Vec::new(),
    })
}

/// Downloads a mod (and its required dependencies) into the instance `mods` folder.
pub async fn download(
    instance_dir: &Path,
    source: &str,
    id: &Value,
    version: &str,
    loader: &str,
    api_key: &str,
    progress: &Progress,
) -> Result<(Vec<String>, Vec<String>), String> {
    if version.is_empty() {
        return Err("Missing Minecraft version".into());
    }
    let mods_dir = instance_dir.join("mods");
    util::mkdirp(&mods_dir).map_err(|e| e.to_string())?;
    let mut queue: Vec<(String, Value)> = vec![(source.to_string(), id.clone())];
    let mut seen: HashSet<String> = HashSet::new();
    let mut downloaded: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut records: Vec<Value> = Vec::new();
    let mut depth = 0;

    while let Some((item_source, item_id)) = queue.first().cloned() {
        queue.remove(0);
        let key = format!("{}:{}", item_source, item_id);
        if !seen.insert(key) {
            continue;
        }
        let file = if item_source == "curseforge" {
            resolve_curseforge(&item_id, version, loader, api_key).await?
        } else {
            let project = item_id
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| item_id.to_string());
            resolve_modrinth(&project, version, loader).await?
        };
        let dest = mods_dir.join(&file.name);
        records.push(json!({ "file": file.name, "source": item_source, "id": item_id }));
        if util::exists(&dest) {
            skipped.push(file.name.clone());
        } else {
            progress(&format!("Downloading {}", file.name));
            util::download(&file.url, &dest, None, None)
                .await
                .map_err(|e| format!("{}: {}", file.name, e))?;
            downloaded.push(file.name.clone());
        }
        if depth < 3 {
            for dep in file.deps {
                queue.push(dep);
            }
            depth += 1;
        }
    }

    let mut by_file: HashMap<String, Value> = read_manifest(instance_dir)
        .into_iter()
        .filter_map(|entry| {
            let file = entry.get("file")?.as_str()?.to_string();
            Some((file, entry))
        })
        .collect();
    for record in records {
        if let Some(file) = record.get("file").and_then(|v| v.as_str()) {
            by_file.insert(file.to_string(), record);
        }
    }
    let mut manifest: Vec<Value> = by_file.into_values().collect();
    manifest.sort_by(|a, b| {
        a.get("file")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(b.get("file").and_then(|v| v.as_str()).unwrap_or(""))
    });
    write_manifest(instance_dir, &manifest);

    Ok((downloaded, skipped))
}

fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

fn query_string(params: &[(String, String)]) -> String {
    params
        .iter()
        .map(|(key, value)| format!("{}={}", encode(key), encode(value)))
        .collect::<Vec<String>>()
        .join("&")
}
