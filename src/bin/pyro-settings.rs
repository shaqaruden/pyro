#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
mod windows_app {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::env;
    use std::fmt::Write as _;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use anyhow::{Context, Result};
    use serde::{Deserialize, Serialize};
    use slint::{
        Color, ComponentHandle, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer, TimerMode,
    };
    use time::{Date, Month, OffsetDateTime};
    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_MENU, VK_SHIFT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, FindWindowW, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, PostMessageW,
        SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_KEYUP,
        WM_SYSKEYDOWN, WM_SYSKEYUP,
    };
    use windows::core::{PCWSTR, w};

    const PALETTE_SIZE: usize = 8;
    const SETTINGS_WINDOW_DEFAULT_WIDTH: i32 = 980;
    const SETTINGS_WINDOW_DEFAULT_HEIGHT: i32 = 860;
    const WHEEL_SIZE: u32 = 220;
    const HUE_RING_OUTER: f32 = 106.0;
    const HUE_RING_INNER: f32 = 82.0;
    const TRIANGLE_RADIUS: f32 = 70.0;
    const HUE_WHEEL_INTERACTIVE_MIN_FRAME_MS: u64 = 90;
    const HUE_WHEEL_INTERACTIVE_MIN_DEGREE_DELTA: i32 = 8;
    const VK_PRINTSCREEN: u32 = 0x2C;
    const HOTKEY_RELOAD_MESSAGE: u32 = WM_APP + 101;

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
            TabWidget,
            VerticalBox,
            GridBox
        } from "std-widgets.slint";

        component ShortcutCaptureField inherits Rectangle {
            in property <string> label;
            in property <string> value;
            in property <bool> recording;
            in property <string> recording_display;
            in property <bool> has_error;
            in property <string> helper_text;
            callback activated();

            min-height: 50px;

            VerticalBox {
                padding: 0px;
                spacing: 4px;

                if root.label != "": Text {
                    text: root.label;
                    color: #9da3ae;
                    vertical-alignment: center;
                    overflow: elide;
                }

                input_box := Rectangle {
                    horizontal-stretch: 1;
                    height: 30px;
                    border-width: 1px;
                    border-color: has_error ? #ff6b6b : recording ? #89c4ff : #4a4f57;
                    border-radius: 4px;
                    background: has_error ? #331616 : recording ? #14263f : #1f2125;

                    Text {
                        x: 10px;
                        y: 0;
                        width: parent.width - (recording ? 86px : 20px);
                        height: parent.height;
                        text: recording ? recording_display : value;
                        color: has_error ? #ffd6d6 : recording ? #eaf3ff : #d9dde6;
                        vertical-alignment: center;
                        overflow: elide;
                    }

                    if recording: Rectangle {
                        x: parent.width - self.width - 8px;
                        y: (parent.height - self.height) / 2;
                        width: 56px;
                        height: 18px;
                        border-width: 1px;
                        border-color: #5f9df0;
                        border-radius: 9px;
                        background: #1d3c66;
                        Text {
                            width: parent.width;
                            height: parent.height;
                            text: "REC";
                            color: #d8ebff;
                            horizontal-alignment: center;
                            vertical-alignment: center;
                            font-size: 11px;
                        }
                    }

                    TouchArea {
                        width: parent.width;
                        height: parent.height;
                        clicked => { root.activated(); }
                    }
                }

                Text {
                    text: has_error ? helper_text : " ";
                    color: has_error ? #ff8f8f : #00000000;
                    overflow: elide;
                    font-size: 11px;
                }
            }
        }

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
            in-out property <string> filename_template;
            in-out property <string> filename_preview;
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
            in-out property <bool> shortcut_recording_active;
            in-out property <int> shortcut_recording_field;
            in-out property <string> shortcut_recorder_display;
            in-out property <int> shortcut_error_field;
            in-out property <string> shortcut_error_message;

            callback save_requested();
            callback reload_requested();
            callback close_requested();
            callback filename_template_changed(value: string);
            callback filename_template_insert_token(token: string);
            callback shortcut_record_requested(field: int);
            callback shortcut_record_key_pressed(key_text: string, ctrl: bool, shift: bool, alt: bool);
            callback shortcut_record_commit();
            callback shortcut_record_cancel();
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
                    if (root.shortcut_recording_active) {
                        if (event.text == Key.Escape) {
                            root.shortcut_record_cancel();
                            return accept;
                        }
                        if (event.text == Key.Return) {
                            root.shortcut_record_commit();
                            return accept;
                        }
                        root.shortcut_record_key_pressed(event.text, event.modifiers.control, event.modifiers.shift, event.modifiers.alt);
                        return accept;
                    }
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

                TabWidget {
                    vertical-stretch: 1;

                    Tab {
                        title: "Capture";
                        ScrollView {
                            GroupBox {
                            title: "Capture Defaults";
                            VerticalBox {
                                spacing: 8px;

                                HorizontalBox {
                                    spacing: 8px;
                                    ShortcutCaptureField {
                                        horizontal-stretch: 1;
                                        max-height: 32px;
                                        label: "Capture Hotkey";
                                        value: root.capture_hotkey;
                                        recording: root.shortcut_recording_active && root.shortcut_recording_field == 0;
                                        recording_display: root.shortcut_recorder_display;
                                        has_error: root.shortcut_error_field == 0;
                                        helper_text: root.shortcut_error_message;
                                        activated => { root.shortcut_record_requested(0); keyboard_scope.focus(); }
                                    }
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
                    }
                }

                    Tab {
                        title: "Filename";
                        ScrollView {
                            GroupBox {
                            title: "Filename";
                            VerticalBox {
                                spacing: 8px;

                                HorizontalBox {
                                    spacing: 8px;
                                    Text { text: "Filename Pattern"; width: 220px; }
                                    filename_pattern_input := LineEdit {
                                        text <=> root.filename_template;
                                        edited => { root.filename_template_changed(self.text); }
                                    }
                                }

                                Text {
                                    text: "Insert date/time tokens";
                                    color: #9da3ae;
                                }

                                HorizontalBox {
                                    spacing: 8px;
                                    Button {
                                        width: 250px;
                                        text: "Century (00-99)";
                                        clicked => { root.filename_template_insert_token("%C"); filename_pattern_input.focus(); filename_pattern_input.set-selection-offsets(2147483647, 2147483647); }
                                    }
                                    Button {
                                        width: 250px;
                                        text: "Day (001-366)";
                                        clicked => { root.filename_template_insert_token("%j"); filename_pattern_input.focus(); filename_pattern_input.set-selection-offsets(2147483647, 2147483647); }
                                    }
                                }

                                HorizontalBox {
                                    spacing: 8px;
                                    Button {
                                        width: 250px;
                                        text: "Day (01-31)";
                                        clicked => { root.filename_template_insert_token("%d"); filename_pattern_input.focus(); filename_pattern_input.set-selection-offsets(2147483647, 2147483647); }
                                    }
                                    Button {
                                        width: 250px;
                                        text: "Day of Month (1-31)";
                                        clicked => { root.filename_template_insert_token("%e"); filename_pattern_input.focus(); filename_pattern_input.set-selection-offsets(2147483647, 2147483647); }
                                    }
                                }

                                HorizontalBox {
                                    spacing: 8px;
                                    Button {
                                        width: 250px;
                                        text: "Full Date (%Y-%m-%d)";
                                        clicked => { root.filename_template_insert_token("%Y-%m-%d"); filename_pattern_input.focus(); filename_pattern_input.set-selection-offsets(2147483647, 2147483647); }
                                    }
                                    Button {
                                        width: 250px;
                                        text: "Full Date (%d-%m-%Y)";
                                        clicked => { root.filename_template_insert_token("%d-%m-%Y"); filename_pattern_input.focus(); filename_pattern_input.set-selection-offsets(2147483647, 2147483647); }
                                    }
                                }

                                HorizontalBox {
                                    spacing: 8px;
                                    Button {
                                        width: 250px;
                                        text: "Hour (00-23)";
                                        clicked => { root.filename_template_insert_token("%H"); filename_pattern_input.focus(); filename_pattern_input.set-selection-offsets(2147483647, 2147483647); }
                                    }
                                    Button {
                                        width: 250px;
                                        text: "Hour (01-12)";
                                        clicked => { root.filename_template_insert_token("%I"); filename_pattern_input.focus(); filename_pattern_input.set-selection-offsets(2147483647, 2147483647); }
                                    }
                                }

                                HorizontalBox {
                                    spacing: 8px;
                                    Button {
                                        width: 250px;
                                        text: "Minute (00-59)";
                                        clicked => { root.filename_template_insert_token("%M"); filename_pattern_input.focus(); filename_pattern_input.set-selection-offsets(2147483647, 2147483647); }
                                    }
                                    Button {
                                        width: 250px;
                                        text: "Month (01-12)";
                                        clicked => { root.filename_template_insert_token("%m"); filename_pattern_input.focus(); filename_pattern_input.set-selection-offsets(2147483647, 2147483647); }
                                    }
                                }

                                HorizontalBox {
                                    spacing: 8px;
                                    Button {
                                        width: 250px;
                                        text: "Second (00-59)";
                                        clicked => { root.filename_template_insert_token("%S"); filename_pattern_input.focus(); filename_pattern_input.set-selection-offsets(2147483647, 2147483647); }
                                    }
                                    Button {
                                        width: 250px;
                                        text: "Week (01-53)";
                                        clicked => { root.filename_template_insert_token("%V"); filename_pattern_input.focus(); filename_pattern_input.set-selection-offsets(2147483647, 2147483647); }
                                    }
                                }

                                HorizontalBox {
                                    spacing: 8px;
                                    Button {
                                        width: 250px;
                                        text: "Week Day (1-7)";
                                        clicked => { root.filename_template_insert_token("%u"); filename_pattern_input.focus(); filename_pattern_input.set-selection-offsets(2147483647, 2147483647); }
                                    }
                                    Button {
                                        width: 250px;
                                        text: "Year (00-99)";
                                        clicked => { root.filename_template_insert_token("%y"); filename_pattern_input.focus(); filename_pattern_input.set-selection-offsets(2147483647, 2147483647); }
                                    }
                                }

                                HorizontalBox {
                                    spacing: 8px;
                                    Button {
                                        width: 250px;
                                        text: "Year (2000)";
                                        clicked => { root.filename_template_insert_token("%Y"); filename_pattern_input.focus(); filename_pattern_input.set-selection-offsets(2147483647, 2147483647); }
                                    }
                                }

                                HorizontalBox {
                                    spacing: 8px;
                                    Text { text: "Filename Preview"; width: 220px; }
                                    LineEdit {
                                        read-only: true;
                                        text: root.filename_preview;
                                    }
                                }
                            }
                        }
                    }
                }

                    Tab {
                        title: "Editor";
                        ScrollView {
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
                    }
                }

                    Tab {
                        title: "Shortcuts";

                        ScrollView {
                            GridBox {
                                spacing: 10px;
                                Row {
                                    ShortcutCaptureField {
                                        horizontal-stretch: 1;
                                        label: "Select";
                                        value: root.shortcut_select;
                                        recording: root.shortcut_recording_active && root.shortcut_recording_field == 1;
                                        recording_display: root.shortcut_recorder_display;
                                        has_error: root.shortcut_error_field == 1;
                                        helper_text: root.shortcut_error_message;
                                        activated => { root.shortcut_record_requested(1); keyboard_scope.focus(); }
                                    }

                                    ShortcutCaptureField {
                                        horizontal-stretch: 1;
                                        label: "Rectangle";
                                        value: root.shortcut_rectangle;
                                        recording: root.shortcut_recording_active && root.shortcut_recording_field == 2;
                                        recording_display: root.shortcut_recorder_display;
                                        has_error: root.shortcut_error_field == 2;
                                        helper_text: root.shortcut_error_message;
                                        activated => { root.shortcut_record_requested(2); keyboard_scope.focus(); }
                                    }

                                    ShortcutCaptureField {
                                        horizontal-stretch: 1;
                                        label: "Ellipse";
                                        value: root.shortcut_ellipse;
                                        recording: root.shortcut_recording_active && root.shortcut_recording_field == 3;
                                        recording_display: root.shortcut_recorder_display;
                                        has_error: root.shortcut_error_field == 3;
                                        helper_text: root.shortcut_error_message;
                                        activated => { root.shortcut_record_requested(3); keyboard_scope.focus(); }
                                    }
                                }

                                Row {
                                    ShortcutCaptureField {
                                        horizontal-stretch: 1;
                                        label: "Line";
                                        value: root.shortcut_line;
                                        recording: root.shortcut_recording_active && root.shortcut_recording_field == 4;
                                        recording_display: root.shortcut_recorder_display;
                                        has_error: root.shortcut_error_field == 4;
                                        helper_text: root.shortcut_error_message;
                                        activated => { root.shortcut_record_requested(4); keyboard_scope.focus(); }
                                    }

                                    ShortcutCaptureField {
                                        horizontal-stretch: 1;
                                        label: "Arrow";
                                        value: root.shortcut_arrow;
                                        recording: root.shortcut_recording_active && root.shortcut_recording_field == 5;
                                        recording_display: root.shortcut_recorder_display;
                                        has_error: root.shortcut_error_field == 5;
                                        helper_text: root.shortcut_error_message;
                                        activated => { root.shortcut_record_requested(5); keyboard_scope.focus(); }
                                    }

                                    ShortcutCaptureField {
                                        horizontal-stretch: 1;
                                        label: "Marker";
                                        value: root.shortcut_marker;
                                        recording: root.shortcut_recording_active && root.shortcut_recording_field == 6;
                                        recording_display: root.shortcut_recorder_display;
                                        has_error: root.shortcut_error_field == 6;
                                        helper_text: root.shortcut_error_message;
                                        activated => { root.shortcut_record_requested(6); keyboard_scope.focus(); }
                                    }
                                }

                                Row {
                                    ShortcutCaptureField {
                                        horizontal-stretch: 1;
                                        label: "Text";
                                        value: root.shortcut_text;
                                        recording: root.shortcut_recording_active && root.shortcut_recording_field == 7;
                                        recording_display: root.shortcut_recorder_display;
                                        has_error: root.shortcut_error_field == 7;
                                        helper_text: root.shortcut_error_message;
                                        activated => { root.shortcut_record_requested(7); keyboard_scope.focus(); }
                                    }

                                    ShortcutCaptureField {
                                        horizontal-stretch: 1;
                                        label: "Pixelate";
                                        value: root.shortcut_pixelate;
                                        recording: root.shortcut_recording_active && root.shortcut_recording_field == 8;
                                        recording_display: root.shortcut_recorder_display;
                                        has_error: root.shortcut_error_field == 8;
                                        helper_text: root.shortcut_error_message;
                                        activated => { root.shortcut_record_requested(8); keyboard_scope.focus(); }
                                    }

                                    ShortcutCaptureField {
                                        horizontal-stretch: 1;
                                        label: "Blur";
                                        value: root.shortcut_blur;
                                        recording: root.shortcut_recording_active && root.shortcut_recording_field == 9;
                                        recording_display: root.shortcut_recorder_display;
                                        has_error: root.shortcut_error_field == 9;
                                        helper_text: root.shortcut_error_message;
                                        activated => { root.shortcut_record_requested(9); keyboard_scope.focus(); }
                                    }
                                }

                                Row {
                                    ShortcutCaptureField {
                                        horizontal-stretch: 1;
                                        label: "Copy";
                                        value: root.shortcut_copy;
                                        recording: root.shortcut_recording_active && root.shortcut_recording_field == 10;
                                        recording_display: root.shortcut_recorder_display;
                                        has_error: root.shortcut_error_field == 10;
                                        helper_text: root.shortcut_error_message;
                                        activated => { root.shortcut_record_requested(10); keyboard_scope.focus(); }
                                    }

                                    ShortcutCaptureField {
                                        horizontal-stretch: 1;
                                        label: "Save";
                                        value: root.shortcut_save;
                                        recording: root.shortcut_recording_active && root.shortcut_recording_field == 11;
                                        recording_display: root.shortcut_recorder_display;
                                        has_error: root.shortcut_error_field == 11;
                                        helper_text: root.shortcut_error_message;
                                        activated => { root.shortcut_record_requested(11); keyboard_scope.focus(); }
                                    }

                                    ShortcutCaptureField {
                                        horizontal-stretch: 1;
                                        label: "Copy+Save";
                                        value: root.shortcut_copy_and_save;
                                        recording: root.shortcut_recording_active && root.shortcut_recording_field == 12;
                                        recording_display: root.shortcut_recorder_display;
                                        has_error: root.shortcut_error_field == 12;
                                        helper_text: root.shortcut_error_message;
                                        activated => { root.shortcut_record_requested(12); keyboard_scope.focus(); }
                                    }
                                }

                                Row {
                                    ShortcutCaptureField {
                                        horizontal-stretch: 1;
                                        label: "Undo";
                                        value: root.shortcut_undo;
                                        recording: root.shortcut_recording_active && root.shortcut_recording_field == 13;
                                        recording_display: root.shortcut_recorder_display;
                                        has_error: root.shortcut_error_field == 13;
                                        helper_text: root.shortcut_error_message;
                                        activated => { root.shortcut_record_requested(13); keyboard_scope.focus(); }
                                    }

                                    ShortcutCaptureField {
                                        horizontal-stretch: 1;
                                        label: "Redo";
                                        value: root.shortcut_redo;
                                        recording: root.shortcut_recording_active && root.shortcut_recording_field == 14;
                                        recording_display: root.shortcut_recorder_display;
                                        has_error: root.shortcut_error_field == 14;
                                        helper_text: root.shortcut_error_message;
                                        activated => { root.shortcut_record_requested(14); keyboard_scope.focus(); }
                                    }

                                    ShortcutCaptureField {
                                        horizontal-stretch: 1;
                                        label: "Delete";
                                        value: root.shortcut_delete_selected;
                                        recording: root.shortcut_recording_active && root.shortcut_recording_field == 15;
                                        recording_display: root.shortcut_recorder_display;
                                        has_error: root.shortcut_error_field == 15;
                                        helper_text: root.shortcut_error_message;
                                        activated => { root.shortcut_record_requested(15); keyboard_scope.focus(); }
                                    }
                                }
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
        ring_pixels: Option<Vec<Rgba8Pixel>>,
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

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    enum ShortcutField {
        CaptureHotkey,
        Select,
        Rectangle,
        Ellipse,
        Line,
        Arrow,
        Marker,
        Text,
        Pixelate,
        Blur,
        Copy,
        Save,
        CopyAndSave,
        Undo,
        Redo,
        DeleteSelected,
    }

    impl ShortcutField {
        fn from_id(id: i32) -> Option<Self> {
            match id {
                0 => Some(Self::CaptureHotkey),
                1 => Some(Self::Select),
                2 => Some(Self::Rectangle),
                3 => Some(Self::Ellipse),
                4 => Some(Self::Line),
                5 => Some(Self::Arrow),
                6 => Some(Self::Marker),
                7 => Some(Self::Text),
                8 => Some(Self::Pixelate),
                9 => Some(Self::Blur),
                10 => Some(Self::Copy),
                11 => Some(Self::Save),
                12 => Some(Self::CopyAndSave),
                13 => Some(Self::Undo),
                14 => Some(Self::Redo),
                15 => Some(Self::DeleteSelected),
                _ => None,
            }
        }

        fn id(self) -> i32 {
            match self {
                Self::CaptureHotkey => 0,
                Self::Select => 1,
                Self::Rectangle => 2,
                Self::Ellipse => 3,
                Self::Line => 4,
                Self::Arrow => 5,
                Self::Marker => 6,
                Self::Text => 7,
                Self::Pixelate => 8,
                Self::Blur => 9,
                Self::Copy => 10,
                Self::Save => 11,
                Self::CopyAndSave => 12,
                Self::Undo => 13,
                Self::Redo => 14,
                Self::DeleteSelected => 15,
            }
        }

        fn label(self) -> &'static str {
            match self {
                Self::CaptureHotkey => "Capture Hotkey",
                Self::Select => "Select",
                Self::Rectangle => "Rectangle",
                Self::Ellipse => "Ellipse",
                Self::Line => "Line",
                Self::Arrow => "Arrow",
                Self::Marker => "Marker",
                Self::Text => "Text",
                Self::Pixelate => "Pixelate",
                Self::Blur => "Blur",
                Self::Copy => "Copy",
                Self::Save => "Save",
                Self::CopyAndSave => "Copy+Save",
                Self::Undo => "Undo",
                Self::Redo => "Redo",
                Self::DeleteSelected => "Delete Selected",
            }
        }

        fn is_capture_hotkey(self) -> bool {
            matches!(self, Self::CaptureHotkey)
        }

        fn is_editor_shortcut(self) -> bool {
            !self.is_capture_hotkey()
        }

        fn editor_fields() -> &'static [ShortcutField] {
            &[
                Self::Select,
                Self::Rectangle,
                Self::Ellipse,
                Self::Line,
                Self::Arrow,
                Self::Marker,
                Self::Text,
                Self::Pixelate,
                Self::Blur,
                Self::Copy,
                Self::Save,
                Self::CopyAndSave,
                Self::Undo,
                Self::Redo,
                Self::DeleteSelected,
            ]
        }
    }

    #[derive(Debug, Default, Clone)]
    struct ShortcutRecorderState {
        active: Option<ShortcutField>,
        pending: Option<KeyChord>,
    }

    struct PrintScreenRecorderBridge {
        ui: slint::Weak<SettingsWindow>,
        recorder_state: Rc<RefCell<ShortcutRecorderState>>,
    }

    thread_local! {
        static PRINTSCREEN_RECORDER_BRIDGE: RefCell<Option<PrintScreenRecorderBridge>> = const { RefCell::new(None) };
        static PRINTSCREEN_RECORDER_KEY_DOWN: RefCell<bool> = const { RefCell::new(false) };
    }

    struct PrintScreenHookGuard {
        hook: HHOOK,
    }

    impl Drop for PrintScreenHookGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = UnhookWindowsHookEx(self.hook);
            }
            PRINTSCREEN_RECORDER_BRIDGE.with(|slot| {
                *slot.borrow_mut() = None;
            });
            PRINTSCREEN_RECORDER_KEY_DOWN.with(|state| {
                *state.borrow_mut() = false;
            });
        }
    }

    pub fn run() -> Result<()> {
        let config_path = resolve_config_path()?;
        let ui = SettingsWindow::new().context("create settings window")?;
        let palette_state = Rc::new(RefCell::new(PaletteUiState::from_colors(
            default_annotation_palette(),
        )));
        let recorder_state = Rc::new(RefCell::new(ShortcutRecorderState::default()));
        let render_cache = Rc::new(RefCell::new(PaletteRenderCache {
            hue_bucket: -1,
            image: None,
            last_interactive_update: None,
            ring_pixels: None,
        }));

        ui.set_config_path(config_path.display().to_string().into());
        ui.set_shortcut_recording_active(false);
        ui.set_shortcut_recording_field(-1);
        ui.set_shortcut_recorder_display("Recording...".into());
        ui.set_shortcut_error_field(-1);
        ui.set_shortcut_error_message("".into());
        apply_loaded_config(&ui, &config_path, &palette_state, &render_cache);
        center_settings_window(&ui);
        let _printscreen_hook =
            match install_printscreen_recording_hook(ui.as_weak(), recorder_state.clone()) {
                Ok(hook) => Some(hook),
                Err(err) => {
                    set_status(
                        &ui,
                        &format!(
                            "Shortcut recorder warning: PrintScreen capture unavailable ({err})."
                        ),
                        StatusKind::Warning,
                    );
                    None
                }
            };
        let printscreen_down_state = Rc::new(RefCell::new(false));
        let _printscreen_poll_timer = {
            let ui_handle = ui.as_weak();
            let recorder_state = recorder_state.clone();
            let down_state = printscreen_down_state.clone();
            let timer = Timer::default();
            timer.start(TimerMode::Repeated, Duration::from_millis(12), move || {
                let Some(ui) = ui_handle.upgrade() else {
                    return;
                };
                if !ui.get_shortcut_recording_active() {
                    *down_state.borrow_mut() = false;
                    return;
                }

                let state = unsafe { GetAsyncKeyState(VK_PRINTSCREEN as i32) };
                let is_down = state < 0;
                let pressed_since_last = (state & 1) != 0;

                let mut was_down = down_state.borrow_mut();
                let should_capture = (is_down && !*was_down) || pressed_since_last;
                if should_capture {
                    let ctrl = unsafe { GetAsyncKeyState(VK_CONTROL.0 as i32) } < 0;
                    let shift = unsafe { GetAsyncKeyState(VK_SHIFT.0 as i32) } < 0;
                    let alt = unsafe { GetAsyncKeyState(VK_MENU.0 as i32) } < 0;
                    handle_shortcut_record_key_event(
                        &ui,
                        &recorder_state,
                        "PrintScreen".to_string(),
                        ctrl,
                        shift,
                        alt,
                    );
                }
                *was_down = is_down;
            });
            timer
        };

        {
            let ui_handle = ui.as_weak();
            let recorder_state = recorder_state.clone();
            ui.on_shortcut_record_requested(move |field_id| {
                let Some(ui) = ui_handle.upgrade() else {
                    return;
                };
                let Some(field) = ShortcutField::from_id(field_id) else {
                    return;
                };

                recorder_state.borrow_mut().active = Some(field);
                recorder_state.borrow_mut().pending = None;
                ui.set_shortcut_recording_active(true);
                ui.set_shortcut_recording_field(field_id);
                ui.set_shortcut_recorder_display("Recording...".into());
                clear_shortcut_field_error(&ui);
                set_status(
                    &ui,
                    &format!(
                        "Recording {} shortcut. Press keys, Enter to commit, Esc to cancel.",
                        field.label()
                    ),
                    StatusKind::Neutral,
                );
            });
        }

        {
            let ui_handle = ui.as_weak();
            let recorder_state = recorder_state.clone();
            ui.on_shortcut_record_cancel(move || {
                let Some(ui) = ui_handle.upgrade() else {
                    return;
                };
                clear_shortcut_recorder(&ui, &recorder_state);
                set_status(&ui, "Shortcut recording canceled.", StatusKind::Neutral);
            });
        }

        {
            let ui_handle = ui.as_weak();
            let recorder_state = recorder_state.clone();
            ui.on_shortcut_record_commit(move || {
                let Some(ui) = ui_handle.upgrade() else {
                    return;
                };
                commit_shortcut_recording(&ui, &recorder_state);
            });
        }

        {
            let ui_handle = ui.as_weak();
            let recorder_state = recorder_state.clone();
            ui.on_shortcut_record_key_pressed(move |key_text, ctrl, shift, alt| {
                let Some(ui) = ui_handle.upgrade() else {
                    return;
                };
                handle_shortcut_record_key_event(
                    &ui,
                    &recorder_state,
                    key_text.to_string(),
                    ctrl,
                    shift,
                    alt,
                );
            });
        }

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
                        Ok(()) => {
                            notify_hotkey_reload();
                            set_status(
                                &ui,
                                "Saved. Running app hotkey was signaled to reload.",
                                StatusKind::Success,
                            );
                        }
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
                clear_shortcut_field_error(&ui);
                apply_loaded_config(&ui, &config_path, &palette_state, &render_cache);
            });
        }

        {
            let ui_handle = ui.as_weak();
            ui.on_filename_template_changed(move |value| {
                let Some(ui) = ui_handle.upgrade() else {
                    return;
                };
                let preview = preview_filename(&value.to_string());
                ui.set_filename_preview(preview.into());
            });
        }

        {
            let ui_handle = ui.as_weak();
            ui.on_filename_template_insert_token(move |token| {
                let Some(ui) = ui_handle.upgrade() else {
                    return;
                };
                let mut current = ui.get_filename_template().to_string();
                current.push_str(&token.to_string());
                let preview = preview_filename(&current);
                ui.set_filename_template(current.into());
                ui.set_filename_preview(preview.into());
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

    fn install_printscreen_recording_hook(
        ui: slint::Weak<SettingsWindow>,
        recorder_state: Rc<RefCell<ShortcutRecorderState>>,
    ) -> Result<PrintScreenHookGuard> {
        PRINTSCREEN_RECORDER_BRIDGE.with(|slot| {
            *slot.borrow_mut() = Some(PrintScreenRecorderBridge { ui, recorder_state });
        });
        PRINTSCREEN_RECORDER_KEY_DOWN.with(|state| {
            *state.borrow_mut() = false;
        });

        let hook = unsafe {
            SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(printscreen_recording_hook_proc),
                HINSTANCE::default(),
                0,
            )
        }
        .context("SetWindowsHookExW(WH_KEYBOARD_LL) failed")?;

        Ok(PrintScreenHookGuard { hook })
    }

    unsafe extern "system" fn printscreen_recording_hook_proc(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code == HC_ACTION as i32 && lparam.0 != 0 {
            let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            let looks_like_printscreen = info.vkCode == VK_PRINTSCREEN || info.scanCode == 0x37;
            if looks_like_printscreen {
                let message = wparam.0 as u32;
                let is_down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
                let is_up = matches!(message, WM_KEYUP | WM_SYSKEYUP);

                if is_up {
                    PRINTSCREEN_RECORDER_KEY_DOWN.with(|state| {
                        *state.borrow_mut() = false;
                    });
                } else if is_down {
                    let first_down = PRINTSCREEN_RECORDER_KEY_DOWN.with(|state| {
                        let mut down = state.borrow_mut();
                        if *down {
                            false
                        } else {
                            *down = true;
                            true
                        }
                    });

                    if first_down {
                        let bridge = PRINTSCREEN_RECORDER_BRIDGE.with(|slot| {
                            slot.borrow()
                                .as_ref()
                                .map(|state| (state.ui.clone(), state.recorder_state.clone()))
                        });
                        if let Some((ui_handle, recorder_state)) = bridge
                            && let Some(ui) = ui_handle.upgrade()
                            && ui.get_shortcut_recording_active()
                        {
                            let ctrl = unsafe { GetAsyncKeyState(VK_CONTROL.0 as i32) } < 0;
                            let shift = unsafe { GetAsyncKeyState(VK_SHIFT.0 as i32) } < 0;
                            let alt = unsafe { GetAsyncKeyState(VK_MENU.0 as i32) } < 0;
                            handle_shortcut_record_key_event(
                                &ui,
                                &recorder_state,
                                "PrintScreen".to_string(),
                                ctrl,
                                shift,
                                alt,
                            );
                            return LRESULT(1);
                        }
                    }
                }
            }
        }

        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    fn center_settings_window(ui: &SettingsWindow) {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

        let window = ui.window();
        let size = window.size();
        let width = i32::try_from(size.width)
            .ok()
            .filter(|value| *value > 0)
            .unwrap_or(SETTINGS_WINDOW_DEFAULT_WIDTH);
        let height = i32::try_from(size.height)
            .ok()
            .filter(|value| *value > 0)
            .unwrap_or(SETTINGS_WINDOW_DEFAULT_HEIGHT);
        let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(1);
        let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) }.max(1);
        let x = ((screen_w - width) / 2).max(0);
        let y = ((screen_h - height) / 2).max(0);
        window.set_position(slint::PhysicalPosition::new(x, y));
    }

    fn handle_shortcut_record_key_event(
        ui: &SettingsWindow,
        recorder_state: &Rc<RefCell<ShortcutRecorderState>>,
        key_text: String,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) {
        if !ui.get_shortcut_recording_active() {
            return;
        }
        let Some(field) = recorder_state.borrow().active else {
            clear_shortcut_recorder(ui, recorder_state);
            return;
        };

        let normalized = key_text.trim();
        let upper = normalized.to_ascii_uppercase();
        if upper == "ESC" || upper == "ESCAPE" {
            clear_shortcut_recorder(ui, recorder_state);
            set_status(ui, "Shortcut recording canceled.", StatusKind::Neutral);
            return;
        }
        if upper == "ENTER" || upper == "RETURN" {
            commit_shortcut_recording(ui, recorder_state);
            return;
        }
        let Some(key) = parse_recorded_key_text(normalized, ctrl) else {
            return;
        };
        let chord = KeyChord {
            key,
            ctrl,
            shift,
            alt,
        };
        if field.is_capture_hotkey() && !capture_hotkey_chord_valid(chord) {
            recorder_state.borrow_mut().pending = None;
            ui.set_shortcut_recorder_display(format_key_chord(chord).into());
            set_shortcut_field_error(
                ui,
                field,
                "Capture hotkey requires Ctrl/Shift/Alt (or PrintScreen).",
            );
            set_status(
                ui,
                "Capture hotkey needs a modifier (or PrintScreen).",
                StatusKind::Warning,
            );
            return;
        }

        recorder_state.borrow_mut().pending = Some(chord);
        ui.set_shortcut_recorder_display(format_key_chord(chord).into());
        clear_shortcut_field_error(ui);
        set_status(
            ui,
            &format!(
                "Shortcut candidate: {}. Press Enter to commit or Esc to cancel.",
                format_key_chord(chord)
            ),
            StatusKind::Neutral,
        );
    }

    fn commit_shortcut_recording(
        ui: &SettingsWindow,
        recorder_state: &Rc<RefCell<ShortcutRecorderState>>,
    ) {
        let (field, pending) = {
            let state = recorder_state.borrow();
            (state.active, state.pending)
        };
        let Some(field) = field else {
            clear_shortcut_recorder(ui, recorder_state);
            return;
        };
        let Some(chord) = pending else {
            set_shortcut_field_error(ui, field, "Press a shortcut, then Enter to commit.");
            set_status(
                ui,
                "Press a shortcut first, then press Enter to commit.",
                StatusKind::Warning,
            );
            return;
        };

        let value = format_key_chord(chord);
        if field.is_editor_shortcut()
            && let Some(conflict_with) = find_editor_shortcut_conflict(ui, field, chord)
        {
            recorder_state.borrow_mut().pending = None;
            set_shortcut_field_error(ui, field, &format!("Already used by {}.", conflict_with));
            set_status(
                ui,
                &format!(
                    "`{value}` is already used by {}. Choose a different shortcut.",
                    conflict_with
                ),
                StatusKind::Warning,
            );
            return;
        }
        set_shortcut_field_value(ui, field, &value);
        clear_shortcut_field_error(ui);
        clear_shortcut_recorder(ui, recorder_state);
        set_status(
            ui,
            &format!(
                "Updated {} to `{value}`. Click Save to persist.",
                field.label()
            ),
            StatusKind::Neutral,
        );
    }

    fn clear_shortcut_recorder(
        ui: &SettingsWindow,
        recorder_state: &Rc<RefCell<ShortcutRecorderState>>,
    ) {
        {
            let mut state = recorder_state.borrow_mut();
            state.active = None;
            state.pending = None;
        }
        ui.set_shortcut_recording_active(false);
        ui.set_shortcut_recording_field(-1);
        ui.set_shortcut_recorder_display("Recording...".into());
    }

    fn capture_hotkey_chord_valid(chord: KeyChord) -> bool {
        (chord.ctrl || chord.shift || chord.alt) || chord.key == VK_PRINTSCREEN
    }

    fn set_shortcut_field_value(ui: &SettingsWindow, field: ShortcutField, value: &str) {
        match field {
            ShortcutField::CaptureHotkey => ui.set_capture_hotkey(value.into()),
            ShortcutField::Select => ui.set_shortcut_select(value.into()),
            ShortcutField::Rectangle => ui.set_shortcut_rectangle(value.into()),
            ShortcutField::Ellipse => ui.set_shortcut_ellipse(value.into()),
            ShortcutField::Line => ui.set_shortcut_line(value.into()),
            ShortcutField::Arrow => ui.set_shortcut_arrow(value.into()),
            ShortcutField::Marker => ui.set_shortcut_marker(value.into()),
            ShortcutField::Text => ui.set_shortcut_text(value.into()),
            ShortcutField::Pixelate => ui.set_shortcut_pixelate(value.into()),
            ShortcutField::Blur => ui.set_shortcut_blur(value.into()),
            ShortcutField::Copy => ui.set_shortcut_copy(value.into()),
            ShortcutField::Save => ui.set_shortcut_save(value.into()),
            ShortcutField::CopyAndSave => ui.set_shortcut_copy_and_save(value.into()),
            ShortcutField::Undo => ui.set_shortcut_undo(value.into()),
            ShortcutField::Redo => ui.set_shortcut_redo(value.into()),
            ShortcutField::DeleteSelected => ui.set_shortcut_delete_selected(value.into()),
        }
    }

    fn set_shortcut_field_error(ui: &SettingsWindow, field: ShortcutField, message: &str) {
        ui.set_shortcut_error_field(field.id());
        ui.set_shortcut_error_message(message.into());
    }

    fn clear_shortcut_field_error(ui: &SettingsWindow) {
        ui.set_shortcut_error_field(-1);
        ui.set_shortcut_error_message("".into());
    }

    fn get_shortcut_field_value(ui: &SettingsWindow, field: ShortcutField) -> String {
        match field {
            ShortcutField::CaptureHotkey => ui.get_capture_hotkey().to_string(),
            ShortcutField::Select => ui.get_shortcut_select().to_string(),
            ShortcutField::Rectangle => ui.get_shortcut_rectangle().to_string(),
            ShortcutField::Ellipse => ui.get_shortcut_ellipse().to_string(),
            ShortcutField::Line => ui.get_shortcut_line().to_string(),
            ShortcutField::Arrow => ui.get_shortcut_arrow().to_string(),
            ShortcutField::Marker => ui.get_shortcut_marker().to_string(),
            ShortcutField::Text => ui.get_shortcut_text().to_string(),
            ShortcutField::Pixelate => ui.get_shortcut_pixelate().to_string(),
            ShortcutField::Blur => ui.get_shortcut_blur().to_string(),
            ShortcutField::Copy => ui.get_shortcut_copy().to_string(),
            ShortcutField::Save => ui.get_shortcut_save().to_string(),
            ShortcutField::CopyAndSave => ui.get_shortcut_copy_and_save().to_string(),
            ShortcutField::Undo => ui.get_shortcut_undo().to_string(),
            ShortcutField::Redo => ui.get_shortcut_redo().to_string(),
            ShortcutField::DeleteSelected => ui.get_shortcut_delete_selected().to_string(),
        }
    }

    fn find_editor_shortcut_conflict(
        ui: &SettingsWindow,
        field: ShortcutField,
        chord: KeyChord,
    ) -> Option<&'static str> {
        for other in ShortcutField::editor_fields() {
            if *other == field {
                continue;
            }
            let raw = get_shortcut_field_value(ui, *other);
            let Ok(existing) = parse_editor_shortcut(raw.trim()) else {
                continue;
            };
            if existing == chord {
                return Some(other.label());
            }
        }
        None
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
        ui.set_filename_template(config.filename_template.clone().into());
        ui.set_filename_preview(preview_filename(&config.filename_template).into());
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
        ui.set_shortcut_recording_active(false);
        ui.set_shortcut_recording_field(-1);
        ui.set_shortcut_recorder_display("Recording...".into());
        ui.set_shortcut_error_field(-1);
        ui.set_shortcut_error_message("".into());
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
        let filename_template = validate_filename_template(ui.get_filename_template())?;
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
        validate_capture_hotkey(&capture_hotkey)?;
        validate_editor_shortcuts(&shortcuts)?;

        Ok(AppConfig {
            capture_hotkey,
            default_target,
            default_delay_ms,
            copy_to_clipboard: ui.get_copy_to_clipboard(),
            open_editor: ui.get_open_editor(),
            save_dir: PathBuf::from(save_dir),
            filename_template,
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
            let ring_pixels = render_cache
                .ring_pixels
                .get_or_insert_with(build_palette_ring_background_pixels);
            render_cache.image = Some(build_palette_wheel_background(hue_degrees, ring_pixels));
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

    fn build_palette_ring_background_pixels() -> Vec<Rgba8Pixel> {
        let mut ring = vec![rgba_pixel(0, 0, 0, 0); (WHEEL_SIZE * WHEEL_SIZE) as usize];
        let center = WHEEL_SIZE as f32 * 0.5;
        for y in 0..WHEEL_SIZE {
            for x in 0..WHEEL_SIZE {
                let fx = x as f32 + 0.5;
                let fy = y as f32 + 0.5;
                let dx = fx - center;
                let dy = fy - center;
                let radius = (dx * dx + dy * dy).sqrt();
                if !(HUE_RING_INNER..=HUE_RING_OUTER).contains(&radius) {
                    continue;
                }
                let hue = (dy.atan2(dx).to_degrees() + 90.0).rem_euclid(360.0);
                let rgb = hsl_to_rgb(HslColor {
                    h: hue,
                    s: 1.0,
                    l: 0.5,
                });
                let idx = (y * WHEEL_SIZE + x) as usize;
                ring[idx] = rgba_pixel(rgb[0], rgb[1], rgb[2], 255);
            }
        }
        ring
    }

    fn build_palette_wheel_background(
        selected_hue: f32,
        ring_pixels: &[Rgba8Pixel],
    ) -> slint::Image {
        let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(WHEEL_SIZE, WHEEL_SIZE);
        let buffer = pixels.make_mut_slice();
        if ring_pixels.len() == buffer.len() {
            buffer.copy_from_slice(ring_pixels);
        } else {
            buffer.fill(rgba_pixel(0, 0, 0, 0));
        }

        let (hue_vertex, white_vertex, black_vertex) = wheel_triangle_vertices(selected_hue);
        let hue_rgb = hsl_to_rgb_f32(HslColor {
            h: selected_hue,
            s: 1.0,
            l: 0.5,
        });

        let min_x = hue_vertex
            .x
            .min(white_vertex.x)
            .min(black_vertex.x)
            .floor()
            .clamp(0.0, WHEEL_SIZE as f32 - 1.0) as u32;
        let max_x = hue_vertex
            .x
            .max(white_vertex.x)
            .max(black_vertex.x)
            .ceil()
            .clamp(0.0, WHEEL_SIZE as f32 - 1.0) as u32;
        let min_y = hue_vertex
            .y
            .min(white_vertex.y)
            .min(black_vertex.y)
            .floor()
            .clamp(0.0, WHEEL_SIZE as f32 - 1.0) as u32;
        let max_y = hue_vertex
            .y
            .max(white_vertex.y)
            .max(black_vertex.y)
            .ceil()
            .clamp(0.0, WHEEL_SIZE as f32 - 1.0) as u32;

        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let fx = x as f32 + 0.5;
                let fy = y as f32 + 0.5;
                let idx = (y * WHEEL_SIZE + x) as usize;

                let Some((wa, wb, wc)) = barycentric(
                    Vec2 { x: fx, y: fy },
                    hue_vertex,
                    white_vertex,
                    black_vertex,
                ) else {
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

    #[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
    struct KeyChord {
        key: u32,
        ctrl: bool,
        shift: bool,
        alt: bool,
    }

    fn validate_capture_hotkey(value: &str) -> std::result::Result<(), String> {
        parse_hotkey(value).map(|_| ())
    }

    fn parse_hotkey(value: &str) -> std::result::Result<KeyChord, String> {
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut win = false;
        let mut key = None::<u32>;

        for raw_token in value.split('+') {
            let token = raw_token.trim();
            if token.is_empty() {
                return Err("hotkey token cannot be empty".to_string());
            }
            let token_upper = token.to_ascii_uppercase();
            match token_upper.as_str() {
                "CTRL" | "CONTROL" => ctrl = true,
                "SHIFT" => shift = true,
                "ALT" => alt = true,
                "WIN" | "WINDOWS" | "META" => win = true,
                _ => {
                    if key.is_some() {
                        return Err("hotkey must include exactly one non-modifier key".to_string());
                    }
                    key = Some(parse_hotkey_key(&token_upper)?);
                }
            }
        }

        let Some(key) = key else {
            return Err("hotkey missing key".to_string());
        };
        if !(ctrl || shift || alt || win) && key != VK_PRINTSCREEN {
            return Err(
                "hotkey must include at least one modifier (except PrintScreen)".to_string(),
            );
        }

        Ok(KeyChord {
            key,
            ctrl,
            shift,
            alt,
        })
    }

    fn parse_hotkey_key(token: &str) -> std::result::Result<u32, String> {
        if token.len() == 1 {
            let ch = token.chars().next().expect("len checked");
            if ch.is_ascii_uppercase() || ch.is_ascii_digit() {
                return Ok(ch as u32);
            }
        }
        if let Some(number) = token.strip_prefix('F')
            && let Ok(value) = number.parse::<u32>()
            && (1..=24).contains(&value)
        {
            return Ok(111 + value);
        }
        match token {
            "PRINTSCREEN" | "PRTSC" | "PRTSCN" | "SNAPSHOT" | "SYSRQ" | "SYSREQ" | "PRINT" => {
                Ok(VK_PRINTSCREEN)
            }
            _ => Err(format!("unsupported key `{token}`")),
        }
    }

    fn validate_editor_shortcuts(
        shortcuts: &EditorShortcutConfig,
    ) -> std::result::Result<(), String> {
        let bindings = [
            ("Select", shortcuts.select.as_str()),
            ("Rectangle", shortcuts.rectangle.as_str()),
            ("Ellipse", shortcuts.ellipse.as_str()),
            ("Line", shortcuts.line.as_str()),
            ("Arrow", shortcuts.arrow.as_str()),
            ("Marker", shortcuts.marker.as_str()),
            ("Text", shortcuts.text.as_str()),
            ("Pixelate", shortcuts.pixelate.as_str()),
            ("Blur", shortcuts.blur.as_str()),
            ("Copy", shortcuts.copy.as_str()),
            ("Save", shortcuts.save.as_str()),
            ("Copy+Save", shortcuts.copy_and_save.as_str()),
            ("Undo", shortcuts.undo.as_str()),
            ("Redo", shortcuts.redo.as_str()),
            ("Delete Selected", shortcuts.delete_selected.as_str()),
        ];
        let mut seen = HashMap::<KeyChord, &'static str>::new();
        let mut conflicts = Vec::<String>::new();
        for (label, value) in bindings {
            let chord = parse_editor_shortcut(value)
                .map_err(|err| format!("{label} shortcut is invalid: {err}"))?;
            if let Some(previous) = seen.insert(chord, label) {
                conflicts.push(format!(
                    "{previous} + {label} => `{}`",
                    format_key_chord(chord)
                ));
            }
        }
        if !conflicts.is_empty() {
            return Err(format!(
                "Shortcut conflicts detected: {}.",
                conflicts.join(" | ")
            ));
        }
        Ok(())
    }

    fn parse_editor_shortcut(value: &str) -> std::result::Result<KeyChord, String> {
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut key = None::<u32>;

        for raw_token in value.split('+') {
            let token = raw_token.trim();
            if token.is_empty() {
                return Err("shortcut token cannot be empty".to_string());
            }
            let token_upper = token.to_ascii_uppercase();
            match token_upper.as_str() {
                "CTRL" | "CONTROL" => ctrl = true,
                "SHIFT" => shift = true,
                "ALT" => alt = true,
                _ => {
                    if key.is_some() {
                        return Err(
                            "shortcut must include exactly one non-modifier key".to_string()
                        );
                    }
                    key = Some(parse_editor_shortcut_key(&token_upper)?);
                }
            }
        }

        let Some(key) = key else {
            return Err("shortcut must include a key".to_string());
        };
        Ok(KeyChord {
            key,
            ctrl,
            shift,
            alt,
        })
    }

    fn parse_editor_shortcut_key(token: &str) -> std::result::Result<u32, String> {
        if token.len() == 1 {
            let ch = token.chars().next().expect("len checked");
            if ch.is_ascii_alphabetic() {
                return Ok(ch.to_ascii_uppercase() as u32);
            }
            if ch.is_ascii_digit() {
                return Ok(ch as u32);
            }
            return match ch {
                '[' => Ok(0xDB),
                ']' => Ok(0xDD),
                ';' => Ok(0xBA),
                '\'' => Ok(0xDE),
                ',' => Ok(0xBC),
                '.' => Ok(0xBE),
                '/' => Ok(0xBF),
                '-' => Ok(0xBD),
                '=' => Ok(0xBB),
                '`' => Ok(0xC0),
                '\\' => Ok(0xDC),
                _ => Err(format!("unsupported key `{token}`")),
            };
        }

        if let Some(number) = token.strip_prefix('F')
            && let Ok(value) = number.parse::<u32>()
            && (1..=24).contains(&value)
        {
            return Ok(111 + value);
        }

        match token {
            "DELETE" | "DEL" => Ok(0x2E),
            "ENTER" | "RETURN" => Ok(0x0D),
            "ESC" | "ESCAPE" => Ok(0x1B),
            "SPACE" => Ok(0x20),
            "TAB" => Ok(0x09),
            "BACKSPACE" | "BKSP" => Ok(0x08),
            "LEFTBRACKET" | "LBRACKET" => Ok(0xDB),
            "RIGHTBRACKET" | "RBRACKET" => Ok(0xDD),
            _ => Err(format!("unsupported key `{token}`")),
        }
    }

    fn parse_recorded_key_text(raw: &str, ctrl: bool) -> Option<u32> {
        let token = raw.trim();
        if token.is_empty() {
            return None;
        }
        if token.chars().count() == 1 {
            let ch = token.chars().next().expect("count checked");
            let code = ch as u32;
            if let Some(mapped) = map_slint_special_key_code(code) {
                return Some(mapped);
            }
            if ctrl && (1..=26).contains(&code) {
                return Some((code + 64) as u32);
            }
            if ctrl {
                // Common control-code aliases produced by Ctrl+number/punctuation on Windows.
                match code {
                    0x1C => return Some(u32::from(b'4')), // Ctrl+4
                    0x1D => return Some(u32::from(b'5')), // Ctrl+5
                    0x1E => return Some(u32::from(b'6')), // Ctrl+6
                    0x1F => return Some(0xBD),            // Ctrl+-
                    _ => {}
                }
            }
            match code {
                0x08 => return Some(0x08), // Backspace
                0x09 => return Some(0x09), // Tab
                0x0D => return Some(0x0D), // Enter
                0x1B => return Some(0x1B), // Esc
                0x20 => return Some(0x20), // Space
                0x7F => return Some(0x2E), // Delete
                _ => {}
            }
            if let Some(mapped) = map_shifted_symbol_key(ch) {
                return Some(mapped);
            }
        }
        let upper = token.to_ascii_uppercase();
        if matches!(
            upper.as_str(),
            "CTRL"
                | "CONTROL"
                | "SHIFT"
                | "ALT"
                | "META"
                | "WIN"
                | "WINDOWS"
                | "LEFTSHIFT"
                | "RIGHTSHIFT"
                | "LEFTCONTROL"
                | "RIGHTCONTROL"
                | "LEFTALT"
                | "RIGHTALT"
        ) {
            return None;
        }
        if matches!(upper.as_str(), "PLUS" | "ADD") {
            return Some(0xBB);
        }
        if upper == "MINUS" {
            return Some(0xBD);
        }
        if matches!(
            upper.as_str(),
            "PRINTSCREEN" | "PRTSC" | "PRTSCN" | "SNAPSHOT" | "SYSRQ" | "SYSREQ" | "PRINT"
        ) {
            return Some(VK_PRINTSCREEN);
        }
        parse_editor_shortcut_key(&upper).ok()
    }

    fn map_slint_special_key_code(code: u32) -> Option<u32> {
        match code {
            0xF700 => Some(0x26),                           // Up
            0xF701 => Some(0x28),                           // Down
            0xF702 => Some(0x25),                           // Left
            0xF703 => Some(0x27),                           // Right
            0xF704..=0xF71B => Some(112 + (code - 0xF704)), // F1..F24
            0xF727 => Some(0x2D),                           // Insert
            0xF729 => Some(0x24),                           // Home
            0xF72B => Some(0x23),                           // End
            0xF72C => Some(0x21),                           // PageUp
            0xF72D => Some(0x22),                           // PageDown
            0xF72E => Some(VK_PRINTSCREEN),                 // PrintScreen/Snapshot
            0xF72F => Some(0x91),                           // ScrollLock
            0xF730 => Some(0x13),                           // Pause
            0xF731 => Some(VK_PRINTSCREEN),                 // SysReq/PrintScreen
            0xF735 => Some(0x5D),                           // Context Menu
            _ => None,
        }
    }

    fn map_shifted_symbol_key(ch: char) -> Option<u32> {
        match ch {
            '!' => Some(u32::from(b'1')),
            '@' => Some(u32::from(b'2')),
            '#' => Some(u32::from(b'3')),
            '$' => Some(u32::from(b'4')),
            '%' => Some(u32::from(b'5')),
            '^' => Some(u32::from(b'6')),
            '&' => Some(u32::from(b'7')),
            '*' => Some(u32::from(b'8')),
            '(' => Some(u32::from(b'9')),
            ')' => Some(u32::from(b'0')),
            '_' => Some(0xBD), // -
            '+' => Some(0xBB), // =
            '{' => Some(0xDB), // [
            '}' => Some(0xDD), // ]
            ':' => Some(0xBA), // ;
            '"' => Some(0xDE), // '
            '<' => Some(0xBC), // ,
            '>' => Some(0xBE), // .
            '?' => Some(0xBF), // /
            '|' => Some(0xDC), // \
            '~' => Some(0xC0), // `
            _ => None,
        }
    }

    fn format_key_chord(chord: KeyChord) -> String {
        let mut parts = Vec::new();
        if chord.ctrl {
            parts.push("Ctrl".to_string());
        }
        if chord.shift {
            parts.push("Shift".to_string());
        }
        if chord.alt {
            parts.push("Alt".to_string());
        }
        parts.push(format_key_code(chord.key));
        parts.join("+")
    }

    fn format_key_code(key: u32) -> String {
        if (u32::from(b'A')..=u32::from(b'Z')).contains(&key)
            || (u32::from(b'0')..=u32::from(b'9')).contains(&key)
        {
            return (char::from_u32(key).unwrap_or('?')).to_string();
        }
        if (112..=135).contains(&key) {
            return format!("F{}", key - 111);
        }
        match key {
            0x2C => "PrintScreen".to_string(),
            0xDB => "[".to_string(),
            0xDD => "]".to_string(),
            0xBA => ";".to_string(),
            0xDE => "'".to_string(),
            0xBC => ",".to_string(),
            0xBE => ".".to_string(),
            0xBF => "/".to_string(),
            0xBD => "-".to_string(),
            0xBB => "=".to_string(),
            0xC0 => "`".to_string(),
            0xDC => "\\".to_string(),
            0x2E => "Delete".to_string(),
            0x0D => "Enter".to_string(),
            0x1B => "Esc".to_string(),
            0x20 => "Space".to_string(),
            0x09 => "Tab".to_string(),
            0x08 => "Backspace".to_string(),
            _ => format!("VK_{key:#X}"),
        }
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

    fn validate_filename_template(value: SharedString) -> std::result::Result<String, String> {
        let template = read_required("Filename pattern", value)?;
        let preview = preview_filename(&template);
        if preview.trim().is_empty() {
            return Err("Filename pattern produced an empty name.".to_string());
        }
        Ok(template)
    }

    fn preview_filename(template: &str) -> String {
        let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        let rendered = render_filename_template(template, now);
        let mut sanitized = sanitize_filename(&rendered);
        if Path::new(&sanitized).extension().is_none() {
            sanitized.push_str(".png");
        }
        sanitized
    }

    fn render_filename_template(template: &str, now: OffsetDateTime) -> String {
        let mut output = String::with_capacity(template.len() + 16);
        let mut chars = template.chars();
        while let Some(ch) = chars.next() {
            if ch != '%' {
                output.push(ch);
                continue;
            }

            let Some(spec) = chars.next() else {
                output.push('%');
                break;
            };

            match spec {
                '%' => output.push('%'),
                'Y' => {
                    let _ = write!(output, "{:04}", now.year());
                }
                'y' => {
                    let year = now.year().rem_euclid(100);
                    let _ = write!(output, "{year:02}");
                }
                'C' => {
                    let century = now.year().div_euclid(100);
                    let _ = write!(output, "{century:02}");
                }
                'm' => {
                    let _ = write!(output, "{:02}", u8::from(now.month()));
                }
                'd' => {
                    let _ = write!(output, "{:02}", now.day());
                }
                'e' => {
                    let _ = write!(output, "{}", now.day());
                }
                'H' => {
                    let _ = write!(output, "{:02}", now.hour());
                }
                'I' => {
                    let hour = match now.hour() % 12 {
                        0 => 12,
                        value => value,
                    };
                    let _ = write!(output, "{hour:02}");
                }
                'M' => {
                    let _ = write!(output, "{:02}", now.minute());
                }
                'S' => {
                    let _ = write!(output, "{:02}", now.second());
                }
                'j' => {
                    let _ = write!(output, "{:03}", now.ordinal());
                }
                'u' => {
                    let _ = write!(output, "{}", now.weekday().number_from_monday());
                }
                'V' => {
                    let _ = write!(output, "{:02}", now.date().iso_week());
                }
                'U' => {
                    let _ = write!(output, "{:02}", week_number_sunday_start(now));
                }
                'F' => {
                    let _ = write!(
                        output,
                        "{:04}-{:02}-{:02}",
                        now.year(),
                        u8::from(now.month()),
                        now.day()
                    );
                }
                _ => {
                    output.push('%');
                    output.push(spec);
                }
            }
        }

        if output.trim().is_empty() {
            return "capture".to_string();
        }
        output
    }

    fn week_number_sunday_start(now: OffsetDateTime) -> u8 {
        let date = now.date();
        let jan1 = Date::from_calendar_date(date.year(), Month::January, 1).unwrap_or(date);
        let jan1_weekday = jan1.weekday().number_days_from_sunday() as u16;
        let first_sunday_ordinal = if jan1_weekday == 0 {
            1
        } else {
            8 - jan1_weekday
        };
        let ordinal = date.ordinal() as u16;
        if ordinal < first_sunday_ordinal {
            return 0;
        }
        (((ordinal - first_sunday_ordinal) / 7) + 1) as u8
    }

    fn sanitize_filename(input: &str) -> String {
        let mut cleaned = String::with_capacity(input.len());
        for ch in input.chars() {
            let is_invalid = ch.is_control()
                || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*');
            cleaned.push(if is_invalid { '_' } else { ch });
        }

        let mut cleaned = cleaned
            .trim()
            .trim_end_matches(|ch| ch == ' ' || ch == '.')
            .to_string();
        if cleaned.is_empty() {
            cleaned = "capture".to_string();
        }
        if is_reserved_windows_name(&cleaned) {
            cleaned.push('_');
        }
        cleaned
    }

    fn is_reserved_windows_name(input: &str) -> bool {
        let stem = Path::new(input)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(input)
            .to_ascii_uppercase();
        matches!(
            stem.as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        )
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

    fn notify_hotkey_reload() {
        let Ok(hwnd) = (unsafe { FindWindowW(w!("PyroTrayWindowClass"), PCWSTR::null()) }) else {
            return;
        };
        if hwnd.0.is_null() {
            return;
        }
        let _ = unsafe { PostMessageW(hwnd, HOTKEY_RELOAD_MESSAGE, WPARAM(0), LPARAM(0)) };
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
        #[serde(default = "default_filename_template")]
        filename_template: String,
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
                filename_template: default_filename_template(),
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

    fn default_filename_template() -> String {
        "pyro-%Y%m%d-%H%M%S".to_string()
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
        "C".to_string()
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
    attach_parent_console();
    let handle = std::thread::Builder::new()
        .name("pyro-settings-ui".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(windows_app::run)
        .map_err(anyhow::Error::from)?;

    match handle.join() {
        Ok(result) => result,
        Err(_) => anyhow::bail!("settings UI thread panicked"),
    }
}

#[cfg(target_os = "windows")]
fn attach_parent_console() {
    use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("pyro-settings currently supports Windows only.");
}
