//! Small helpers: folders, timestamps, clipboard, child processes, recording IPC.
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use image::RgbaImage;

/// Folder for screenshots (honours the custom folder in Settings).
pub fn captures_dir() -> PathBuf {
    crate::config::Settings::load().shots_path()
}

pub fn timestamp() -> String {
    chrono::Local::now().format("%Y%m%d_%H%M%S").to_string()
}

pub fn no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

pub fn open_in_explorer(p: &Path) {
    let _ = Command::new("explorer").arg(p).spawn();
}

pub fn reveal_in_explorer(p: &Path) {
    let _ = Command::new("explorer")
        .arg(format!("/select,{}", p.display()))
        .spawn();
}

pub fn open_file(p: &Path) {
    let mut c = Command::new("cmd");
    c.args(["/C", "start", ""]).arg(p);
    no_window(&mut c);
    let _ = c.spawn();
}

pub fn open_url(url: &str) {
    let mut c = Command::new("cmd");
    c.args(["/C", "start", ""]).arg(url);
    no_window(&mut c);
    let _ = c.spawn();
}

/// Start a capture worker process: "region", "full", "ocr", "record" or "open <path>".
pub fn spawn_worker(kind: &str, extra: Option<&str>) {
    if let Ok(exe) = std::env::current_exe() {
        let mut c = Command::new(exe);
        c.arg("--worker").arg(kind);
        if let Some(e) = extra {
            c.arg(e);
        }
        let _ = c.spawn();
    }
}

/// Start a worker that needs a file path (pin, open, video editor).
pub fn spawn_worker_path(kind: &str, path: &Path) {
    let s = path.to_string_lossy().to_string();
    spawn_worker(kind, Some(&s));
}

pub fn copy_image(img: &RgbaImage) -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_image(arboard::ImageData {
        width: img.width() as usize,
        height: img.height() as usize,
        bytes: Cow::Owned(img.as_raw().clone()),
    })
    .map_err(|e| e.to_string())
}

pub fn temp_png(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "{prefix}_{}_{}.png",
        timestamp(),
        std::process::id()
    ))
}

pub fn clipboard_image() -> Option<RgbaImage> {
    let mut cb = arboard::Clipboard::new().ok()?;
    let d = cb.get_image().ok()?;
    RgbaImage::from_raw(d.width as u32, d.height as u32, d.bytes.into_owned())
}

pub fn copy_text(text: &str) -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_text(text.to_string()).map_err(|e| e.to_string())
}

// ---- tiny file-based IPC so the Ctrl+Alt+R hotkey can stop a running recording ----
fn marker_path() -> PathBuf {
    std::env::temp_dir().join("snipforge_recording.marker")
}
fn stop_path() -> PathBuf {
    std::env::temp_dir().join("snipforge_recording.stop")
}

pub fn touch_marker() {
    let _ = std::fs::write(marker_path(), timestamp());
}

pub fn clear_marker() {
    let _ = std::fs::remove_file(marker_path());
    let _ = std::fs::remove_file(stop_path());
}

/// A recording is "active" if its marker was refreshed in the last 3 seconds.
pub fn recording_active() -> bool {
    std::fs::metadata(marker_path())
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map(|d| d.as_secs_f32() < 3.0)
        .unwrap_or(false)
}

fn pause_path() -> PathBuf {
    std::env::temp_dir().join("snipforge_recording.pause")
}

pub fn request_pause() {
    let _ = std::fs::write(pause_path(), "pause");
}

pub fn take_pause_request() -> bool {
    let p = pause_path();
    if p.exists() {
        let _ = std::fs::remove_file(p);
        true
    } else {
        false
    }
}

fn snap_path() -> PathBuf {
    std::env::temp_dir().join("snipforge_recording.snap")
}

pub fn request_snap() {
    let _ = std::fs::write(snap_path(), "snap");
}

pub fn take_snap_request() -> bool {
    let p = snap_path();
    if p.exists() {
        let _ = std::fs::remove_file(p);
        true
    } else {
        false
    }
}

pub fn request_stop() {
    let _ = std::fs::write(stop_path(), "stop");
}

pub fn take_stop_request() -> bool {
    let p = stop_path();
    if p.exists() {
        let _ = std::fs::remove_file(p);
        true
    } else {
        false
    }
}
