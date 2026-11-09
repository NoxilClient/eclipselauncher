// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod engine;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

/// Un solo download alla volta: l'updater d'avvio e il pulsante non devono sovrapporsi.
static UPDATE_RUNNING: AtomicBool = AtomicBool::new(false);

struct UpdateGuard;

impl Drop for UpdateGuard {
    fn drop(&mut self) {
        UPDATE_RUNNING.store(false, Ordering::SeqCst);
    }
}

fn emit_update_progress(app: &AppHandle, downloaded: u64, total: Option<u64>) {
    let percent = total
        .filter(|total| *total > 0)
        .map(|total| ((downloaded.min(total) * 100) / total) as u32);
    let _ = app.emit(
        "update-progress",
        json!({ "downloaded": downloaded, "total": total, "percent": percent }),
    );
}

/// Intervallo minimo tra due controlli automatici (come fa Basalt).
const MIN_CHECK_INTERVAL_SECS: i64 = 4 * 60 * 60;

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct UpdateState {
    #[serde(default)]
    last_checked_at: i64,
    #[serde(default)]
    latest_version: Option<String>,
    #[serde(default)]
    available: bool,
    #[serde(default)]
    dismissed_version: Option<String>,
}

fn state_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("update_state.json"))
}

fn read_state(app: &AppHandle) -> UpdateState {
    state_path(app)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<UpdateState>(&text).ok())
        .unwrap_or_default()
}

