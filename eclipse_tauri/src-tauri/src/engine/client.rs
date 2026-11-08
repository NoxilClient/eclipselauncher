//! Launcher orchestration: supported versions, install and launch.

use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::{json, Value};

use super::util::{self, Progress};
use super::{java, launch, loaders, minecraft, mods};

pub struct VersionSpec {
    pub version: &'static str,
    pub loader: &'static str,
    pub label: &'static str,
    pub java: u32,
    pub forge: Option<&'static str>,
    pub legacy_forge: bool,
}

/// The only versions Eclipse Client supports.
pub const SUPPORTED: &[VersionSpec] = &[
    VersionSpec {
        version: "1.8.9",
        loader: "Forge",
        label: "PvP classico",
        java: 8,
        forge: Some("1.8.9-11.15.1.2318-1.8.9"),
        legacy_forge: true,
    },
    VersionSpec {
        version: "1.20.1",
        loader: "Forge",
        label: "Moderne",
        java: 17,
        forge: Some("1.20.1-47.4.28"),
        legacy_forge: false,
    },
    VersionSpec {
        version: "1.21.11",
        loader: "Fabric",
        label: "Ultima release",
        java: 21,
        forge: None,
        legacy_forge: false,
    },
    VersionSpec {
        version: "26.2",
        loader: "Fabric",
        label: "Snapshot",
        java: 25,
        forge: None,
        legacy_forge: false,
    },
    VersionSpec {
        version: "26.3",
        loader: "Fabric",
        label: "Snapshot",
        java: 25,
        forge: None,
        legacy_forge: false,
    },
];

pub fn spec_of(version: &str) -> Option<&'static VersionSpec> {
    SUPPORTED.iter().find(|spec| spec.version == version)
}

/// Same data folder the Electron build used, so existing installs keep working.
pub fn data_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("ECLIPSE_HOME") {
        if !custom.trim().is_empty() {
            return PathBuf::from(custom);
        }
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "windows") {
        let base = std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| home.join("AppData/Roaming"));
        base.join("EclipseClient")
    } else if cfg!(target_os = "macos") {
        home.join("Library/Application Support/EclipseClient")
    } else {
        home.join(".local/share/EclipseClient")
    }
}

pub fn instance_dir(version: &str) -> PathBuf {
    data_dir().join("instances").join(version)
}

fn marker_file(version: &str) -> PathBuf {
    instance_dir(version).join("installed.json")
}

fn read_marker(version: &str) -> Option<Value> {
    let text = std::fs::read_to_string(marker_file(version)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_marker(version: &str, version_id: &str, loader: &str) {
    let body = json!({
        "version": version,
        "versionId": version_id,
        "loader": loader,
        "installedAt": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        "mods": list_mod_names(version)
    });
    if let Ok(text) = serde_json::to_string_pretty(&body) {
        let _ = util::mkdirp(&instance_dir(version));
        let _ = std::fs::write(marker_file(version), text);
    }
}

fn installed_version_id(version: &str) -> Option<String> {
    let marker = read_marker(version)?;
    let id = marker.get("versionId")?.as_str()?.to_string();
    let file = data_dir()
        .join("versions")
        .join(&id)
        .join(format!("{}.json", id));
    if util::exists(&file) {
        Some(id)
    } else {
        None
    }
}

fn list_mod_names(version: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mods_dir = instance_dir(version).join("mods");
    if let Ok(entries) = std::fs::read_dir(&mods_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.to_ascii_lowercase().ends_with(".jar") {
                names.push(name);
            }
        }
    }
    names.sort();
    names
}

/// Resource folder Tauri reports for the packaged app (filled at startup).
static RESOURCE_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn set_resource_dir(dir: PathBuf) {
    let _ = RESOURCE_DIR.set(dir);
}

/// Where the jars for `version` can live, most specific first: Tauri's resource folder,
/// next to the binary (portable/AppImage), the `/usr/lib/<app>` layout of the .deb, and the
/// project folder for development runs.
pub fn bundled_mods_candidates(
    version: &str,
    exe_dir: &std::path::Path,
    resource_dir: Option<&std::path::Path>,
) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(root) = resource_dir {
        dirs.push(root.join("mods").join(version));
        dirs.push(root.join(version));
    }
    dirs.push(exe_dir.join("mods").join(version));
    dirs.push(exe_dir.join("resources").join("mods").join(version));
    // .deb/AppImage: the binary sits in usr/bin, resources in usr/lib/<app>
    if let Some(prefix) = exe_dir.parent() {
        for name in ["Eclipse Client", "eclipse-client"] {
            dirs.push(prefix.join("lib").join(name).join("mods").join(version));
        }
    }
    if let Some(appdir) = std::env::var_os("APPDIR") {
        for name in ["Eclipse Client", "eclipse-client"] {
            dirs.push(PathBuf::from(&appdir).join("usr/lib").join(name).join("mods").join(version));
        }
    }
    if let Some(up) = exe_dir.parent() {
        dirs.push(up.join("mods").join(version));
        dirs.push(up.join("resources").join("mods").join(version));
        if let Some(up2) = up.parent() {
            dirs.push(up2.join("mods").join(version));
        }
    }
    dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("mods").join(version));
    dirs
}

