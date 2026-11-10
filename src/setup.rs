//! Finds ffmpeg and installs it automatically (built-in downloader: progress + clear errors).
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::util::no_window;

pub const DOWNLOAD_PAGE: &str = "https://www.gyan.dev/ffmpeg/builds/";

const URLS: [&str; 2] = [
    "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip",
    "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip",
];

/// Shared between the installer thread and the UI so a progress bar can be shown.
#[derive(Default)]
pub struct Progress {
    done: AtomicU64,
    total: AtomicU64,
    phase: AtomicU8, // 0 = downloading, 1 = unpacking
}

impl Progress {
    pub fn reset(&self) {
        self.done.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
        self.phase.store(0, Ordering::Relaxed);
    }

    pub fn fraction(&self) -> f32 {
        if self.phase.load(Ordering::Relaxed) == 1 {
            return 1.0;
        }
        let t = self.total.load(Ordering::Relaxed);
        if t == 0 {
            0.0
        } else {
            (self.done.load(Ordering::Relaxed) as f32 / t as f32).min(1.0)
        }
    }

    pub fn text(&self) -> String {
        if self.phase.load(Ordering::Relaxed) == 1 {
            return "Unpacking...".to_string();
        }
        let d = self.done.load(Ordering::Relaxed) / 1_000_000;
        let t = self.total.load(Ordering::Relaxed) / 1_000_000;
        if t == 0 {
            format!("Downloading... {d} MB")
        } else {
            format!("Downloading... {d} / {t} MB")
        }
    }
}

pub fn bin_dir() -> PathBuf {
    crate::config::Settings::load().engine_path()
}

/// ffmpeg next to the exe, in our private bin folder, or on PATH.
pub fn ffmpeg_path() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("ffmpeg.exe");
            if p.exists() {
                return Some(p);
            }
        }
    }
    let p = bin_dir().join("ffmpeg.exe");
    if p.exists() {
        return Some(p);
    }
    let mut c = Command::new("ffmpeg");
    c.arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    no_window(&mut c);
    match c.status() {
        Ok(s) if s.success() => Some(PathBuf::from("ffmpeg")),
        _ => None,
    }
}

fn download(url: &str, path: &Path, p: &Progress) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(60))
        .build();
    let resp = agent.get(url).call().map_err(|e| e.to_string())?;
    let total = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    p.total.store(total, Ordering::Relaxed);
    p.done.store(0, Ordering::Relaxed);
    p.phase.store(0, Ordering::Relaxed);

    let mut reader = resp.into_reader();
    let mut file =
        File::create(path).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("connection lost: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        p.done.fetch_add(n as u64, Ordering::Relaxed);
    }
    file.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn extract_ffmpeg(zip_path: &Path, out: &Path, p: &Progress) -> Result<(), String> {
    p.phase.store(1, Ordering::Relaxed);
    let file = File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().replace('\\', "/");
        if name.ends_with("/bin/ffmpeg.exe") || name == "ffmpeg.exe" {
            let tmp = out.with_extension("part");
            let mut f =
                File::create(&tmp).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
            std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
            drop(f);
            std::fs::rename(&tmp, out).map_err(|e| e.to_string())?;
            return Ok(());
        }
    }
    Err("ffmpeg.exe was not found inside the download".to_string())
}

/// Download ffmpeg into SnipForge's own folder (no admin rights, no winget).
pub fn install_ffmpeg(progress: Arc<Progress>) -> Result<PathBuf, String> {
    let dest = bin_dir();
    std::fs::create_dir_all(&dest).map_err(|e| format!("Cannot create {}: {e}", dest.display()))?;
    let zip_path = std::env::temp_dir().join("snipforge_ffmpeg.zip");
    let mut errors: Vec<String> = Vec::new();

    for url in URLS {
        progress.reset();
        match download(url, &zip_path, &progress) {
            Ok(()) => match extract_ffmpeg(&zip_path, &dest.join("ffmpeg.exe"), &progress) {
                Ok(()) => {
                    let _ = std::fs::remove_file(&zip_path);
                    return ffmpeg_path()
                        .ok_or_else(|| "ffmpeg was unpacked but cannot be started".to_string());
                }
                Err(e) => errors.push(format!("{url}\n   unpack failed: {e}")),
            },
            Err(e) => errors.push(format!("{url}\n   {e}")),
        }
    }
    let _ = std::fs::remove_file(&zip_path);
    Err(format!(
        "Could not download ffmpeg:\n{}\n\nIf your network blocks it, download ffmpeg yourself and use 'Choose ffmpeg.exe...'.",
        errors.join("\n")
    ))
}

/// Use an ffmpeg.exe the user picked manually.
pub fn import_ffmpeg(src: &Path) -> Result<PathBuf, String> {
    let dest_dir = bin_dir();
    std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
    let dest = dest_dir.join("ffmpeg.exe");
    std::fs::copy(src, &dest).map_err(|e| format!("Could not copy the file: {e}"))?;
    let mut c = Command::new(&dest);
    c.arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    no_window(&mut c);
    match c.status() {
        Ok(s) if s.success() => Ok(dest),
        _ => {
            let _ = std::fs::remove_file(&dest);
            Err("That file does not look like a working ffmpeg.exe".to_string())
        }
    }
}
