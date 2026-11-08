// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod engine;

use std::sync::Arc;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use engine::client;
use engine::ipc::{DownloadInput, LaunchOptionsIn, SearchInput};
use engine::mods;
use engine::util::Progress;

fn progress_for(app: &AppHandle, version: &str) -> Progress {
    let app = app.clone();
    let version = version.to_string();
    Arc::new(move |message: &str| {
        let _ = app.emit(
            "eclipse-progress",
            json!({ "version": version, "message": message }),
        );
    })
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[tauri::command]
async fn eclipse_status() -> Value {
    client::status()
}

#[tauri::command]
async fn eclipse_install(app: AppHandle, version: String) -> Value {
    let progress = progress_for(&app, &version);
    match client::install(&version, progress).await {
        Ok(mut result) => {
            result["ok"] = json!(true);
            result
        }
        Err(err) => json!({ "ok": false, "error": err }),
    }
}

#[tauri::command]
async fn eclipse_launch(app: AppHandle, version: String, options: Option<LaunchOptionsIn>) -> Value {
    let raw = options.unwrap_or_default();
    let input = client::LaunchInput {
        username: raw.username.unwrap_or_else(|| "Player".to_string()),
        ram_mb: raw.ram_mb.unwrap_or(4096),
        java_path: raw.java_path.unwrap_or_default(),
    };
    let progress = progress_for(&app, &version);
    match client::launch_version(&version, input, progress).await {
        Ok(value) => value,
        Err(err) => json!({ "ok": false, "error": err }),
    }
}

#[tauri::command]
async fn eclipse_get_mods(version: String) -> Value {
    json!({ "ok": true, "mods": client::mods_for(&version) })
}

#[tauri::command]
async fn eclipse_remove_mod(version: String, name: String) -> Value {
    let instance = client::instance_dir(&version);
    match mods::remove_mod(&instance, &name) {
        Ok(removed) => json!({ "ok": true, "removed": removed, "name": name }),
        Err(err) => json!({ "ok": false, "error": err }),
    }
}

#[tauri::command]
async fn eclipse_search_mods(opts: Option<SearchInput>) -> Value {
    let opts = opts.unwrap_or_default();
    match mods::search(&opts.source, &opts.query, &opts.version, &opts.loader, &opts.api_key).await {
        Ok(results) => {
            let instance = client::instance_dir(&opts.version);
            let installed = mods::installed_projects(&instance);
            let files: Vec<String> = client::mods_for(&opts.version)
                .iter()
                .filter_map(|item| item.get("name").and_then(|v| v.as_str()).map(normalize))
                .collect();
            let results: Vec<Value> = results
                .into_iter()
                .map(|mut item| {
                    let id = item
                        .get("id")
                        .map(|value| value.to_string().trim_matches('"').to_string())
                        .unwrap_or_default();
                    let source = item
                        .get("source")
                        .and_then(|v| v.as_str())
                        .unwrap_or("modrinth")
                        .to_string();
                    let by_id = installed.iter().any(|entry| *entry == format!("{}:{}", source, id));
                    let slug = normalize(item.get("slug").and_then(|v| v.as_str()).unwrap_or(""));
                    let by_file = slug.len() > 2 && files.iter().any(|file| file.starts_with(&slug));
                    item["installed"] = json!(by_id || by_file);
                    item
                })
                .collect();
            json!({ "ok": true, "results": results })
        }
        Err(err) => json!({ "ok": false, "error": err, "results": [] }),
    }
}

#[tauri::command]
async fn eclipse_download_mod(app: AppHandle, opts: Option<DownloadInput>) -> Value {
    let opts = opts.unwrap_or_default();
    let instance = client::instance_dir(&opts.version);
    let progress = progress_for(&app, &opts.version);
    match mods::download(
        &instance,
        &opts.source,
        &opts.id,
        &opts.version,
        &opts.loader,
        &opts.api_key,
        &progress,
    )
    .await
    {
        Ok((downloaded, skipped)) => json!({ "ok": true, "downloaded": downloaded, "skipped": skipped }),
        Err(err) => json!({ "ok": false, "error": err }),
    }
}

#[tauri::command]
async fn divine_check_updates(app: AppHandle) -> Value {
    use tauri_plugin_updater::UpdaterExt;
    let updater = match app.updater() {
        Ok(u) => u,
        Err(err) => return json!({ "ok": false, "error": err.to_string() }),
    };
    let update = match updater.check().await {
        Ok(Some(update)) => update,
        Ok(None) => return json!({ "ok": true, "updated": false }),
        Err(err) => return json!({ "ok": false, "error": err.to_string() }),
    };
    let version = update.version.clone();
    let app2 = app.clone();
    match update
        .download_and_install(
            move |chunk, total| {
                let _ = app2.emit("update-progress", (chunk, total));
            },
            || {},
        )
        .await
    {
        Ok(()) => json!({ "ok": true, "updated": true, "version": version }),
        Err(err) => json!({ "ok": false, "error": err.to_string() }),
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                use tauri_plugin_updater::UpdaterExt;
                let updater = match app_handle.updater() { Ok(u) => u, Err(_) => return };
                let update = match updater.check().await { Ok(Some(u)) => u, _ => return };
                let _ = app_handle.emit("update-start", update.version.clone());
                let app2 = app_handle.clone();
                let _ = update.download_and_install(move |chunk, total| {
                    let _ = app2.emit("update-progress", (chunk, total));
                }, || {}).await;
                let _ = app_handle.emit("update-done", ());
                std::process::exit(0);
            });
            // where the packaged app keeps its resources (the client's mods live there)
            if let Ok(dir) = app.path().resource_dir() {
                client::set_resource_dir(dir);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            eclipse_status,
            eclipse_install,
            eclipse_launch,
            eclipse_get_mods,
            eclipse_remove_mod,
            eclipse_search_mods,
            eclipse_download_mod,
            divine_check_updates
        ])
        .run(tauri::generate_context!())
        .expect("error while running DivineClient");
}
