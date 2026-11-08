//! Payloads the web UI sends to the Tauri commands (same shapes the Electron IPC used).

use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, Default, Debug)]
pub struct LaunchOptionsIn {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default, rename = "ramMb")]
    pub ram_mb: Option<u64>,
    #[serde(default, rename = "javaPath")]
    pub java_path: Option<String>,
}

#[derive(Deserialize, Default, Debug)]
pub struct SearchInput {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub loader: String,
    #[serde(default, rename = "apiKey")]
    pub api_key: String,
}

#[derive(Deserialize, Default, Debug)]
pub struct DownloadInput {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub id: Value,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub loader: String,
    #[serde(default, rename = "apiKey")]
    pub api_key: String,
}
