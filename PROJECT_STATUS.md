# Pyro Project Status

Last updated: March 2, 2026

## Project Goal
Build a Windows screenshot utility in Rust, similar to Flameshot, with reliable multi-monitor + mixed-DPI behavior and an interactive region editor.

## Current Progress
- Core app is working end-to-end:
  - CLI capture flow (`run` + `capture` commands)
  - Tray app with custom modern Win11-style popup menu
  - Global hotkey support
  - Settings window (`settings` command + tray action)
  - Save to PNG and copy to clipboard
  - Delayed capture
  - Region / primary display / all displays capture targets
- Settings UI stack:
  - Rust + Slint (`pyro-settings` binary)
  - Reads/writes the same `config.toml`
  - WinUI/.NET settings app removed from the repo
- DPI handling foundation is in place:
  - Process sets per-monitor DPI awareness v2
  - Virtual desktop coordinates are used across selection/editor
- Region flow is implemented:
  - Region selection overlay
  - Immediate transition to region editor
  - Region resizing/moving with handles and resize cursors
  - Confirm/cancel behavior
- Region editor currently supports:
  - `Select`
  - `Rectangle`
  - `Ellipse` (hold `Shift` to constrain to equal width/height)
  - `Line`
  - `Arrow`
  - `Marker` (implemented as translucent highlighter line, hold `Shift` to snap to 45°)
  - Color palette + thickness controls
  - Mouse wheel thickness adjustment
  - Undo/redo (`Ctrl+Z`, `Ctrl+Y`)
  - Annotation selection in select mode + `Delete` to remove selected annotation

## Recent Commits
- `064276b` feat: add ellipse and highlighter marker tools in region editor
- `065ccb4` feat: expand region editor with line and arrow annotations
- `a7a9519` feat: add region editor workflow with tray and overlay improvements
- `2f5a72f` feat: add hotkey tray runtime with modern themed menu
- `5063234` feat: bootstrap Windows screenshot utility core

## Not Implemented Yet (vs Flameshot-style target)
- Better keyboard shortcut conflict validation
- Richer settings UX polish (grouping, spacing, descriptions)
- Rich keyboard shortcut customization UI/config for all editor actions
- Deeper editor UX polish (tool icons, better layout density, etc.)

## Next Recommended Steps
1. Add `Text` tool (click-to-place, inline typing, apply/cancel).
2. Add `Blur` + `Pixelate` tools with preview and final render parity.
3. Expand settings validation and shortcut conflict handling.
4. Continue editor performance/AA tuning and workflow polish.

## Quick Run/Test Notes
- Build check:
  - `cargo check --locked`
- Run tray app:
  - `cargo run -- run`
- One-off capture:
  - `cargo run -- capture --target region --edit`
- Open settings window:
  - `cargo run -- settings`

## Local Workspace Notes
- Untracked image artifacts currently present:
  - `captures/region.png`
  - `flameshot-ui.png`
