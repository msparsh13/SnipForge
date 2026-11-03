# SnipForge (Rust edition)

A native Windows screenshot / annotate / OCR / screen-recording tool written in Rust
(egui UI, single `.exe`, no web view).

> **Status: v0.1 — written without access to a Rust compiler, so run `cargo check` first.**
> If it prints errors, paste them back and they get fixed quickly.

## Build & run
1. Install Rust: https://rustup.rs  (choose the default MSVC toolchain; it will ask you to install
   *Visual Studio Build Tools -> "Desktop development with C++"*).
2. In this folder:  `cargo run --release`
   Output: `target\release\snipforge.exe` (a single file you can copy anywhere).
3. Optional tools:
   - **Recording** needs ffmpeg:  `winget install Gyan.FFmpeg`  (or put `ffmpeg.exe` next to `snipforge.exe`)
   - **OCR** needs Tesseract: https://github.com/UB-Mannheim/tesseract/wiki

## What works in v0.1
| Feature | Hotkey |
|---|---|
| Region screenshot -> annotation editor | Ctrl+Alt+A |
| Full-screen screenshot | Ctrl+Alt+F |
| Grab text (OCR) from a region -> clipboard | Ctrl+Alt+T |
| Record region to H.264 MP4 (+ microphone) | Ctrl+Alt+R (press again to stop) |
| Open any existing image and edit it | button |

Editor tools: arrow, rectangle, ellipse, pen, highlighter, text, auto-numbered steps, blur/pixelate
redaction, undo/redo (Ctrl+Z / Ctrl+Y), copy, save (Ctrl+S), save as. Preview and saved file come from
the same renderer, so they always match. Files go to `Pictures\SnipForge`.

## Known limits (v0.1)
- Primary monitor only.
- Recording starts minimised; stop with Ctrl+Alt+R or restore the window from the taskbar.
- Hotkeys rely on the window waking up while minimised — **first thing to verify** (see below).

## Not ported yet (the Python edition has them)
scrolling capture, pin-to-screen, history browser, system tray icon, auto-captions (speech-to-text),
video editor (trim/cut/speed/captions/export).

## Planned port path
- tray icon + pin-to-screen + history
- scrolling capture (same template-matching idea, `imageproc`/own SAD matcher)
- captions via whisper.cpp (`whisper-cli.exe` + ggml model) driven from Rust
- video editor on top of ffmpeg (trim/cut/speed/burn-in subtitles)
