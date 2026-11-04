# SnipForge (Rust edition) v0.2

Native Windows screenshot / annotate / OCR / screen-recording tool (Rust + egui, one `.exe`).

> Written without access to a Rust compiler. Run `cargo check` first; if it prints errors,
> paste them back and they get fixed fast.

## Build & run
1. Install Rust from https://rustup.rs (accept the "Desktop development with C++" build tools prompt).
2. In this folder: `cargo run --release`  ->  `target\release\snipforge.exe`
3. That's all. On first start SnipForge downloads the recording engine (ffmpeg, ~90 MB) by itself.
   Text recognition uses the OCR built into Windows 10/11, so there is nothing to install.

## Use
| Action | Hotkey |
|---|---|
| Region capture -> editor | Ctrl+Alt+A |
| Full-screen capture -> editor | Ctrl+Alt+F |
| Grab text (OCR) -> clipboard | Ctrl+Alt+T |
| Record region (press again to stop) | Ctrl+Alt+R |

Closing the main window only minimises it, so the hotkeys keep working. Use **Quit SnipForge** to exit.
Files are saved in `Pictures\SnipForge`. Settings are in `%APPDATA%\SnipForge\settings.txt`.

## How a capture works now
Every capture runs as its own small process: it hides the other SnipForge windows, grabs the screen,
then shows the selection overlay immediately - nothing needs to be clicked twice.

## Editor
Arrow, rectangle, ellipse, pen, marker, text, numbered steps, blur, **crop**.
Undo/redo (Ctrl+Z / Ctrl+Y), copy (Ctrl+C), save (Ctrl+S), save as. Crop: drag, then Enter to apply.

## Limits
- Primary monitor only.
- Recording is video + microphone (first audio device); no system audio yet.
- See ROADMAP.md for what is planned next.
