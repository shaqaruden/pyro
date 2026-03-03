#[cfg(target_os = "windows")]
mod windows_app {
    use std::cell::RefCell;
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use anyhow::{Context, Result};
    use serde::{Deserialize, Serialize};
    use slint::{Color, ComponentHandle, Rgba8Pixel, SharedPixelBuffer, SharedString};

    const PALETTE_SIZE: usize = 8;
    const WHEEL_SIZE: u32 = 220;
    const HUE_RING_OUTER: f32 = 106.0;
    const HUE_RING_INNER: f32 = 82.0;
    const TRIANGLE_RADIUS: f32 = 70.0;
    const HUE_WHEEL_INTERACTIVE_MIN_FRAME_MS: u64 = 90;
    const HUE_WHEEL_INTERACTIVE_MIN_DEGREE_DELTA: i32 = 8;

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
            icon: @image-url("../assets/app-icon.ico");
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
            in-out property <int> palette_selected_index;
            in-out property <string> palette_selected_hex;
            in-out property <string> palette_selected_slot_text;
            in-out property <string> palette_hsl_label;
            in-out property <image> palette_wheel_image;
            in-out property <bool> palette_picker_visible;
            in-out property <length> palette_hue_marker_x;
            in-out property <length> palette_hue_marker_y;
            in-out property <length> palette_triangle_marker_x;
            in-out property <length> palette_triangle_marker_y;
            in-out property <color> palette_preview_1;
            in-out property <color> palette_preview_2;
            in-out property <color> palette_preview_3;
            in-out property <color> palette_preview_4;
            in-out property <color> palette_preview_5;
            in-out property <color> palette_preview_6;
            in-out property <color> palette_preview_7;
            in-out property <color> palette_preview_8;

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
            callback palette_slot_selected(index: int);
            callback palette_wheel_drag_start(x: length, y: length);
            callback palette_wheel_drag_move(x: length, y: length);
            callback palette_wheel_drag_end();
            callback palette_picker_close_requested();

            keyboard_scope := FocusScope {
                width: parent.width;
                height: parent.height;
                focus-on-click: true;
                key-pressed(event) => {
                    if (root.palette_picker_visible && event.text == Key.Escape) {
                        root.palette_picker_close_requested();
                        return accept;
                    }
                    return reject;
                }
            }

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

                                Text {
                                    text: "Annotation Palette";
                                    color: #9da3ae;
                                }

                                HorizontalBox {
                                    spacing: 6px;

                                    Rectangle {
                                        width: 24px; height: 24px;
                                        border-width: root.palette_selected_index == 0 ? 2px : 1px;
                                        border-color: root.palette_selected_index == 0 ? #89c4ff : #5e636d;
                                        background: root.palette_preview_1;
                                        TouchArea { clicked => { root.palette_slot_selected(0); } }
                                    }
                                    Rectangle {
                                        width: 24px; height: 24px;
                                        border-width: root.palette_selected_index == 1 ? 2px : 1px;
                                        border-color: root.palette_selected_index == 1 ? #89c4ff : #5e636d;
                                        background: root.palette_preview_2;
                                        TouchArea { clicked => { root.palette_slot_selected(1); } }
                                    }
                                    Rectangle {
                                        width: 24px; height: 24px;
                                        border-width: root.palette_selected_index == 2 ? 2px : 1px;
                                        border-color: root.palette_selected_index == 2 ? #89c4ff : #5e636d;
                                        background: root.palette_preview_3;
                                        TouchArea { clicked => { root.palette_slot_selected(2); } }
                                    }
                                    Rectangle {
                                        width: 24px; height: 24px;
                                        border-width: root.palette_selected_index == 3 ? 2px : 1px;
                                        border-color: root.palette_selected_index == 3 ? #89c4ff : #5e636d;
                                        background: root.palette_preview_4;
                                        TouchArea { clicked => { root.palette_slot_selected(3); } }
                                    }
                                    Rectangle {
                                        width: 24px; height: 24px;
                                        border-width: root.palette_selected_index == 4 ? 2px : 1px;
                                        border-color: root.palette_selected_index == 4 ? #89c4ff : #5e636d;
                                        background: root.palette_preview_5;
                                        TouchArea { clicked => { root.palette_slot_selected(4); } }
                                    }
                                    Rectangle {
                                        width: 24px; height: 24px;
                                        border-width: root.palette_selected_index == 5 ? 2px : 1px;
                                        border-color: root.palette_selected_index == 5 ? #89c4ff : #5e636d;
                                        background: root.palette_preview_6;
                                        TouchArea { clicked => { root.palette_slot_selected(5); } }
                                    }
                                    Rectangle {
                                        width: 24px; height: 24px;
                                        border-width: root.palette_selected_index == 6 ? 2px : 1px;
                                        border-color: root.palette_selected_index == 6 ? #89c4ff : #5e636d;
                                        background: root.palette_preview_7;
                                        TouchArea { clicked => { root.palette_slot_selected(6); } }
                                    }
                                    Rectangle {
                                        width: 24px; height: 24px;
                                        border-width: root.palette_selected_index == 7 ? 2px : 1px;
                                        border-color: root.palette_selected_index == 7 ? #89c4ff : #5e636d;
                                        background: root.palette_preview_8;
                                        TouchArea { clicked => { root.palette_slot_selected(7); } }
                                    }
                                }

