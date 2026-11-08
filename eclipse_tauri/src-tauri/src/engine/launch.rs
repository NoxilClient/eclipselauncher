//! Builds the JVM command line and starts Minecraft detached.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::minecraft as mc;
use super::util::{self, Progress};

pub struct LaunchOptions {
    pub data_dir: PathBuf,
    pub instance_dir: PathBuf,
    pub version_id: String,
    pub mc_version: String,
    pub username: String,
    pub ram_mb: u64,
    pub java_path: PathBuf,
}

/// Offline UUID identical to Minecraft's `nameUUIDFromBytes("OfflinePlayer:" + name)`.
pub fn offline_uuid(name: &str) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(format!("OfflinePlayer:{}", name).as_bytes());
    let mut hash = hasher.finalize().to_vec();
    hash[6] = (hash[6] & 0x0f) | 0x30;
    hash[8] = (hash[8] & 0x3f) | 0x80;
    let hex: String = hash.iter().map(|byte| format!("{:02x}", byte)).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn tail(file: &Path, lines: usize) -> String {
    let Ok(text) = std::fs::read_to_string(file) else {
        return String::new();
    };
    let all: Vec<&str> = text.trim().lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// Prepares (if needed) and starts the game. The process is detached: the launcher can close.
pub async fn launch(options: LaunchOptions, progress: Progress) -> Result<(u32, PathBuf), String> {
    let data_dir = options.data_dir.clone();
    let instance_dir = options.instance_dir.clone();
    util::mkdirp(&instance_dir).map_err(|e| e.to_string())?;
    let natives_dir = data_dir.join("natives").join(&options.version_id);
    let chain = mc::chain_for(&data_dir, &options.version_id)?;
    let vanilla = chain.last().cloned().ok_or("empty version chain")?;

    progress("Checking libraries");
    let libraries = mc::install_libraries(&data_dir, &chain, &natives_dir, &progress).await?;
    progress("Checking assets");
    let (assets_dir, index_id) = mc::install_assets(&data_dir, &vanilla, &progress).await?;

    let jar = mc::client_jar(&data_dir, &chain);
    let mut classpath: Vec<String> = libraries
        .iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect();
    if jar.is_file() {
        classpath.push(jar.to_string_lossy().to_string());
    }
    let separator = if cfg!(target_os = "windows") { ";" } else { ":" };

    let mut vars: HashMap<String, String> = HashMap::new();
    let username = options.username.clone();
    vars.insert("auth_player_name".into(), username.clone());
    vars.insert("auth_uuid".into(), offline_uuid(&username));
    vars.insert("auth_access_token".into(), "0".into());
    vars.insert("auth_session".into(), "0".into());
    vars.insert("user_type".into(), "legacy".into());
    vars.insert("user_properties".into(), "{}".into());
    vars.insert("clientid".into(), String::new());
    vars.insert("auth_xuid".into(), String::new());
    vars.insert("version_name".into(), options.mc_version.clone());
    vars.insert("version_type".into(), "release".into());
    vars.insert("game_directory".into(), instance_dir.to_string_lossy().to_string());
    vars.insert("assets_root".into(), assets_dir.to_string_lossy().to_string());
    vars.insert("assets_index_name".into(), index_id);
    vars.insert(
        "game_assets".into(),
        assets_dir.join("virtual").join("legacy").to_string_lossy().to_string(),
    );
    vars.insert("natives_directory".into(), natives_dir.to_string_lossy().to_string());
    vars.insert("library_directory".into(), data_dir.join("libraries").to_string_lossy().to_string());
    vars.insert("classpath".into(), classpath.join(separator));
    vars.insert("classpath_separator".into(), separator.to_string());
    vars.insert("launcher_name".into(), "EclipseClient".into());
    vars.insert("launcher_version".into(), "0.3".into());

    let command = mc::build_command(&chain, &vars);
    if command.main_class.is_empty() {
        return Err(format!("Version {} has no main class", options.version_id));
    }

    let mut args: Vec<String> = vec![
        "-Xms512M".to_string(),
        format!("-Xmx{}M", options.ram_mb.max(1024)),
        "-Dminecraft.launcher.brand=EclipseClient".to_string(),
        "-Dminecraft.launcher.version=0.3".to_string(),
    ];
    args.extend(command.jvm);
    args.push(command.main_class);
    args.extend(command.game);

    let logs_dir = data_dir.join("logs");
    util::mkdirp(&logs_dir).map_err(|e| e.to_string())?;
    let log_file = logs_dir.join(format!("{}-latest.log", options.mc_version));
    let mut sink = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
        .map_err(|e| e.to_string())?;
    use std::io::Write;
    let _ = writeln!(sink, "\n=== Eclipse Client launch {} ===", chrono_like());
    let stdout = sink.try_clone().map_err(|e| e.to_string())?;
    let stderr = sink.try_clone().map_err(|e| e.to_string())?;

    let mut cmd = Command::new(&options.java_path);
    cmd.args(&args)
        .current_dir(&instance_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().map_err(|e| format!("Could not start Java: {}", e))?;
    let pid = child.id();

    tokio::time::sleep(std::time::Duration::from_millis(1800)).await;
    match child.try_wait() {
        Ok(Some(status)) if !status.success() => {
            Err(format!("Minecraft exited with code {:?}: {}", status.code(), tail(&log_file, 10)))
        }
        _ => Ok((pid, log_file)),
    }
}

fn chrono_like() -> String {
    // seconds since the epoch is enough for the log separator
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("t={}", now)
}
