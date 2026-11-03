//! OCR using the text recogniser built into Windows 10/11 (nothing to install).
//! Falls back to Tesseract only if it happens to be installed.
use std::path::{Path, PathBuf};
use std::process::Command;

use image::RgbaImage;

use crate::util::no_window;

const OCR_PS: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$null = [Windows.Storage.StorageFile, Windows.Storage, ContentType = WindowsRuntime]
$null = [Windows.Storage.FileAccessMode, Windows.Storage, ContentType = WindowsRuntime]
$null = [Windows.Storage.Streams.IRandomAccessStream, Windows.Storage.Streams, ContentType = WindowsRuntime]
$null = [Windows.Graphics.Imaging.BitmapDecoder, Windows.Graphics, ContentType = WindowsRuntime]
$null = [Windows.Graphics.Imaging.SoftwareBitmap, Windows.Foundation, ContentType = WindowsRuntime]
$null = [Windows.Media.Ocr.OcrEngine, Windows.Foundation, ContentType = WindowsRuntime]
$null = [Windows.Media.Ocr.OcrResult, Windows.Foundation, ContentType = WindowsRuntime]
$asTaskGeneric = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
function Await($task, $resultType) {
  $m = $asTaskGeneric.MakeGenericMethod($resultType)
  $t = $m.Invoke($null, @($task))
  $t.Wait(-1) | Out-Null
  $t.Result
}
$path = '__IMG__'
$file = Await ([Windows.Storage.StorageFile]::GetFileFromPathAsync($path)) ([Windows.Storage.StorageFile])
$stream = Await ($file.OpenAsync([Windows.Storage.FileAccessMode]::Read)) ([Windows.Storage.Streams.IRandomAccessStream])
$decoder = Await ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
$bitmap = Await ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
$engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages()
if ($engine -eq $null) { throw 'No OCR language is installed. Add one in Settings > Time and language > Language.' }
$result = Await ($engine.RecognizeAsync($bitmap)) ([Windows.Media.Ocr.OcrResult])
Write-Output $result.Text
"#;

pub fn ocr(img: &RgbaImage) -> Result<String, String> {
    let big = if img.width() < 1200 {
        image::imageops::resize(img, img.width() * 2, img.height() * 2, image::imageops::FilterType::Lanczos3)
    } else {
        img.clone()
    };
    let path = std::env::temp_dir().join("snipforge_ocr.png");
    big.save(&path).map_err(|e| e.to_string())?;

    match windows_ocr(&path) {
        Ok(t) => Ok(t),
        Err(win_err) => match tesseract(&path) {
            Ok(t) => Ok(t),
            Err(_) => Err(format!("Text recognition failed:\n{win_err}")),
        },
    }
}

fn windows_ocr(path: &Path) -> Result<String, String> {
    let p = path.to_string_lossy().replace('\'', "''");
    let script = OCR_PS.replace("__IMG__", &p);
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"]).arg(script);
    no_window(&mut cmd);
    let out = cmd.output().map_err(|e| format!("Could not start PowerShell: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn tesseract(path: &Path) -> Result<String, String> {
    let mut exe = PathBuf::from("tesseract");
    for p in [
        r"C:\Program Files\Tesseract-OCR\tesseract.exe",
        r"C:\Program Files (x86)\Tesseract-OCR\tesseract.exe",
    ] {
        if Path::new(p).exists() {
            exe = PathBuf::from(p);
        }
    }
    let mut cmd = Command::new(exe);
    cmd.arg(path).arg("stdout");
    no_window(&mut cmd);
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