                                Text {
                                    text: "Click a swatch to open the color picker";
                                    color: #8f95a1;
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

            if root.palette_picker_visible: Rectangle {
                x: 0;
                y: 0;
                width: parent.width;
                height: parent.height;
                background: #00000080;

                modal_focus := FocusScope {
                    width: parent.width;
                    height: parent.height;
                    focus-on-click: true;
                    key-pressed(event) => {
                        if (event.text == Key.Escape) {
                            root.palette_picker_close_requested();
                            return accept;
                        }
                        return reject;
                    }
                }

                backdrop_touch := TouchArea {
                    pointer-event(event) => {
                        if (event.button != PointerEventButton.left || event.kind != PointerEventKind.down) {
                            return;
                        }
                        modal_focus.focus();
                        if (self.mouse_x < picker_panel.x
                            || self.mouse_x > picker_panel.x + picker_panel.width
                            || self.mouse_y < picker_panel.y
                            || self.mouse_y > picker_panel.y + picker_panel.height) {
                            root.palette_picker_close_requested();
                        }
                    }
                }

                picker_panel := Rectangle {
                    width: 540px;
                    height: 320px;
                    x: (parent.width - self.width) / 2;
                    y: (parent.height - self.height) / 2;
                    border-width: 1px;
                    border-color: #4a4f57;
                    background: #1f2125;

                    VerticalBox {
                        padding: 12px;
                        spacing: 8px;

                        HorizontalBox {
                            spacing: 8px;
                            Text { text: "Color Picker"; color: #d9dde6; }
                            Rectangle { width: 20px; }
                            Text { text: root.palette_selected_slot_text; color: #9da3ae; }
                            Rectangle { width: 20px; }
                            Button {
                                text: "Done";
                                clicked => { root.palette_picker_close_requested(); }
                            }
                        }

                        HorizontalBox {
                            spacing: 12px;

                            Rectangle {
                                width: 220px;
                                height: 220px;
                                border-width: 1px;
                                border-color: #4a4f57;
                                background: #232529;

                                Image {
                                    width: parent.width;
                                    height: parent.height;
                                    source: root.palette_wheel_image;
                                }

                                Rectangle {
                                    width: 14px;
                                    height: 14px;
                                    x: root.palette_hue_marker_x - (self.width / 2);
                                    y: root.palette_hue_marker_y - (self.height / 2);
                                    border-radius: 7px;
                                    border-width: 2px;
                                    border-color: #000000;
                                    background: #00000000;
                                }

                                Rectangle {
                                    width: 10px;
                                    height: 10px;
                                    x: root.palette_hue_marker_x - (self.width / 2);
                                    y: root.palette_hue_marker_y - (self.height / 2);
                                    border-radius: 5px;
                                    border-width: 2px;
                                    border-color: #FFFFFF;
                                    background: #00000000;
                                }

                                Rectangle {
                                    width: 14px;
                                    height: 14px;
                                    x: root.palette_triangle_marker_x - (self.width / 2);
                                    y: root.palette_triangle_marker_y - (self.height / 2);
                                    border-radius: 7px;
                                    border-width: 2px;
                                    border-color: #000000;
                                    background: #00000000;
                                }

                                Rectangle {
                                    width: 10px;
                                    height: 10px;
                                    x: root.palette_triangle_marker_x - (self.width / 2);
                                    y: root.palette_triangle_marker_y - (self.height / 2);
                                    border-radius: 5px;
                                    border-width: 2px;
                                    border-color: #FFFFFF;
                                    background: #00000000;
                                }

                                TouchArea {
                                    mouse-cursor: crosshair;
                                    pointer-event(event) => {
                                        if (event.button != PointerEventButton.left) {
                                            return;
                                        }
                                        if (event.kind == PointerEventKind.down) {
                                            root.palette_wheel_drag_start(self.mouse_x, self.mouse_y);
                                        } else if (event.kind == PointerEventKind.up || event.kind == PointerEventKind.cancel) {
                                            root.palette_wheel_drag_end();
                                        }
                                    }
                                    moved => {
                                        if (self.pressed) {
                                            root.palette_wheel_drag_move(self.mouse_x, self.mouse_y);
                                        }
                                    }
                                }
                            }

                            VerticalBox {
                                spacing: 8px;

                                HorizontalBox {
                                    spacing: 8px;
                                    Text { text: "Hex"; width: 44px; }
                                    LineEdit {
                                        text <=> root.palette_selected_hex;
                                        read-only: true;
                                    }
                                }

                                Text {
                                    text: root.palette_hsl_label;
                                    color: #9da3ae;
                                }

                                Text {
                                    text: "Click outside to close";
                                    color: #7f8693;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct HslColor {
        h: f32,
        s: f32,
        l: f32,
    }

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    enum PaletteDragMode {
        None,
        Hue,
        Triangle,
    }

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    enum PaletteDragUpdate {
        None,
        Hue,
        Triangle,
    }

    #[derive(Debug, Clone)]
    struct PaletteUiState {
        colors: [String; PALETTE_SIZE],
        selected: usize,
        hsl: HslColor,
        triangle_weights: [f32; 3],
        triangle_marker: Vec2,
        drag_mode: PaletteDragMode,
    }

    #[derive(Debug, Default)]
    struct PaletteRenderCache {
        hue_bucket: i32,
        image: Option<slint::Image>,
        last_interactive_update: Option<Instant>,
    }

    impl PaletteUiState {
        fn from_colors(colors: [String; PALETTE_SIZE]) -> Self {
            let mut sanitized = default_annotation_palette();
            for (dst, src) in sanitized.iter_mut().zip(colors.iter()) {
                if let Some(normalized) = normalize_hex_color(src) {
                    *dst = normalized;
                }
            }
            let rgb = hex_to_rgb(&sanitized[0]).unwrap_or([255, 255, 255]);
            let mut hsl = rgb_to_hsl(rgb);
            hsl.h = hsl.h.rem_euclid(360.0);
            let triangle_weights = weights_from_rgb_and_hue(rgb, hsl.h);
            let triangle_marker = marker_from_hue_weights(hsl.h, triangle_weights);
            Self {
                colors: sanitized,
                selected: 0,
                hsl,
                triangle_weights,
                triangle_marker,
                drag_mode: PaletteDragMode::None,
            }
        }

        fn selected_hex(&self) -> &str {
            &self.colors[self.selected]
        }

        fn selected_rgb(&self) -> [u8; 3] {
            hex_to_rgb(self.selected_hex()).unwrap_or([255, 255, 255])
        }

        fn select(&mut self, index: usize) {
            if index >= PALETTE_SIZE {
                return;
            }
            self.selected = index;
            let rgb = self.selected_rgb();
            self.hsl = rgb_to_hsl(rgb);
            self.hsl.h = self.hsl.h.rem_euclid(360.0);
            self.triangle_weights = weights_from_rgb_and_hue(rgb, self.hsl.h);
            self.triangle_marker = marker_from_hue_weights(self.hsl.h, self.triangle_weights);
            self.drag_mode = PaletteDragMode::None;
        }

        fn set_selected_hue(&mut self, hue: f32) -> bool {
            let hue = hue.rem_euclid(360.0);
            if (self.hsl.h - hue).abs() < 0.1 {
                return false;
            }
            self.hsl.h = hue;
            let rgb = rgb_from_hue_weights(hue, self.triangle_weights);
            self.colors[self.selected] = rgb_to_hex(rgb);
            self.hsl = rgb_to_hsl(rgb);
            self.hsl.h = hue;
            self.triangle_marker = marker_from_hue_weights(hue, self.triangle_weights);
            true
        }

        fn set_selected_triangle(&mut self, weights: [f32; 3], marker: Vec2) -> bool {
            let weights = normalize_weights(weights);
            let changed = weights
                .iter()
                .zip(self.triangle_weights.iter())
                .any(|(next, cur)| (next - cur).abs() > 0.0005);
            let marker_dx = marker.x - self.triangle_marker.x;
            let marker_dy = marker.y - self.triangle_marker.y;
            let marker_changed = (marker_dx * marker_dx) + (marker_dy * marker_dy) > 0.25;
            if !changed && !marker_changed {
                return false;
            }
            self.triangle_weights = weights;
            let hue = self.hsl.h;
            let rgb = rgb_from_hue_weights(hue, weights);
            self.colors[self.selected] = rgb_to_hex(rgb);
            self.hsl = rgb_to_hsl(rgb);
            self.hsl.h = hue;
            self.triangle_marker = marker_from_hue_weights(hue, weights);
            true
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct Vec2 {
        x: f32,
        y: f32,
    }

    pub fn run() -> Result<()> {
        let config_path = resolve_config_path()?;
        let ui = SettingsWindow::new().context("create settings window")?;
        let palette_state = Rc::new(RefCell::new(PaletteUiState::from_colors(
            default_annotation_palette(),
        )));
        let render_cache = Rc::new(RefCell::new(PaletteRenderCache {
            hue_bucket: -1,
            image: None,
            last_interactive_update: None,
        }));

        ui.set_config_path(config_path.display().to_string().into());
        apply_loaded_config(&ui, &config_path, &palette_state, &render_cache);

        {
            let ui_handle = ui.as_weak();
            let config_path = config_path.clone();
            let palette_state = palette_state.clone();
            ui.on_save_requested(move || {
                let Some(ui) = ui_handle.upgrade() else {
                    return;
                };

                let palette = palette_state.borrow();
                match collect_config_from_ui(&ui, &palette) {
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
            let palette_state = palette_state.clone();
            let render_cache = render_cache.clone();
            ui.on_reload_requested(move || {
                let Some(ui) = ui_handle.upgrade() else {
                    return;
                };
                apply_loaded_config(&ui, &config_path, &palette_state, &render_cache);
            });
        }

        {
            let ui_handle = ui.as_weak();
            let palette_state = palette_state.clone();
            let render_cache = render_cache.clone();
            ui.on_palette_slot_selected(move |index| {
                let Some(ui) = ui_handle.upgrade() else {
                    return;
                };
                let mut palette = palette_state.borrow_mut();
                palette.select(index.max(0) as usize);
                ui.set_palette_picker_visible(true);
                apply_palette_ui(&ui, &palette, &mut render_cache.borrow_mut());
            });
        }

        {
            let ui_handle = ui.as_weak();
            let palette_state = palette_state.clone();
            let render_cache = render_cache.clone();
            ui.on_palette_wheel_drag_start(move |x, y| {
                let Some(ui) = ui_handle.upgrade() else {
                    return;
                };
                let mut palette = palette_state.borrow_mut();
                let update = begin_palette_wheel_drag(&mut palette, x, y);
                if update != PaletteDragUpdate::None {
                    apply_palette_drag_ui(&ui, &palette, &mut render_cache.borrow_mut(), update);
                }
            });
        }

        {
            let ui_handle = ui.as_weak();
            let palette_state = palette_state.clone();
            let render_cache = render_cache.clone();
            ui.on_palette_wheel_drag_move(move |x, y| {
                let Some(ui) = ui_handle.upgrade() else {
                    return;
                };
                let mut palette = palette_state.borrow_mut();
                let update = continue_palette_wheel_drag(&mut palette, x, y);
                if update != PaletteDragUpdate::None {
                    apply_palette_drag_ui(&ui, &palette, &mut render_cache.borrow_mut(), update);
                }
            });
        }

        {
            let ui_handle = ui.as_weak();
            let palette_state = palette_state.clone();
            let render_cache = render_cache.clone();
            ui.on_palette_wheel_drag_end(move || {
                let Some(ui) = ui_handle.upgrade() else {
                    return;
                };
                let mut palette = palette_state.borrow_mut();
                palette.drag_mode = PaletteDragMode::None;
                apply_palette_ui(&ui, &palette, &mut render_cache.borrow_mut());
            });
        }

        {
            let ui_handle = ui.as_weak();
            ui.on_palette_picker_close_requested(move || {
                let Some(ui) = ui_handle.upgrade() else {
                    return;
                };
                ui.set_palette_picker_visible(false);
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

    fn apply_loaded_config(
        ui: &SettingsWindow,
        config_path: &Path,
        palette_state: &Rc<RefCell<PaletteUiState>>,
        render_cache: &Rc<RefCell<PaletteRenderCache>>,
    ) {
        match load_config(config_path) {
            Ok((config, warning)) => {
                bind_config_to_ui(ui, &config, palette_state, render_cache);
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
                bind_config_to_ui(ui, &AppConfig::default(), palette_state, render_cache);
            }
        }
    }

    fn bind_config_to_ui(
        ui: &SettingsWindow,
        config: &AppConfig,
        palette_state: &Rc<RefCell<PaletteUiState>>,
        render_cache: &Rc<RefCell<PaletteRenderCache>>,
    ) {
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
        ui.set_palette_picker_visible(false);

        {
            let mut palette = palette_state.borrow_mut();
            *palette = PaletteUiState::from_colors(config.editor.annotation_palette.clone());
            apply_palette_ui(ui, &palette, &mut render_cache.borrow_mut());
        }
    }

    fn collect_config_from_ui(
        ui: &SettingsWindow,
        palette: &PaletteUiState,
    ) -> std::result::Result<AppConfig, String> {
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
                annotation_palette: palette.colors.clone(),
            },
        })
    }

    fn apply_palette_ui(
        ui: &SettingsWindow,
        palette: &PaletteUiState,
        render_cache: &mut PaletteRenderCache,
    ) {
        let selected = palette.selected.min(PALETTE_SIZE.saturating_sub(1));
        ui.set_palette_selected_index(selected as i32);
        ui.set_palette_selected_hex(palette.selected_hex().to_string().into());
        ui.set_palette_selected_slot_text(format!("Selected slot: {}", selected + 1).into());
        ui.set_palette_hsl_label(
            format!(
                "H: {:03.0}  S: {:03.0}%  L: {:03.0}%",
                palette.hsl.h,
                palette.hsl.s * 100.0,
                palette.hsl.l * 100.0
            )
            .into(),
        );

        ui.set_palette_preview_1(hex_to_slint_color(&palette.colors[0]));
        ui.set_palette_preview_2(hex_to_slint_color(&palette.colors[1]));
        ui.set_palette_preview_3(hex_to_slint_color(&palette.colors[2]));
        ui.set_palette_preview_4(hex_to_slint_color(&palette.colors[3]));
        ui.set_palette_preview_5(hex_to_slint_color(&palette.colors[4]));
        ui.set_palette_preview_6(hex_to_slint_color(&palette.colors[5]));
        ui.set_palette_preview_7(hex_to_slint_color(&palette.colors[6]));
        ui.set_palette_preview_8(hex_to_slint_color(&palette.colors[7]));
        update_palette_wheel_image(ui, palette.hsl.h, render_cache, true);

        let hue_marker = hue_marker_position(palette.hsl.h);
        ui.set_palette_hue_marker_x(hue_marker.x);
        ui.set_palette_hue_marker_y(hue_marker.y);
        ui.set_palette_triangle_marker_x(palette.triangle_marker.x);
        ui.set_palette_triangle_marker_y(palette.triangle_marker.y);
    }

    fn apply_palette_drag_ui(
        ui: &SettingsWindow,
        palette: &PaletteUiState,
        render_cache: &mut PaletteRenderCache,
        update: PaletteDragUpdate,
    ) {
        set_palette_preview_slot(
            ui,
            palette.selected,
            hex_to_slint_color(palette.selected_hex()),
        );

        if update == PaletteDragUpdate::Hue {
            update_palette_wheel_image(ui, palette.hsl.h, render_cache, false);
        }

        let hue_marker = hue_marker_position(palette.hsl.h);
        ui.set_palette_hue_marker_x(hue_marker.x);
        ui.set_palette_hue_marker_y(hue_marker.y);
        ui.set_palette_triangle_marker_x(palette.triangle_marker.x);
        ui.set_palette_triangle_marker_y(palette.triangle_marker.y);
    }

    fn set_palette_preview_slot(ui: &SettingsWindow, index: usize, color: Color) {
        match index {
            0 => ui.set_palette_preview_1(color),
            1 => ui.set_palette_preview_2(color),
            2 => ui.set_palette_preview_3(color),
            3 => ui.set_palette_preview_4(color),
            4 => ui.set_palette_preview_5(color),
            5 => ui.set_palette_preview_6(color),
            6 => ui.set_palette_preview_7(color),
            7 => ui.set_palette_preview_8(color),
            _ => {}
        }
    }

    fn update_palette_wheel_image(
        ui: &SettingsWindow,
        hue_degrees: f32,
        render_cache: &mut PaletteRenderCache,
        force: bool,
    ) {
        let now = Instant::now();
        let hue_bucket = hue_degrees.round() as i32;
        if !force && render_cache.image.is_some() {
            let within_rate_limit = render_cache.last_interactive_update.is_some_and(|last| {
                now.duration_since(last) < Duration::from_millis(HUE_WHEEL_INTERACTIVE_MIN_FRAME_MS)
            });
            let hue_delta_small = (hue_bucket - render_cache.hue_bucket).abs()
                < HUE_WHEEL_INTERACTIVE_MIN_DEGREE_DELTA;
            if within_rate_limit || hue_delta_small {
                return;
            }
        }

        if render_cache.hue_bucket != hue_bucket || render_cache.image.is_none() {
            render_cache.image = Some(build_palette_wheel_background(hue_degrees));
            render_cache.hue_bucket = hue_bucket;
            if let Some(image) = &render_cache.image {
                ui.set_palette_wheel_image(image.clone());
            }
        }
        if !force {
            render_cache.last_interactive_update = Some(now);
        }
    }

    fn begin_palette_wheel_drag(palette: &mut PaletteUiState, x: f32, y: f32) -> PaletteDragUpdate {
        let mode = palette_drag_mode_for_point(palette, x, y);
        palette.drag_mode = mode;
        continue_palette_wheel_drag(palette, x, y)
    }

    fn continue_palette_wheel_drag(
        palette: &mut PaletteUiState,
        x: f32,
        y: f32,
    ) -> PaletteDragUpdate {
        match palette.drag_mode {
            PaletteDragMode::None => PaletteDragUpdate::None,
            PaletteDragMode::Hue => {
                let center = WHEEL_SIZE as f32 * 0.5;
                let hue = ((y - center).atan2(x - center).to_degrees() + 90.0).rem_euclid(360.0);
                if palette.set_selected_hue(hue) {
                    PaletteDragUpdate::Hue
                } else {
                    PaletteDragUpdate::None
                }
            }
            PaletteDragMode::Triangle => {
                let (weights, marker) = triangle_weights_and_marker_for_point(palette.hsl.h, x, y);
                if palette.set_selected_triangle(weights, marker) {
                    PaletteDragUpdate::Triangle
                } else {
                    PaletteDragUpdate::None
                }
            }
        }
    }

    fn palette_drag_mode_for_point(palette: &PaletteUiState, x: f32, y: f32) -> PaletteDragMode {
        let center = WHEEL_SIZE as f32 * 0.5;
        let dx = x - center;
        let dy = y - center;
        let radius = (dx * dx + dy * dy).sqrt();
        if (HUE_RING_INNER..=HUE_RING_OUTER).contains(&radius) {
            return PaletteDragMode::Hue;
        }

        let (hue_vertex, white_vertex, black_vertex) = wheel_triangle_vertices(palette.hsl.h);
        let Some((wa, wb, wc)) = barycentric(Vec2 { x, y }, hue_vertex, white_vertex, black_vertex)
        else {
            return PaletteDragMode::None;
        };
        if wa >= -0.005 && wb >= -0.005 && wc >= -0.005 {
            PaletteDragMode::Triangle
        } else {
            PaletteDragMode::None
        }
    }

    fn triangle_weights_and_marker_for_point(hue: f32, x: f32, y: f32) -> ([f32; 3], Vec2) {
        let (hue_vertex, white_vertex, black_vertex) = wheel_triangle_vertices(hue);
        let (wa, wb, wc) = barycentric(Vec2 { x, y }, hue_vertex, white_vertex, black_vertex)
            .unwrap_or((0.0, 0.0, 1.0));

        let wa = wa.max(0.0);
        let wb = wb.max(0.0);
        let wc = wc.max(0.0);
        let sum = (wa + wb + wc).max(0.0001);
        let wa = wa / sum;
        let wb = wb / sum;
        let wc = wc / sum;

        let weights = [wa, wb, wc];
        let marker = Vec2 {
            x: hue_vertex.x * wa + white_vertex.x * wb + black_vertex.x * wc,
            y: hue_vertex.y * wa + white_vertex.y * wb + black_vertex.y * wc,
        };
        (weights, marker)
    }

    fn normalize_weights(weights: [f32; 3]) -> [f32; 3] {
        let w0 = weights[0].max(0.0);
        let w1 = weights[1].max(0.0);
        let w2 = weights[2].max(0.0);
        let sum = (w0 + w1 + w2).max(0.0001);
        [w0 / sum, w1 / sum, w2 / sum]
    }

    fn rgb_from_hue_weights(hue: f32, weights: [f32; 3]) -> [u8; 3] {
        let weights = normalize_weights(weights);
        let hue_rgb = hsl_to_rgb_f32(HslColor {
            h: hue,
            s: 1.0,
            l: 0.5,
        });
        [
            ((hue_rgb[0] * weights[0] + weights[1]) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8,
            ((hue_rgb[1] * weights[0] + weights[1]) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8,
            ((hue_rgb[2] * weights[0] + weights[1]) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8,
        ]
    }

    fn marker_from_hue_weights(hue: f32, weights: [f32; 3]) -> Vec2 {
        let weights = normalize_weights(weights);
        let (hue_vertex, white_vertex, black_vertex) = wheel_triangle_vertices(hue);
        Vec2 {
            x: hue_vertex.x * weights[0]
                + white_vertex.x * weights[1]
                + black_vertex.x * weights[2],
            y: hue_vertex.y * weights[0]
                + white_vertex.y * weights[1]
                + black_vertex.y * weights[2],
        }
    }

    fn weights_from_rgb_and_hue(rgb: [u8; 3], hue: f32) -> [f32; 3] {
        let rgb = [
            rgb[0] as f32 / 255.0,
            rgb[1] as f32 / 255.0,
            rgb[2] as f32 / 255.0,
        ];
        let hue_rgb = hsl_to_rgb_f32(HslColor {
            h: hue,
            s: 1.0,
            l: 0.5,
        });

        let mut sum_h = 0.0f32;
        let mut sum_h2 = 0.0f32;
        let mut sum_rgb = 0.0f32;
        let mut sum_h_rgb = 0.0f32;
        for i in 0..3 {
            sum_h += hue_rgb[i];
            sum_h2 += hue_rgb[i] * hue_rgb[i];
            sum_rgb += rgb[i];
            sum_h_rgb += hue_rgb[i] * rgb[i];
        }
        let n = 3.0f32;
        let denom = (sum_h2 * n) - (sum_h * sum_h);
        let (wa, wb) = if denom.abs() < 0.000_01 {
            (0.0, (sum_rgb / n).clamp(0.0, 1.0))
        } else {
            let wa = ((n * sum_h_rgb) - (sum_h * sum_rgb)) / denom;
            let wb = (sum_rgb - wa * sum_h) / n;
            (wa, wb)
        };
        let wa = wa.clamp(0.0, 1.0);
        let mut wb = wb.clamp(0.0, 1.0);
        if wa + wb > 1.0 {
            wb = 1.0 - wa;
        }
        let wc = (1.0 - wa - wb).clamp(0.0, 1.0);
        normalize_weights([wa, wb, wc])
    }

    fn build_palette_wheel_background(selected_hue: f32) -> slint::Image {
        let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(WHEEL_SIZE, WHEEL_SIZE);
        let buffer = pixels.make_mut_slice();
        let center = WHEEL_SIZE as f32 * 0.5;
        let (hue_vertex, white_vertex, black_vertex) = wheel_triangle_vertices(selected_hue);
        let hue_rgb = hsl_to_rgb_f32(HslColor {
            h: selected_hue,
            s: 1.0,
            l: 0.5,
        });

        for y in 0..WHEEL_SIZE {
            for x in 0..WHEEL_SIZE {
                let fx = x as f32 + 0.5;
                let fy = y as f32 + 0.5;
                let dx = fx - center;
                let dy = fy - center;
                let radius = (dx * dx + dy * dy).sqrt();
                let idx = (y * WHEEL_SIZE + x) as usize;

                if (HUE_RING_INNER..=HUE_RING_OUTER).contains(&radius) {
                    let hue = (dy.atan2(dx).to_degrees() + 90.0).rem_euclid(360.0);
                    let rgb = hsl_to_rgb(HslColor {
                        h: hue,
                        s: 1.0,
                        l: 0.5,
                    });
                    buffer[idx] = rgba_pixel(rgb[0], rgb[1], rgb[2], 255);
                    continue;
                }

                let Some((wa, wb, wc)) = barycentric(
                    Vec2 { x: fx, y: fy },
                    hue_vertex,
                    white_vertex,
                    black_vertex,
                ) else {
                    buffer[idx] = rgba_pixel(0, 0, 0, 0);
                    continue;
                };

                if wa >= -0.002 && wb >= -0.002 && wc >= -0.002 {
                    let sum = (wa + wb + wc).max(0.0001);
                    let wa = (wa / sum).clamp(0.0, 1.0);
                    let wb = (wb / sum).clamp(0.0, 1.0);
                    let rgb = [
                        ((hue_rgb[0] * wa + wb) * 255.0).round().clamp(0.0, 255.0) as u8,
                        ((hue_rgb[1] * wa + wb) * 255.0).round().clamp(0.0, 255.0) as u8,
                        ((hue_rgb[2] * wa + wb) * 255.0).round().clamp(0.0, 255.0) as u8,
                    ];
                    buffer[idx] = rgba_pixel(rgb[0], rgb[1], rgb[2], 255);
                } else {
                    buffer[idx] = rgba_pixel(0, 0, 0, 0);
                }
            }
        }

        slint::Image::from_rgba8(pixels)
    }

    fn hue_marker_position(hue_degrees: f32) -> Vec2 {
        let center = WHEEL_SIZE as f32 * 0.5;
        let theta = (hue_degrees - 90.0).to_radians();
        let marker_radius = (HUE_RING_INNER + HUE_RING_OUTER) * 0.5;
        Vec2 {
            x: center + theta.cos() * marker_radius,
            y: center + theta.sin() * marker_radius,
        }
    }

    fn wheel_triangle_vertices(hue_degrees: f32) -> (Vec2, Vec2, Vec2) {
        let center = WHEEL_SIZE as f32 * 0.5;
        let hue_theta = (hue_degrees - 90.0).to_radians();
        let white_theta = hue_theta + (2.0 * std::f32::consts::PI / 3.0);
        let black_theta = hue_theta - (2.0 * std::f32::consts::PI / 3.0);
        (
            Vec2 {
                x: center + hue_theta.cos() * TRIANGLE_RADIUS,
                y: center + hue_theta.sin() * TRIANGLE_RADIUS,
            },
            Vec2 {
                x: center + white_theta.cos() * TRIANGLE_RADIUS,
                y: center + white_theta.sin() * TRIANGLE_RADIUS,
            },
            Vec2 {
                x: center + black_theta.cos() * TRIANGLE_RADIUS,
                y: center + black_theta.sin() * TRIANGLE_RADIUS,
            },
        )
    }

    fn barycentric(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> Option<(f32, f32, f32)> {
        let v0 = Vec2 {
            x: b.x - a.x,
            y: b.y - a.y,
        };
        let v1 = Vec2 {
            x: c.x - a.x,
            y: c.y - a.y,
        };
        let v2 = Vec2 {
            x: p.x - a.x,
            y: p.y - a.y,
        };
        let denom = v0.x * v1.y - v1.x * v0.y;
        if denom.abs() < 0.000_01 {
            return None;
        }
        let b_weight = (v2.x * v1.y - v1.x * v2.y) / denom;
        let c_weight = (v0.x * v2.y - v2.x * v0.y) / denom;
        let a_weight = 1.0 - b_weight - c_weight;
        Some((a_weight, b_weight, c_weight))
    }

    fn hex_to_slint_color(raw: &str) -> Color {
        if let Some(rgb) = hex_to_rgb(raw) {
            Color::from_rgb_u8(rgb[0], rgb[1], rgb[2])
        } else {
            Color::from_rgb_u8(255, 255, 255)
        }
    }

    fn hex_to_rgb(raw: &str) -> Option<[u8; 3]> {
        let normalized = normalize_hex_color(raw)?;
        let hex = &normalized[1..];
        Some([
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
        ])
    }

    fn rgb_to_hex(rgb: [u8; 3]) -> String {
        format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
    }

    fn hsl_to_rgb(hsl: HslColor) -> [u8; 3] {
        let rgb = hsl_to_rgb_f32(hsl);
        [
            (rgb[0] * 255.0).round().clamp(0.0, 255.0) as u8,
            (rgb[1] * 255.0).round().clamp(0.0, 255.0) as u8,
            (rgb[2] * 255.0).round().clamp(0.0, 255.0) as u8,
        ]
    }

    fn hsl_to_rgb_f32(hsl: HslColor) -> [f32; 3] {
        let h = hsl.h.rem_euclid(360.0) / 360.0;
        let s = hsl.s.clamp(0.0, 1.0);
        let l = hsl.l.clamp(0.0, 1.0);

        if s <= f32::EPSILON {
            return [l, l, l];
        }

        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - (l * s)
        };
        let p = (2.0 * l) - q;
        [
            hue_to_rgb(p, q, h + (1.0 / 3.0)),
            hue_to_rgb(p, q, h),
            hue_to_rgb(p, q, h - (1.0 / 3.0)),
        ]
    }

    fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 1.0 / 2.0 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    }

    fn rgb_to_hsl(rgb: [u8; 3]) -> HslColor {
        let r = rgb[0] as f32 / 255.0;
        let g = rgb[1] as f32 / 255.0;
        let b = rgb[2] as f32 / 255.0;
        let max = r.max(g.max(b));
        let min = r.min(g.min(b));
        let delta = max - min;
        let l = (max + min) * 0.5;

        if delta <= f32::EPSILON {
            return HslColor { h: 0.0, s: 0.0, l };
        }

        let s = delta / (1.0 - (2.0 * l - 1.0).abs());
        let mut h = if (max - r).abs() < f32::EPSILON {
            ((g - b) / delta).rem_euclid(6.0)
        } else if (max - g).abs() < f32::EPSILON {
            ((b - r) / delta) + 2.0
        } else {
            ((r - g) / delta) + 4.0
        };
        h *= 60.0;
        HslColor {
            h: h.rem_euclid(360.0),
            s: s.clamp(0.0, 1.0),
            l: l.clamp(0.0, 1.0),
        }
    }

    fn rgba_pixel(r: u8, g: u8, b: u8, a: u8) -> Rgba8Pixel {
        Rgba8Pixel { r, g, b, a }
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
        #[serde(default = "default_annotation_palette")]
        annotation_palette: [String; PALETTE_SIZE],
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
                annotation_palette: default_annotation_palette(),
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

    fn default_annotation_palette() -> [String; PALETTE_SIZE] {
        [
            "#FF5E5E".to_string(),
            "#FFAA43".to_string(),
            "#FFE256".to_string(),
            "#5FD382".to_string(),
            "#48B4FF".to_string(),
            "#5A80FF".to_string(),
            "#B67AFF".to_string(),
            "#FFFFFF".to_string(),
        ]
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
