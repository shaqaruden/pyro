use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::capture::CaptureTarget;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_hotkey")]
    pub capture_hotkey: String,
    #[serde(default = "default_capture_target")]
    pub default_target: CaptureTarget,
    #[serde(default)]
    pub default_delay_ms: u64,
    #[serde(default = "default_copy_to_clipboard")]
    pub copy_to_clipboard: bool,
    #[serde(default = "default_open_editor")]
    pub open_editor: bool,
    #[serde(default = "default_save_dir")]
    pub save_dir: PathBuf,
    #[serde(default)]
    pub editor: EditorConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorConfig {
    #[serde(default)]
    pub shortcuts: EditorShortcutConfig,
    #[serde(default = "default_text_commit_feedback_color")]
    pub text_commit_feedback_color: String,
    #[serde(default)]
    pub radial_menu_animation_speed: RadialMenuAnimationSpeed,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RadialMenuAnimationSpeed {
    Instant,
    Fast,
    #[default]
    Normal,
    Slow,
    Slower,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorShortcutConfig {
    #[serde(default = "default_shortcut_select")]
    pub select: String,
    #[serde(default = "default_shortcut_rectangle")]
    pub rectangle: String,
    #[serde(default = "default_shortcut_ellipse")]
    pub ellipse: String,
    #[serde(default = "default_shortcut_line")]
    pub line: String,
    #[serde(default = "default_shortcut_arrow")]
    pub arrow: String,
    #[serde(default = "default_shortcut_marker")]
    pub marker: String,
    #[serde(default = "default_shortcut_text")]
    pub text: String,
    #[serde(default = "default_shortcut_pixelate")]
    pub pixelate: String,
    #[serde(default = "default_shortcut_blur")]
    pub blur: String,
    #[serde(default = "default_shortcut_copy")]
    pub copy: String,
    #[serde(default = "default_shortcut_save")]
    pub save: String,
    #[serde(default = "default_shortcut_copy_save")]
    pub copy_and_save: String,
    #[serde(default = "default_shortcut_undo")]
    pub undo: String,
    #[serde(default = "default_shortcut_redo")]
    pub redo: String,
    #[serde(default = "default_shortcut_delete")]
    pub delete_selected: String,
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
            default_target: default_capture_target(),
            default_delay_ms: 0,
            copy_to_clipboard: default_copy_to_clipboard(),
            open_editor: default_open_editor(),
            save_dir: default_save_dir(),
            editor: EditorConfig::default(),
        }
    }
}

impl Default for EditorShortcutConfig {
    fn default() -> Self {
        Self {
            select: default_shortcut_select(),
            rectangle: default_shortcut_rectangle(),
            ellipse: default_shortcut_ellipse(),
            line: default_shortcut_line(),
            arrow: default_shortcut_arrow(),
            marker: default_shortcut_marker(),
            text: default_shortcut_text(),
            pixelate: default_shortcut_pixelate(),
            blur: default_shortcut_blur(),
            copy: default_shortcut_copy(),
            save: default_shortcut_save(),
            copy_and_save: default_shortcut_copy_save(),
            undo: default_shortcut_undo(),
            redo: default_shortcut_redo(),
            delete_selected: default_shortcut_delete(),
        }
    }
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            shortcuts: EditorShortcutConfig::default(),
            text_commit_feedback_color: default_text_commit_feedback_color(),
            radial_menu_animation_speed: RadialMenuAnimationSpeed::default(),
        }
    }
}

impl RadialMenuAnimationSpeed {
    pub const fn duration_ms(self) -> u32 {
        match self {
            Self::Instant => 0,
            Self::Fast => 150,
            Self::Normal => 250,
            Self::Slow => 350,
            Self::Slower => 500,
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

pub fn load_config_from_path(path: &Path) -> Result<AppConfig> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("read config {}", path.display()))?;
    toml::from_str::<AppConfig>(&contents)
        .with_context(|| format!("parse config {}", path.display()))
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
    "PrintScreen".to_string()
}

fn default_capture_target() -> CaptureTarget {
    CaptureTarget::Region
}

fn default_copy_to_clipboard() -> bool {
    true
}

fn default_open_editor() -> bool {
    true
}

fn default_save_dir() -> PathBuf {
    dirs::picture_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Pyro")
}

fn default_shortcut_select() -> String {
    "S".to_string()
}

fn default_shortcut_rectangle() -> String {
    "R".to_string()
}

fn default_shortcut_ellipse() -> String {
    "E".to_string()
}

fn default_shortcut_line() -> String {
    "L".to_string()
}

fn default_shortcut_arrow() -> String {
    "A".to_string()
}

fn default_shortcut_marker() -> String {
    "M".to_string()
}

fn default_shortcut_text() -> String {
    "T".to_string()
}

fn default_shortcut_pixelate() -> String {
    "P".to_string()
}

fn default_shortcut_blur() -> String {
    "B".to_string()
}

fn default_shortcut_copy() -> String {
    "Ctrl+C".to_string()
}

fn default_shortcut_save() -> String {
    "Ctrl+S".to_string()
}

fn default_shortcut_copy_save() -> String {
    "Ctrl+Shift+S".to_string()
}

fn default_shortcut_undo() -> String {
    "Ctrl+Z".to_string()
}

fn default_shortcut_redo() -> String {
    "Ctrl+Y".to_string()
}

fn default_shortcut_delete() -> String {
    "Delete".to_string()
}

fn default_text_commit_feedback_color() -> String {
    "#48B4FF".to_string()
}
