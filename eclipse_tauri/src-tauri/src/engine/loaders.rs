//! Mod loaders: Fabric profile, Forge installer, and the legacy 1.8.9 Forge profile.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::util::{self, Progress};

const FORGE_MAVEN: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge/";

pub struct LoaderInstall {
    pub id: String,
    #[allow(dead_code)]
    pub json: Value,
}

/// Official Fabric profile from meta.fabricmc.net, written into the launcher's versions folder.
pub async fn install_fabric(data_dir: &Path, mc: &str, progress: &Progress) -> Result<LoaderInstall, String> {
    let list = util::get_json(&format!("https://meta.fabricmc.net/v2/versions/loader/{}", mc)).await?;
    let entries = list.as_array().ok_or_else(|| format!("Fabric returned no loaders for {}", mc))?;
    if entries.is_empty() {
        return Err(format!("Fabric has no loader for {}", mc));
    }
    let entry = entries
        .iter()
        .find(|item| {
            item.get("loader")
                .and_then(|loader| loader.get("stable"))
                .and_then(|stable| stable.as_bool())
                .unwrap_or(false)
        })
        .unwrap_or(&entries[0]);
    let loader = entry
        .get("loader")
        .and_then(|loader| loader.get("version"))
        .and_then(|version| version.as_str())
        .ok_or("Fabric loader has no version")?
        .to_string();
    progress(&format!("Fabric {} for {}", loader, mc));
    let profile = util::get_json(&format!(
        "https://meta.fabricmc.net/v2/versions/loader/{}/{}/profile/json",
        mc, loader
    ))
    .await?;
    let id = profile
        .get("id")
        .and_then(|value| value.as_str())
        .ok_or("Fabric profile has no id")?
        .to_string();
    let dir = data_dir.join("versions").join(&id);
    util::mkdirp(&dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&profile).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{}.json", id)), text).map_err(|e| e.to_string())?;
    Ok(LoaderInstall { id, json: profile })
}

/// Forge installed with its own official installer (client side, headless).
pub async fn install_forge(
    data_dir: &Path,
    mc: &str,
    spec: &str,
    java: &Path,
    progress: &Progress,
) -> Result<LoaderInstall, String> {
    let url = format!("{}{}/forge-{}-installer.jar", FORGE_MAVEN, spec, spec);
    let meta_dir = data_dir.join("meta");
    util::mkdirp(&meta_dir).map_err(|e| e.to_string())?;
    let installer = meta_dir.join(format!("forge-{}-installer.jar", spec));
    progress(&format!("Forge installer {}", spec));
    util::download(&url, &installer, None, None).await?;

    // the installer expects a launcher_profiles.json in the target folder
    let profiles = data_dir.join("launcher_profiles.json");
    if !util::exists(&profiles) {
        let body = "{\n  \"profiles\": {},\n  \"settings\": {},\n  \"version\": 3\n}";
        std::fs::write(&profiles, body).map_err(|e| e.to_string())?;
    }

    let output = tokio::process::Command::new(java)
        .arg("-jar")
        .arg(&installer)
        .arg("--installClient")
        .arg(data_dir)
        .current_dir(data_dir)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        let mut text = String::from_utf8_lossy(&output.stderr).to_string();
        text.push('\n');
        text.push_str(&String::from_utf8_lossy(&output.stdout));
        let tail: Vec<&str> = text.trim().lines().rev().take(6).collect();
        return Err(format!(
            "Forge installer failed ({}): {}",
            output.status.code().unwrap_or(-1),
            tail.into_iter().rev().collect::<Vec<_>>().join(" | ")
        ));
    }

    let versions_dir = data_dir.join("versions");
    let mut found: Vec<(String, Value)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&versions_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let file = versions_dir.join(&name).join(format!("{}.json", name));
            let Ok(text) = std::fs::read_to_string(&file) else { continue };
            let Ok(json) = serde_json::from_str::<Value>(&text) else { continue };
            let inherits = json.get("inheritsFrom").and_then(|v| v.as_str()).unwrap_or("");
            if inherits == mc && name.to_ascii_lowercase().contains("forge") {
                found.push((name, json));
            }
        }
    }
    found.sort_by_key(|(id, _)| std::cmp::Reverse(id.len()));
    let (id, json) = found
        .into_iter()
        .next()
        .ok_or_else(|| format!("Forge installed but there is no profile for {}", mc))?;
    Ok(LoaderInstall { id, json })
}

fn read_entry(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<Vec<u8>> {
    let mut entry = archive.by_name(name).ok()?;
    let mut buffer = Vec::new();
    entry.read_to_end(&mut buffer).ok()?;
    Some(buffer)
}

/// Old Forge (1.8.9): its installer has no `--installClient`, but the jar already
/// contains `install_profile.json` with the version JSON and the universal jar.
pub async fn install_forge_legacy(
    data_dir: &Path,
    spec: &str,
    progress: &Progress,
) -> Result<LoaderInstall, String> {
    let url = format!("{}{}/forge-{}-installer.jar", FORGE_MAVEN, spec, spec);
    let meta_dir = data_dir.join("meta");
    util::mkdirp(&meta_dir).map_err(|e| e.to_string())?;
    let installer_path = meta_dir.join(format!("forge-{}-installer.jar", spec));
    progress(&format!("Forge installer {}", spec));
    util::download(&url, &installer_path, None, None).await?;

    let file = std::fs::File::open(&installer_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let profile_bytes = read_entry(&mut archive, "install_profile.json")
        .ok_or("Forge installer has no install_profile.json")?;

    let mut universal: Option<Vec<u8>> = None;
    for index in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(index) else { continue };
        let name = entry.name().to_string();
        if name.ends_with("-universal.jar") {
            let mut buffer = Vec::new();
            if entry.read_to_end(&mut buffer).is_ok() {
                universal = Some(buffer);
                break;
            }
        }
    }
    let universal = universal.ok_or("Forge installer has no universal jar")?;

    let profile: Value = serde_json::from_slice(&profile_bytes).map_err(|e| e.to_string())?;
    let mut info = profile
        .get("versionInfo")
        .filter(|value| value.get("id").is_some())
        .cloned()
        .ok_or("Forge installer has no version info")?;
    let id = info
        .get("id")
        .and_then(|value| value.as_str())
        .ok_or("Forge version info has no id")?
        .to_string();

    let forge_name = {
        let libraries = info
            .get_mut("libraries")
            .and_then(|value| value.as_array_mut())
            .ok_or("Forge version has no libraries")?;
        let index = libraries
            .iter()
            .position(|lib| {
                lib.get("name")
                    .and_then(|value| value.as_str())
                    .map(|name| name.starts_with("net.minecraftforge:forge:"))
                    .unwrap_or(false)
            })
            .ok_or("Forge version has no forge library")?;
        let lib = &mut libraries[index];
        let name = lib
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let name = if name.split(':').count() == 3 {
            format!("{}:universal", name)
        } else {
            name
        };
        lib["name"] = Value::String(name.clone());
        if lib.get("url").is_none() {
            lib["url"] = Value::String(FORGE_MAVEN.to_string());
        }
        name
    };

    let library_path: PathBuf = data_dir.join("libraries").join(util::maven_path(&forge_name));
    if let Some(parent) = library_path.parent() {
        util::mkdirp(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&library_path, &universal).map_err(|e| e.to_string())?;

    let dir = data_dir.join("versions").join(&id);
    util::mkdirp(&dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&info).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{}.json", id)), text).map_err(|e| e.to_string())?;
    Ok(LoaderInstall { id, json: info })
}