/// Folder with the jars the client ships for a version (resources of the packaged app).
pub fn bundled_mods_dir(version: &str) -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("ECLIPSE_MODS_DIR") {
        let dir = PathBuf::from(custom).join(version);
        if dir.is_dir() {
            return Some(dir);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    bundled_mods_candidates(version, exe_dir, RESOURCE_DIR.get().map(|dir| dir.as_path()))
        .into_iter()
        .find(|dir| dir.is_dir())
}

/// Jars the launcher owns inside an instance: a stale one left by an earlier build would be
/// loaded next to the current jar (same mod id twice, or a jar built for another version).
fn is_managed_jar(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".jar")
        && (lower.starts_with("crosshairclient")
            || lower.starts_with("fabric-api")
            || lower.starts_with("lithium")
            || lower.starts_with("sodium")
            || lower.starts_with("purescale")
            || lower.starts_with("ferritecore")
            || lower.starts_with("immediatelyfast"))
}

/// Copies the client's jars into `target`, keeps the ones that are already right and removes
/// managed jars that are no longer shipped (renamed or replaced by another version).
pub fn sync_bundled_mods(source: &std::path::Path, target: &std::path::Path) -> Vec<String> {
    if util::mkdirp(target).is_err() {
        return Vec::new();
    }
    let mut shipped: Vec<String> = Vec::new();
    let mut copied = Vec::new();
    if let Ok(entries) = std::fs::read_dir(source) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !is_managed_jar(&name) {
                continue;
            }
            shipped.push(name.clone());
            let from = source.join(&name);
            let to = target.join(&name);
            if util::exists(&to) && util::size_of(&to) == util::size_of(&from) {
                continue;
            }
            if std::fs::copy(&from, &to).is_ok() {
                copied.push(name);
            }
        }
    }
    if !shipped.is_empty() {
        if let Ok(entries) = std::fs::read_dir(target) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !is_managed_jar(&name) {
                    continue;
                }
                if !shipped.iter().any(|kept| kept.eq_ignore_ascii_case(&name)) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    copied
}

/// Copies the client's jars into the instance, skipping files that are already there.
pub fn install_bundled_mods(version: &str) -> Vec<String> {
    let Some(source) = bundled_mods_dir(version) else {
        return Vec::new();
    };
    sync_bundled_mods(&source, &instance_dir(version).join("mods"))
}

fn status_for(spec: &VersionSpec) -> Value {
    let java = java::find_java(spec.java, None);
    let installed_id = installed_version_id(spec.version);
    json!({
        "version": spec.version,
        "loader": spec.loader,
        "label": spec.label,
        "java": spec.java,
        "installed": installed_id.is_some(),
        "versionId": installed_id,
        "javaPath": java.path.as_ref().map(|path| path.to_string_lossy().to_string()),
        "instanceDir": instance_dir(spec.version).to_string_lossy().to_string(),
        "mods": if installed_version_id(spec.version).is_some() { json!(list_mod_names(spec.version)) } else { json!([]) }
    })
}

