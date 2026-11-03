//! Finds ffmpeg, and downloads it automatically when it is missing (no winget needed).
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::util::no_window;

pub fn bin_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SnipForge")
        .join("bin")
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
    c.arg("-version").stdout(Stdio::null()).stderr(Stdio::null());
    no_window(&mut c);
    match c.status() {
        Ok(s) if s.success() => Some(PathBuf::from("ffmpeg")),
        _ => None,
    }
}

/// Downloads a static ffmpeg build with PowerShell (built into Windows) into our bin folder.
pub fn install_ffmpeg() -> Result<PathBuf, String> {
    let dest = bin_dir();
    let dest_str = dest.to_string_lossy().replace('\'', "''");
    let script = format!(
        r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$dest = '{dest}'
New-Item -ItemType Directory -Force -Path $dest | Out-Null
$zip = Join-Path $env:TEMP 'snipforge_ffmpeg.zip'
$tmp = Join-Path $env:TEMP 'snipforge_ffmpeg_x'
$urls = @('https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip', 'https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip')
$ok = $false
foreach ($u in $urls) {{
  try {{ Invoke-WebRequest -Uri $u -OutFile $zip -UseBasicParsing; $ok = $true; break }} catch {{ }}
}}
if (-not $ok) {{ throw 'Download failed. Please check your internet connection.' }}
if (Test-Path $tmp) {{ Remove-Item $tmp -Recurse -Force }}
Expand-Archive -Path $zip -DestinationPath $tmp -Force
$f = Get-ChildItem -Path $tmp -Recurse -Filter ffmpeg.exe | Select-Object -First 1
if (-not $f) {{ throw 'ffmpeg.exe was not found in the download.' }}
Copy-Item $f.FullName (Join-Path $dest 'ffmpeg.exe') -Force
Remove-Item $zip -Force
Remove-Item $tmp -Recurse -Force
"#,
        dest = dest_str
    );
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"]).arg(script);
    no_window(&mut cmd);
    let out = cmd.output().map_err(|e| format!("Could not start PowerShell: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() { "Download failed.".to_string() } else { err });
    }
    ffmpeg_path().ok_or_else(|| "ffmpeg still not found after install.".to_string())
}
