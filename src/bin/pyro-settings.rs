#[cfg(target_os = "windows")]
mod windows_app {
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result};
    use serde::{Deserialize, Serialize};
    use slint::{ComponentHandle, SharedString};

    slint::slint! {
        import {
            Button,
            CheckBox,
            ComboBox,
            GroupBox,
            HorizontalBox,
            LineEdit,
            ScrollView,
            Slider,
            VerticalBox
        } from "std-widgets.slint";

        export component SettingsWindow inherits Window {
            title: "Pyro Settings";
            width: 980px;
            height: 860px;

            in property <string> config_path;
            in property <string> status_message;
            in property <int> status_kind;

            in-out property <string> capture_hotkey;
            in-out property <int> default_target_index;
            in-out property <string> default_delay_ms;
            in-out property <string> save_dir;
            in-out property <bool> copy_to_clipboard;
            in-out property <bool> open_editor;

            in-out property <string> text_commit_feedback_color;
            in-out property <float> radial_animation_speed_index;

            in-out property <string> shortcut_select;
            in-out property <string> shortcut_rectangle;
            in-out property <string> shortcut_ellipse;
            in-out property <string> shortcut_line;
            in-out property <string> shortcut_arrow;
            in-out property <string> shortcut_marker;
            in-out property <string> shortcut_text;
            in-out property <string> shortcut_pixelate;
            in-out property <string> shortcut_blur;
            in-out property <string> shortcut_copy;
            in-out property <string> shortcut_save;
            in-out property <string> shortcut_copy_and_save;
            in-out property <string> shortcut_undo;
            in-out property <string> shortcut_redo;
            in-out property <string> shortcut_delete_selected;

            callback save_requested();
            callback reload_requested();
            callback close_requested();

            VerticalBox {
                padding: 16px;
                spacing: 10px;

                Text {
                    text: "Pyro Settings";
                    font-size: 28px;
                }

                Text {
                    text: "Config file: " + root.config_path;
                    color: #9da3ae;
                }

                Text {
                    text: root.status_message;
                    color: root.status_kind == 1 ? #67d38d : root.status_kind == 2 ? #ff7575 : #9da3ae;
                }

                ScrollView {
                    VerticalBox {
                        spacing: 10px;

                        GroupBox {
                            title: "Capture Defaults";
                            VerticalBox {
                                spacing: 8px;

                                HorizontalBox {
                                    spacing: 8px;
                                    Text { text: "Capture Hotkey"; width: 220px; }
                                    LineEdit { text <=> root.capture_hotkey; }
                                }

                                HorizontalBox {
                                    spacing: 8px;
                                    Text { text: "Default Target"; width: 220px; }
                                    ComboBox {
                                        model: ["region", "primary", "all-displays"];
                                        current-index <=> root.default_target_index;
                                    }
                                }

                                HorizontalBox {
                                    spacing: 8px;
                                    Text { text: "Default Delay (ms)"; width: 220px; }
                                    LineEdit { text <=> root.default_delay_ms; }
                                }

                                HorizontalBox {
                                    spacing: 8px;
                                    Text { text: "Save Directory"; width: 220px; }
                                    LineEdit { text <=> root.save_dir; }
                                }

                                CheckBox {
                                    text: "Copy to clipboard by default";
                                    checked <=> root.copy_to_clipboard;
                                }

                                CheckBox {
                                    text: "Open region editor by default";
                                    checked <=> root.open_editor;
                                }
                            }
                        }

                        GroupBox {
                            title: "Editor";
                            VerticalBox {
                                spacing: 8px;

                                HorizontalBox {
                                    spacing: 8px;
                                    Text { text: "Text Commit Feedback Color"; width: 220px; }
                                    LineEdit { text <=> root.text_commit_feedback_color; }
                                }

                                HorizontalBox {
                                    spacing: 8px;
                                    Text { text: "Radial Palette Animation"; width: 220px; }
                                    Slider {
                                        minimum: 0;
                                        maximum: 4;
                                        value <=> root.radial_animation_speed_index;
                                        changed value => {
                                            root.radial_animation_speed_index = self.value.round();
                                        }
                                    }
                                    Text {
                                        width: 84px;
                                        text:
                                            root.radial_animation_speed_index < 0.5 ? "Instant" :
                                            root.radial_animation_speed_index < 1.5 ? "Fast" :
                                            root.radial_animation_speed_index < 2.5 ? "Normal" :
                                            root.radial_animation_speed_index < 3.5 ? "Slow" : "Slower";
                                    }
                                }
                            }
                        }

                        GroupBox {
                            title: "Editor Shortcuts";
                            VerticalBox {
                                spacing: 8px;

                                HorizontalBox { spacing: 8px; Text { text: "Select"; width: 220px; } LineEdit { text <=> root.shortcut_select; } }
                                HorizontalBox { spacing: 8px; Text { text: "Rectangle"; width: 220px; } LineEdit { text <=> root.shortcut_rectangle; } }
                                HorizontalBox { spacing: 8px; Text { text: "Ellipse"; width: 220px; } LineEdit { text <=> root.shortcut_ellipse; } }
                                HorizontalBox { spacing: 8px; Text { text: "Line"; width: 220px; } LineEdit { text <=> root.shortcut_line; } }
                                HorizontalBox { spacing: 8px; Text { text: "Arrow"; width: 220px; } LineEdit { text <=> root.shortcut_arrow; } }
                                HorizontalBox { spacing: 8px; Text { text: "Marker"; width: 220px; } LineEdit { text <=> root.shortcut_marker; } }
                                HorizontalBox { spacing: 8px; Text { text: "Text"; width: 220px; } LineEdit { text <=> root.shortcut_text; } }
                                HorizontalBox { spacing: 8px; Text { text: "Pixelate"; width: 220px; } LineEdit { text <=> root.shortcut_pixelate; } }
                                HorizontalBox { spacing: 8px; Text { text: "Blur"; width: 220px; } LineEdit { text <=> root.shortcut_blur; } }
                                HorizontalBox { spacing: 8px; Text { text: "Copy"; width: 220px; } LineEdit { text <=> root.shortcut_copy; } }
                                HorizontalBox { spacing: 8px; Text { text: "Save"; width: 220px; } LineEdit { text <=> root.shortcut_save; } }
                                HorizontalBox { spacing: 8px; Text { text: "Copy+Save"; width: 220px; } LineEdit { text <=> root.shortcut_copy_and_save; } }
                                HorizontalBox { spacing: 8px; Text { text: "Undo"; width: 220px; } LineEdit { text <=> root.shortcut_undo; } }
                                HorizontalBox { spacing: 8px; Text { text: "Redo"; width: 220px; } LineEdit { text <=> root.shortcut_redo; } }
                                HorizontalBox { spacing: 8px; Text { text: "Delete Selected"; width: 220px; } LineEdit { text <=> root.shortcut_delete_selected; } }
                            }
                        }
                    }
                }

                HorizontalBox {
                    spacing: 8px;

                    Button {
                        text: "Reload";
                        clicked => { root.reload_requested(); }
                    }

                    Button {
                        text: "Save";
                        clicked => { root.save_requested(); }
                    }

                    Button {
                        text: "Close";
                        clicked => { root.close_requested(); }
                    }
                }
            }
        }
    }

    pub fn run() -> Result<()> {
        let config_path = resolve_config_path()?;
        let ui = SettingsWindow::new().context("create settings window")?;

        ui.set_config_path(config_path.display().to_string().into());
        apply_loaded_config(&ui, &config_path);

        {
            let ui_handle = ui.as_weak();
            let config_path = config_path.clone();
            ui.on_save_requested(move || {
                let Some(ui) = ui_handle.upgrade() else {
                    return;
                };

                match collect_config_from_ui(&ui) {
                    Ok(config) => match save_config(&config_path, &config) {
                        Ok(()) => set_status(
                            &ui,
                            "Saved. Most changes apply on the next capture; hotkey changes still require restart.",
                            StatusKind::Success,
                        ),
                        Err(err) => {
                            set_status(&ui, &format!("Save failed: {err}"), StatusKind::Error)
                        }
                    },
                    Err(err) => set_status(&ui, &err, StatusKind::Error),
                }
            });
        }

        {
            let ui_handle = ui.as_weak();
            let config_path = config_path.clone();
            ui.on_reload_requested(move || {
                let Some(ui) = ui_handle.upgrade() else {
                    return;
                };
                apply_loaded_config(&ui, &config_path);
            });
        }

        {
            let ui_handle = ui.as_weak();
            ui.on_close_requested(move || {
                let Some(ui) = ui_handle.upgrade() else {
                    return;
                };
                ui.hide().ok();
                let _ = slint::quit_event_loop();
            });
        }

        ui.run().context("run settings window")?;
        Ok(())
    }

    fn apply_loaded_config(ui: &SettingsWindow, config_path: &Path) {
        match load_config(config_path) {
            Ok((config, warning)) => {
                bind_config_to_ui(ui, &config);
                if let Some(message) = warning {
                    set_status(ui, &message, StatusKind::Warning);
                } else {
                    set_status(ui, "Loaded settings.", StatusKind::Neutral);
                }
            }
            Err(err) => {
                set_status(
                    ui,
                    &format!("Load failed: {err}. Using in-memory defaults."),
                    StatusKind::Error,
                );
                bind_config_to_ui(ui, &AppConfig::default());
            }
        }
    }

    fn bind_config_to_ui(ui: &SettingsWindow, config: &AppConfig) {
        ui.set_capture_hotkey(config.capture_hotkey.clone().into());
        ui.set_default_target_index(target_to_index(config.default_target));
        ui.set_default_delay_ms(config.default_delay_ms.to_string().into());
        ui.set_save_dir(config.save_dir.display().to_string().into());
        ui.set_copy_to_clipboard(config.copy_to_clipboard);
        ui.set_open_editor(config.open_editor);
        ui.set_text_commit_feedback_color(config.editor.text_commit_feedback_color.clone().into());
        ui.set_radial_animation_speed_index(speed_to_slider_index(
            config.editor.radial_menu_animation_speed,
        ));

        ui.set_shortcut_select(config.editor.shortcuts.select.clone().into());
        ui.set_shortcut_rectangle(config.editor.shortcuts.rectangle.clone().into());
        ui.set_shortcut_ellipse(config.editor.shortcuts.ellipse.clone().into());
        ui.set_shortcut_line(config.editor.shortcuts.line.clone().into());
        ui.set_shortcut_arrow(config.editor.shortcuts.arrow.clone().into());
        ui.set_shortcut_marker(config.editor.shortcuts.marker.clone().into());
        ui.set_shortcut_text(config.editor.shortcuts.text.clone().into());
        ui.set_shortcut_pixelate(config.editor.shortcuts.pixelate.clone().into());
        ui.set_shortcut_blur(config.editor.shortcuts.blur.clone().into());
        ui.set_shortcut_copy(config.editor.shortcuts.copy.clone().into());
        ui.set_shortcut_save(config.editor.shortcuts.save.clone().into());
        ui.set_shortcut_copy_and_save(config.editor.shortcuts.copy_and_save.clone().into());
        ui.set_shortcut_undo(config.editor.shortcuts.undo.clone().into());
        ui.set_shortcut_redo(config.editor.shortcuts.redo.clone().into());
        ui.set_shortcut_delete_selected(config.editor.shortcuts.delete_selected.clone().into());
    }

    fn collect_config_from_ui(ui: &SettingsWindow) -> std::result::Result<AppConfig, String> {
        let capture_hotkey = read_required("Capture hotkey", ui.get_capture_hotkey())?;
        let default_target = index_to_target(ui.get_default_target_index())?;
        let default_delay_ms = parse_delay(ui.get_default_delay_ms())?;
        let save_dir = read_required("Save directory", ui.get_save_dir())?;
        let text_commit_feedback_color = validate_color(ui.get_text_commit_feedback_color())?;
        let radial_menu_animation_speed =
            slider_index_to_speed(ui.get_radial_animation_speed_index());

        let shortcuts = EditorShortcutConfig {
            select: read_required("Shortcut Select", ui.get_shortcut_select())?,
            rectangle: read_required("Shortcut Rectangle", ui.get_shortcut_rectangle())?,
            ellipse: read_required("Shortcut Ellipse", ui.get_shortcut_ellipse())?,
            line: read_required("Shortcut Line", ui.get_shortcut_line())?,
            arrow: read_required("Shortcut Arrow", ui.get_shortcut_arrow())?,
            marker: read_required("Shortcut Marker", ui.get_shortcut_marker())?,
            text: read_required("Shortcut Text", ui.get_shortcut_text())?,
            pixelate: read_required("Shortcut Pixelate", ui.get_shortcut_pixelate())?,
            blur: read_required("Shortcut Blur", ui.get_shortcut_blur())?,
            copy: read_required("Shortcut Copy", ui.get_shortcut_copy())?,
            save: read_required("Shortcut Save", ui.get_shortcut_save())?,
            copy_and_save: read_required("Shortcut Copy+Save", ui.get_shortcut_copy_and_save())?,
            undo: read_required("Shortcut Undo", ui.get_shortcut_undo())?,
            redo: read_required("Shortcut Redo", ui.get_shortcut_redo())?,
            delete_selected: read_required(
                "Shortcut Delete Selected",
                ui.get_shortcut_delete_selected(),
            )?,
        };

        Ok(AppConfig {
            capture_hotkey,
            default_target,
            default_delay_ms,
            copy_to_clipboard: ui.get_copy_to_clipboard(),
            open_editor: ui.get_open_editor(),
            save_dir: PathBuf::from(save_dir),
            editor: EditorConfig {
                shortcuts,
                text_commit_feedback_color,
                radial_menu_animation_speed,
            },
        })
    }

    fn read_required(label: &str, value: SharedString) -> std::result::Result<String, String> {
        let owned = value.to_string();
        let trimmed = owned.trim();
        if trimmed.is_empty() {
            return Err(format!("{label} cannot be empty."));
        }
        Ok(trimmed.to_string())
    }

    fn parse_delay(value: SharedString) -> std::result::Result<u64, String> {
        let owned = value.to_string();
        let trimmed = owned.trim();
        if trimmed.is_empty() {
            return Ok(0);
        }
        trimmed
            .parse::<u64>()
            .map_err(|_| "Default delay (ms) must be a non-negative integer.".to_string())
    }

    fn validate_color(value: SharedString) -> std::result::Result<String, String> {
        let owned = value.to_string();
        let trimmed = owned.trim();
        normalize_hex_color(trimmed)
            .ok_or_else(|| "Text commit feedback color must be #RRGGBB.".to_string())
    }

    fn normalize_hex_color(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        let hex = trimmed.strip_prefix('#').unwrap_or(trimmed);
        if hex.len() != 6 || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return None;
        }
        Some(format!("#{}", hex.to_ascii_uppercase()))
    }

    fn resolve_config_path() -> Result<PathBuf> {
        if let Some(arg) = env::args_os().nth(1) {
            return to_absolute(PathBuf::from(arg));
        }

        let base = dirs::config_dir().context("resolve config directory")?;
        Ok(base.join("pyro").join("config.toml"))
    }

    fn to_absolute(path: PathBuf) -> Result<PathBuf> {
        if path.is_absolute() {
            return Ok(path);
        }
        Ok(env::current_dir()
            .context("resolve current directory")?
            .join(path))
    }

    fn load_config(path: &Path) -> Result<(AppConfig, Option<String>)> {
        if !path.exists() {
            return Ok((
                AppConfig::default(),
                Some("Config file does not exist yet. Press Save to create it.".to_string()),
            ));
        }

        let contents =
            fs::read_to_string(path).with_context(|| format!("read config {}", path.display()))?;
        match toml::from_str::<AppConfig>(&contents) {
            Ok(config) => Ok((config, None)),
            Err(err) => Ok((
                AppConfig::default(),
                Some(format!(
                    "Config parse failed ({}). Loaded defaults in editor.",
                    err
                )),
            )),
        }
    }

    fn save_config(path: &Path, config: &AppConfig) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create config dir {}", parent.display()))?;
        }

        let serialized = toml::to_string_pretty(config).context("serialize config")?;
        fs::write(path, serialized).with_context(|| format!("write config {}", path.display()))?;
        Ok(())
    }

    fn set_status(ui: &SettingsWindow, message: &str, kind: StatusKind) {
        ui.set_status_message(message.into());
        ui.set_status_kind(kind as i32);
    }

    fn target_to_index(target: CaptureTarget) -> i32 {
        match target {
            CaptureTarget::Region => 0,
            CaptureTarget::Primary => 1,
            CaptureTarget::AllDisplays => 2,
        }
    }

    fn index_to_target(index: i32) -> std::result::Result<CaptureTarget, String> {
        match index {
            0 => Ok(CaptureTarget::Region),
            1 => Ok(CaptureTarget::Primary),
            2 => Ok(CaptureTarget::AllDisplays),
            _ => Err("Default target selection is invalid.".to_string()),
        }
    }

    fn speed_to_slider_index(speed: RadialMenuAnimationSpeed) -> f32 {
        match speed {
            RadialMenuAnimationSpeed::Instant => 0.0,
            RadialMenuAnimationSpeed::Fast => 1.0,
            RadialMenuAnimationSpeed::Normal => 2.0,
            RadialMenuAnimationSpeed::Slow => 3.0,
            RadialMenuAnimationSpeed::Slower => 4.0,
        }
    }

    fn slider_index_to_speed(index: f32) -> RadialMenuAnimationSpeed {
        match index.round() as i32 {
            0 => RadialMenuAnimationSpeed::Instant,
            1 => RadialMenuAnimationSpeed::Fast,
            3 => RadialMenuAnimationSpeed::Slow,
            4 => RadialMenuAnimationSpeed::Slower,
            _ => RadialMenuAnimationSpeed::Normal,
        }
    }

    #[repr(i32)]
    enum StatusKind {
        Neutral = 0,
        Success = 1,
        Error = 2,
        Warning = 3,
    }

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    enum CaptureTarget {
        Primary,
        Region,
        AllDisplays,
    }

    #[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    enum RadialMenuAnimationSpeed {
        Instant,
        Fast,
        #[default]
        Normal,
        Slow,
        Slower,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct AppConfig {
        #[serde(default = "default_hotkey")]
        capture_hotkey: String,
        #[serde(default = "default_target")]
        default_target: CaptureTarget,
        #[serde(default)]
        default_delay_ms: u64,
        #[serde(default = "default_copy_to_clipboard")]
        copy_to_clipboard: bool,
        #[serde(default = "default_open_editor")]
        open_editor: bool,
        #[serde(default = "default_save_dir")]
        save_dir: PathBuf,
        #[serde(default)]
        editor: EditorConfig,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct EditorConfig {
        #[serde(default)]
        shortcuts: EditorShortcutConfig,
        #[serde(default = "default_text_commit_feedback_color")]
        text_commit_feedback_color: String,
        #[serde(default)]
        radial_menu_animation_speed: RadialMenuAnimationSpeed,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct EditorShortcutConfig {
        #[serde(default = "default_shortcut_select")]
        select: String,
        #[serde(default = "default_shortcut_rectangle")]
        rectangle: String,
        #[serde(default = "default_shortcut_ellipse")]
        ellipse: String,
        #[serde(default = "default_shortcut_line")]
        line: String,
        #[serde(default = "default_shortcut_arrow")]
        arrow: String,
        #[serde(default = "default_shortcut_marker")]
        marker: String,
        #[serde(default = "default_shortcut_text")]
        text: String,
        #[serde(default = "default_shortcut_pixelate")]
        pixelate: String,
        #[serde(default = "default_shortcut_blur")]
        blur: String,
        #[serde(default = "default_shortcut_copy")]
        copy: String,
        #[serde(default = "default_shortcut_save")]
        save: String,
        #[serde(default = "default_shortcut_copy_save")]
        copy_and_save: String,
        #[serde(default = "default_shortcut_undo")]
        undo: String,
        #[serde(default = "default_shortcut_redo")]
        redo: String,
        #[serde(default = "default_shortcut_delete")]
        delete_selected: String,
    }

    impl Default for AppConfig {
        fn default() -> Self {
            Self {
                capture_hotkey: default_hotkey(),
                default_target: default_target(),
                default_delay_ms: 0,
                copy_to_clipboard: default_copy_to_clipboard(),
                open_editor: default_open_editor(),
                save_dir: default_save_dir(),
                editor: EditorConfig::default(),
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

    fn default_hotkey() -> String {
        "PrintScreen".to_string()
    }

    fn default_target() -> CaptureTarget {
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

    fn default_text_commit_feedback_color() -> String {
        "#48B4FF".to_string()
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
}

#[cfg(target_os = "windows")]
fn main() -> anyhow::Result<()> {
    windows_app::run()
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("pyro-settings currently supports Windows only.");
}