pub fn status() -> Value {
    let versions: Vec<Value> = SUPPORTED.iter().map(status_for).collect();
    json!({
        "dataDir": data_dir().to_string_lossy().to_string(),
        "versions": versions,
        "offline": true
    })
}

/// Full install: vanilla, loader (Fabric or Forge), libraries, assets and the client mod.
pub async fn install(version: &str, progress: Progress) -> Result<Value, String> {
    let spec = spec_of(version).ok_or_else(|| format!("Unsupported version: {}", version))?;
    util::mkdirp(&data_dir()).map_err(|e| e.to_string())?;
    let instance = instance_dir(version);
    util::mkdirp(&instance).map_err(|e| e.to_string())?;

    progress(&format!("Downloading Minecraft {}", version));
    let (vanilla, _client_jar) = minecraft::vanilla_version(&data_dir(), version, &progress).await?;

    let version_id = if spec.loader == "Fabric" {
        loaders::install_fabric(&data_dir(), version, &progress).await?.id
    } else if spec.legacy_forge {
        let forge = spec.forge.ok_or("Forge version missing")?;
        loaders::install_forge_legacy(&data_dir(), forge, &progress).await?.id
    } else {
        let forge = spec.forge.ok_or("Forge version missing")?;
        let java = java::find_java(spec.java, None);
        let java_path = java.path.ok_or_else(|| {
            format!(
                "Java {} is required for Forge {}. Install it or set the Java path in Settings.",
                spec.java, version
            )
        })?;
        loaders::install_forge(&data_dir(), version, forge, &java_path, &progress)
            .await?
            .id
    };

    progress("Downloading libraries");
    let chain = minecraft::chain_for(&data_dir(), &version_id)?;
    let natives = data_dir().join("natives").join(&version_id);
    minecraft::install_libraries(&data_dir(), &chain, &natives, &progress).await?;

    progress("Downloading assets");
    minecraft::install_assets(&data_dir(), &vanilla, &progress).await?;

    progress("Installing Eclipse Client");
    let copied = install_bundled_mods(version);
    let _ = util::mkdirp(&instance.join("saves"));
    write_marker(version, &version_id, spec.loader);
    progress("Ready");

    Ok(json!({
        "version": version,
        "versionId": version_id,
        "mods": list_mod_names(version),
        "copied": copied
    }))
}

/// Launch options coming from the UI.
pub struct LaunchInput {
    pub username: String,
    pub ram_mb: u64,
    pub java_path: String,
}

/// Starts Minecraft, installing the version first when needed.
pub async fn launch_version(
    version: &str,
    input: LaunchInput,
    progress: Progress,
) -> Result<Value, String> {
    let spec = spec_of(version).ok_or_else(|| format!("Unsupported version: {}", version))?;
    let username: String = {
        let trimmed = input.username.trim();
        let value = if trimmed.is_empty() { "Player" } else { trimmed };
        value.chars().take(16).collect()
    };
    let ram_mb = input.ram_mb.clamp(1024, 16384);

    if installed_version_id(version).is_none() {
        install(version, progress.clone()).await?;
    }

    let java = java::find_java(spec.java, Some(&input.java_path));
    let java_path = java.path.ok_or_else(|| {
        format!(
            "Java {} not found. Install it or set the Java path in Settings.",
            spec.java
        )
    })?;

    let version_id = installed_version_id(version).ok_or_else(|| format!("Version {} is not installed", version))?;
    install_bundled_mods(version);

    let (pid, log) = launch::launch(
        launch::LaunchOptions {
            data_dir: data_dir(),
            instance_dir: instance_dir(version),
            version_id,
            mc_version: version.to_string(),
            username,
            ram_mb,
            java_path,
        },
        progress,
    )
    .await?;

    Ok(json!({
        "ok": true,
        "pid": pid,
        "log": log.to_string_lossy().to_string()
    }))
}

