# User Guide

## 1. Getting Started

Run the tray app:

```powershell
pyro.exe run
```

Default behavior:

- Press `PrintScreen` to start a region capture.
- After region selection, the editor opens immediately.

Install from MSI (recommended):

```powershell
msiexec /i pyro-<version>-windows-x64.msi
```

## 2. Capture Modes

One-off capture command:

```powershell
pyro.exe capture --target <region|primary|all-displays> [--delay-ms 500] [--edit]
```

Notes:

- `--edit` only applies to `region`.
- Save path can be provided with `--output <path>`.
- Clipboard behavior can be forced with `--clipboard` or `--no-clipboard`.

## 3. Region Editor

### Tools (default shortcuts)

- Select: `S`
- Rectangle: `R`
- Ellipse: `E`
- Line: `L`
- Arrow: `A`
- Marker (highlighter): `M`
- Text: `T`
- Pixelate: `P`
- Blur: `B`

### Common controls

- Draw/select region: left mouse drag
- Adjust selection/annotations: drag resize handles
- Change stroke thickness: mouse wheel
- Open radial color menu: right mouse button
- Undo/redo: `Ctrl+Z` / `Ctrl+Y`
- Delete selected annotation: `Delete`
- Copy: `Ctrl+C`
- Save: `Ctrl+S`
- Copy + Save: `Ctrl+Shift+S`
- Cancel editor: `Esc`

### Modifiers

- `Shift` + line/arrow/marker: snap angle to 5-degree steps
- `Shift` + ellipse: constrain to equal width and height

## 4. Text Tool

- Click and drag to place text area.
- After placement, enter your text.
- `Shift+Enter` inserts a newline.
- `Esc` or clicking outside commits text editing.

## 5. Pinned Capture

From editor actions, choose Pin to open a floating always-on-top capture window.

- Drag anywhere on the image to move.
- Mouse wheel to zoom.
- Right-click for pin actions.
- `Ctrl+C` and `Ctrl+S` work on the pinned window.
- `Esc` closes the pinned window.

## 6. Settings

Open settings:

```powershell
pyro.exe settings
```

Settings include:

- Global capture hotkey (record mode in-field)
- Default target and behavior
- Editor shortcuts
- Annotation palette swatches (HSL wheel picker)
- Radial color menu animation speed
- Filename template

Config file location:

- `%APPDATA%\\pyro\\config.toml`

## 7. Filename Template Tokens

Supported tokens:

- `%C` century (`00-99`)
- `%j` day of year (`001-366`)
- `%d` day (`01-31`)
- `%e` day (`1-31`)
- `%F` full date (`%Y-%m-%d`)
- `%H` hour (`00-23`)
- `%I` hour (`01-12`)
- `%M` minute (`00-59`)
- `%m` month (`01-12`)
- `%S` second (`00-59`)
- `%U` week number, Sunday start (`00-53`)
- `%u` weekday (`1-7`, Monday start)
- `%y` year (`00-99`)
- `%Y` year (`4-digit`)
- `%%` literal percent

## 8. Monitor Diagnostics

Use this command to inspect monitor layout and DPI:

```powershell
pyro.exe monitors [--json] [--validate] [--report <path>]
```

Useful validation flags:

- `--expect-count <N>`
- `--max-gap-px <N>`
- `--max-overlap-px <N>`
- `--compare-report <baseline.json>`

## 9. Troubleshooting

- If `settings` fails, ensure `pyro-settings.exe` exists beside `pyro.exe`.
- You can override the settings binary path with `PYRO_SETTINGS_EXE`.
- For source runs, build both binaries:

```powershell
cargo build --release --locked --bin pyro --bin pyro-settings
```
