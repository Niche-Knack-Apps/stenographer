use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StenoConfig {
    pub data_dir: PathBuf,
    pub retention_days: u32,
    pub batch_flush_ms: u64,
    pub batch_max_events: usize,
    pub session_check_interval_secs: u64,
    pub grouper_interval_secs: u64,
    pub gc_interval_secs: u64,
    pub git_cache_ttl_secs: u64,
    pub private_mode: bool,
    pub history_action: String,
    /// Max commands loaded into the history picker. 0 or any negative value
    /// means unlimited (bounded only by retention_days). CLI `--limit` overrides.
    pub history_limit: i64,
}

impl Default for StenoConfig {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            retention_days: 90,
            batch_flush_ms: 100,
            batch_max_events: 64,
            session_check_interval_secs: 60,
            grouper_interval_secs: 300,
            gc_interval_secs: 86400,
            git_cache_ttl_secs: 30,
            private_mode: false,
            history_action: "print".to_string(),
            history_limit: 500,
        }
    }
}

pub fn default_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("com.niche-knack.stenographer")
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("stenographer")
        .join("config.toml")
}

pub fn tasks_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("stenographer")
        .join("tasks.toml")
}

pub fn runtime_dir() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::state_dir().unwrap_or_else(|| PathBuf::from("/tmp")))
        .join("stenographer")
}

pub fn spool_path() -> PathBuf {
    runtime_dir().join("spool.ndjson")
}

pub fn socket_path() -> PathBuf {
    runtime_dir().join("steno.sock")
}

pub fn pid_path() -> PathBuf {
    runtime_dir().join("steno.pid")
}

pub fn db_path(config: &StenoConfig) -> PathBuf {
    config.data_dir.join("db").join("steno.db")
}

pub fn log_dir(config: &StenoConfig) -> PathBuf {
    config.data_dir.join("logs")
}

pub fn load_config() -> StenoConfig {
    let path = config_path();
    load_config_from(&path)
}

pub fn load_config_from(path: &Path) -> StenoConfig {
    match std::fs::read_to_string(path) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(config) => config,
            Err(e) => {
                tracing::warn!(
                    "Failed to parse config {}: {}, using defaults",
                    path.display(),
                    e
                );
                StenoConfig::default()
            }
        },
        Err(_) => StenoConfig::default(),
    }
}