pub fn mods_for(version: &str) -> Vec<Value> {
    let bundled = bundled_mods_dir(version);
    mods::get_mods(&instance_dir(version), bundled.as_deref())
}

#[cfg(test)]
mod tests {
    use super::bundled_mods_candidates;
    use std::fs;
    use std::path::Path;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("eclipse-mods-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The .deb installs the binary in usr/bin and the resources in usr/lib/<product name>.
    #[test]
    fn finds_mods_in_deb_layout() {
        let root = temp_root("deb");
        let bin = root.join("usr/bin");
        let mods = root.join("usr/lib/Eclipse Client/mods/1.8.9");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&mods).unwrap();
        fs::write(mods.join("crosshairclient-1.8.9.jar"), b"jar").unwrap();

        let found = bundled_mods_candidates("1.8.9", &bin, None)
            .into_iter()
            .find(|dir| dir.is_dir())
            .expect("deb layout must be found");
        assert_eq!(found, mods);
        let _ = fs::remove_dir_all(&root);
    }

    /// A binary shipped with a mods folder beside it (portable / AppImage payload).
    #[test]
    fn finds_mods_next_to_binary() {
        let root = temp_root("portable");
        let bin = root.join("app");
        let mods = bin.join("mods/1.20.1");
        fs::create_dir_all(&mods).unwrap();

        let found = bundled_mods_candidates("1.20.1", &bin, None)
            .into_iter()
            .find(|dir| dir.is_dir())
            .expect("portable layout must be found");
        assert_eq!(found, mods);
        let _ = fs::remove_dir_all(&root);
    }

    /// Tauri's own resource dir wins when the bundler puts the jars there.
    #[test]
    fn resource_dir_takes_priority() {
        let root = temp_root("resource");
        let bin = root.join("usr/bin");
        let resource_mods = root.join("usr/lib/eclipse-client/mods/1.21.11");
        let beside_mods = bin.join("mods/1.21.11");
        fs::create_dir_all(&resource_mods).unwrap();
        fs::create_dir_all(&beside_mods).unwrap();

        let candidate = bundled_mods_candidates("1.21.11", &bin, Some(Path::new(&root.join("usr/lib/eclipse-client"))))
            .into_iter()
            .find(|dir| dir.is_dir())
            .expect("resource dir must be found");
        assert_eq!(candidate, resource_mods);
        let _ = fs::remove_dir_all(&root);
    }

    /// A clean instance gets the shipped jars; an old managed jar is replaced, a renamed one is
    /// dropped (two client jars = duplicate mod id at launch), user jars are left alone.
    #[test]
    fn sync_installs_and_cleans_bundled_jars() {
        use super::sync_bundled_mods;
        let root = temp_root("sync");
        let source = root.join("bundle");
        let target = root.join("instance/mods");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(source.join("crosshairclient-26.2.jar"), b"new client").unwrap();
        fs::write(source.join("fabric-api-0.161.0+26.2.jar"), b"new api").unwrap();
        fs::write(target.join("crosshairclient-26.2.jar"), b"old").unwrap();
        fs::write(target.join("crosshairclient-26.3.jar"), b"wrong version").unwrap();
        fs::write(target.join("sodium-1.0.jar"), b"user mod").unwrap();

        let copied = sync_bundled_mods(&source, &target);

        assert_eq!(copied.len(), 2);
        assert_eq!(fs::read(target.join("crosshairclient-26.2.jar")).unwrap(), b"new client");
        assert_eq!(fs::read(target.join("fabric-api-0.161.0+26.2.jar")).unwrap(), b"new api");
        assert!(!target.join("crosshairclient-26.3.jar").exists(), "stale client jar must go");
        assert!(target.join("sodium-1.0.jar").exists(), "user jars must survive");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_version_yields_nothing() {
        let root = temp_root("empty");
        let bin = root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let found = bundled_mods_candidates("9.9.9", &bin, None)
            .into_iter()
            .find(|dir| dir.is_dir());
        assert!(found.is_none());
        let _ = fs::remove_dir_all(&root);
    }
}