fn write_state(app: &AppHandle, state: &UpdateState) {
    let Some(path) = state_path(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(path, text);
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// Chi possiede questa installazione: solo una copia "self" puo' aggiornarsi da sola.
#[cfg(target_os = "linux")]
fn install_policy() -> (&'static str, &'static str) {
    const SELF_HINT: &str = "Il launcher scarica e installa aggiornamenti firmati.";
    const PACKAGE_HINT: &str =
        "Questa copia e' gestita dal sistema: aggiorna con il gestore pacchetti o scarica la nuova AppImage.";
    if std::env::var_os("APPIMAGE").is_some() {
        return ("self", SELF_HINT);
    }
    if let Ok(exe) = std::env::current_exe() {
        let path = exe.to_string_lossy();
        if path.starts_with("/usr/") || path.starts_with("/opt/") || path.starts_with("/nix/store") {
            return ("package", PACKAGE_HINT);
        }
    }
    ("self", SELF_HINT)
}

#[cfg(not(target_os = "linux"))]
fn install_policy() -> (&'static str, &'static str) {
    ("self", "Il launcher scarica e installa aggiornamenti firmati.")
}

/// Solo controllo: nessun download. `force` ignora l'intervallo minimo.
async fn divine_find_update(app: &AppHandle, force: bool) -> Value {
    use tauri_plugin_updater::UpdaterExt;

    let (policy, hint) = install_policy();
    let mut state = read_state(app);

    // entro l'intervallo non si tocca la rete: si riusa l'esito salvato su disco
    if !force && state.last_checked_at > 0 {
        let since = now_secs() - state.last_checked_at;
        if (0..MIN_CHECK_INTERVAL_SECS).contains(&since) {
            let dismissed = state.available
                && state.latest_version.is_some()
                && state.dismissed_version == state.latest_version;
            return json!({
                "ok": true,
                "cached": true,
                "available": state.available,
                "version": state.latest_version,
                "dismissed": dismissed,
                "policy": policy,
                "hint": hint
            });
        }
    }

    let updater = match app.updater() {
        Ok(updater) => updater,
        Err(err) => return json!({ "ok": false, "error": err.to_string() }),
    };

    match tokio::time::timeout(std::time::Duration::from_secs(20), updater.check()).await {
        Ok(Ok(Some(update))) => {
            let dismissed = state.dismissed_version.as_deref() == Some(update.version.as_str());
            state.last_checked_at = now_secs();
            state.available = true;
            state.latest_version = Some(update.version.clone());
            write_state(app, &state);
            json!({
                "ok": true,
                "available": true,
                "version": update.version,
                "dismissed": dismissed,
                "policy": policy,
                "hint": hint
            })
        }
        Ok(Ok(None)) => {
            state.last_checked_at = now_secs();
            state.available = false;
            state.latest_version = None;
            write_state(app, &state);
            json!({ "ok": true, "available": false, "policy": policy, "hint": hint })
        }
        Ok(Err(err)) => json!({ "ok": false, "error": err.to_string() }),
        Err(_) => json!({ "ok": false, "error": "timeout" }),
    }
}

#[tauri::command]
async fn divine_check_update_status(app: AppHandle, force: Option<bool>) -> Value {
    divine_find_update(&app, force.unwrap_or(false)).await
}

/// "Non ora": questa versione non viene piu' proposta finche' non ne esce una nuova.
#[tauri::command]
fn divine_dismiss_update(app: AppHandle, version: Option<String>) -> Value {
    let mut state = read_state(&app);
    state.dismissed_version = version.or_else(|| state.latest_version.clone());
    write_state(&app, &state);
    json!({ "ok": true })
}

async fn divine_update(app: AppHandle) -> Value {
    use tauri_plugin_updater::UpdaterExt;

    if UPDATE_RUNNING.swap(true, Ordering::SeqCst) {
        return json!({ "ok": true, "updated": false, "busy": true });
    }
    let _guard = UpdateGuard;

    // su una copia gestita dal sistema il self-update non deve nemmeno partire
    let (policy, hint) = install_policy();
    if policy != "self" {
        return json!({ "ok": false, "error": hint, "policy": policy });
    }

    // un nuovo tentativo rimette in gioco l'avviso se l'installazione non riesce
    let mut state = read_state(&app);
    state.dismissed_version = None;
    write_state(&app, &state);

    let updater = match app.updater() {
        Ok(updater) => updater,
        Err(err) => {
            let message = format!("Updater non disponibile: {err}");
            let _ = app.emit("update-error", json!({ "message": message }));
            return json!({ "ok": false, "error": message });
        }
    };

    let update = match updater.check().await {
        Ok(Some(update)) => update,
        Ok(None) => return json!({ "ok": true, "updated": false }),
        Err(err) => {
            let message = format!("Controllo aggiornamenti fallito: {err}");
            let _ = app.emit("update-error", json!({ "message": message }));
            return json!({ "ok": false, "error": message });
        }
    };

    let version = update.version.clone();
    let _ = app.emit("update-start", json!({ "version": version }));

    let downloaded = Arc::new(AtomicU64::new(0));
    let total_seen = Arc::new(AtomicU64::new(0));
    let counter = downloaded.clone();
    let total_state = total_seen.clone();
    let progress_app = app.clone();

    // il plugin passa solo la dimensione del singolo pezzo: il progresso va accumulato qui,
    // altrimenti il rapporto pezzo/totale resta a 0% per tutto il download.
    let download = update.download_and_install(
        move |chunk: usize, total: Option<u64>| {
            let done = counter.fetch_add(chunk as u64, Ordering::SeqCst) + chunk as u64;
            if let Some(total) = total.filter(|total| *total > 0) {
                total_state.store(total, Ordering::SeqCst);
            }
            let known_total = match total_state.load(Ordering::SeqCst) {
                0 => None,
                value => Some(value),
            };
            emit_update_progress(&progress_app, done, known_total);
        },
        || {},
    );

    // senza timeout un download bloccato lascia l'overlay a 0% per sempre
    let result = tokio::time::timeout(std::time::Duration::from_secs(900), download).await;

    match result {
        Ok(Ok(())) => {
            // dopo l'installazione il prossimo avvio deve ricontrollare subito
            if let Some(path) = state_path(&app) {
                let _ = std::fs::remove_file(path);
            }
            let _ = app.emit("update-done", json!({ "version": version }));
            let restarter = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
                restarter.restart();
            });
            json!({ "ok": true, "updated": true, "version": version })
        }
        Ok(Err(err)) => {
            let message = format!("Download aggiornamento non riuscito: {err}");
            let _ = app.emit("update-error", json!({ "message": message }));
            json!({ "ok": false, "error": message })
        }
        Err(_) => {
            let message = "Download aggiornamento interrotto dopo 15 minuti: rete lenta o GitHub non raggiungibile".to_string();
            let _ = app.emit("update-error", json!({ "message": message }));
            json!({ "ok": false, "error": message })
        }
    }
}

#[tauri::command]
async fn divine_check_updates(app: AppHandle) -> Value {
    divine_update(app).await
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
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
            divine_check_updates,
            divine_check_update_status,
            divine_dismiss_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running DivineClient");
}
