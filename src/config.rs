use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::capture::CaptureTarget;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_hotkey")]
    pub capture_hotkey: String,
    #[serde(default)]
    pub default_target: CaptureTarget,
    #[serde(default)]
    pub default_delay_ms: u64,
    #[serde(default = "default_copy_to_clipboard")]
    pub copy_to_clipboard: bool,
    #[serde(default = "default_save_dir")]
    pub save_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub path: PathBuf,
    pub data: AppConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            capture_hotkey: default_hotkey(),
            default_target: CaptureTarget::default(),
            default_delay_ms: 0,
            copy_to_clipboard: default_copy_to_clipboard(),
            save_dir: default_save_dir(),
        }
    }
}

pub fn load_or_create_config() -> Result<LoadedConfig> {
    let path = config_path()?;
    let data = if path.exists() {
        let contents =
            fs::read_to_string(&path).with_context(|| format!("read config {}", path.display()))?;
        toml::from_str::<AppConfig>(&contents)
            .with_context(|| format!("parse config {}", path.display()))?
    } else {
        let default = AppConfig::default();
        write_config(&path, &default)?;
        default
    };

    Ok(LoadedConfig { path, data })
}

fn write_config(path: &PathBuf, data: &AppConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create config dir {}", parent.display()))?;
    }

    let serialized = toml::to_string_pretty(data).context("serialize config")?;
    fs::write(path, serialized).with_context(|| format!("write config {}", path.display()))?;
    Ok(())
}

fn config_path() -> Result<PathBuf> {
    if let Some(override_dir) = env::var_os("PYRO_CONFIG_DIR") {
        return Ok(PathBuf::from(override_dir).join("config.toml"));
    }

    let base = dirs::config_dir().context("resolve config directory")?;
    Ok(base.join("pyro").join("config.toml"))
}

fn default_hotkey() -> String {
    "Alt+Shift+S".to_string()
}

fn default_copy_to_clipboard() -> bool {
    true
}

fn default_save_dir() -> PathBuf {
    dirs::picture_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Pyro")
}
