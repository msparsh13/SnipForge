//! Screen recording: frames from DXGI (`scrap`) are piped into ffmpeg (H.264 + optional mic).
use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eframe::egui;
use scrap::{Capturer, Display};

use crate::util::no_window;

pub struct Recorder {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<Result<PathBuf, String>>>,
}

impl Recorder {
    /// region = (x, y, width, height) in physical pixels of the primary display.
    pub fn start(ctx: egui::Context, region: (u32, u32, u32, u32), fps: u32, mic: bool, out: PathBuf, ffmpeg: PathBuf) -> Recorder {
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let handle = thread::spawn(move || {
            let r = run(region, fps, mic, out, stop2, ffmpeg);
            ctx.request_repaint();
            r
        });
        Recorder { stop, handle: Some(handle) }
    }

    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// Returns Some(result) once the recording has been finalised.
    pub fn poll(&mut self) -> Option<Result<PathBuf, String>> {
        let finished = self.handle.as_ref().map(|h| h.is_finished()).unwrap_or(false);
        if !finished {
            return None;
        }
        let h = self.handle.take()?;
        Some(h.join().unwrap_or_else(|_| Err("recorder crashed".to_string())))
    }
}

/// First DirectShow audio device (usually the default microphone).
fn mic_device(ff: &std::path::Path) -> Option<String> {
    let mut cmd = Command::new(ff);
    cmd.args(["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"]);
    no_window(&mut cmd);
    let out = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&out.stderr).to_string();
    for line in text.lines() {
        if line.contains("(audio)") {
            let a = line.find('"')?;
            let b = line[a + 1..].find('"')?;
            return Some(line[a + 1..a + 1 + b].to_string());
        }
    }
    None
}

fn run(region: (u32, u32, u32, u32), fps: u32, mic: bool, out: PathBuf, stop: Arc<AtomicBool>, ff: PathBuf) -> Result<PathBuf, String> {
    let (rx, ry, rw, rh) = region;
    let (rw, rh) = (rw & !1, rh & !1); // encoders want even sizes
    if rw < 16 || rh < 16 {
        return Err("Region too small".to_string());
    }
    let (rx, ry, rw_u, rh_u) = (rx as usize, ry as usize, rw as usize, rh as usize);

    let display = Display::primary().map_err(|e| e.to_string())?;
    let mut cap = Capturer::new(display).map_err(|e| e.to_string())?;
    let (sw, sh) = (cap.width(), cap.height());
    if rx + rw_u > sw || ry + rh_u > sh {
        return Err("Region is outside the primary display".to_string());
    }

    let mut cmd = Command::new(&ff);
    cmd.args(["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "bgra"]);
    cmd.arg("-video_size").arg(format!("{}x{}", rw, rh));
    cmd.arg("-framerate").arg(fps.to_string());
    cmd.args(["-i", "-"]);
    let mic_name = if mic { mic_device(&ff) } else { None };
    if let Some(name) = &mic_name {
        cmd.args(["-thread_queue_size", "1024", "-f", "dshow", "-i"]);
        cmd.arg(format!("audio={}", name));
        cmd.args(["-map", "0:v", "-map", "1:a", "-c:a", "aac", "-b:a", "160k"]);
    }
    cmd.args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-pix_fmt", "yuv420p", "-movflags", "+faststart"]);
    cmd.arg(&out);
    cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
    no_window(&mut cmd);

    let mut child = cmd.spawn().map_err(|e| {
        format!("Could not start ffmpeg ({e}). Open SnipForge and use 'Install recording engine'.")
    })?;
    let mut stdin = child.stdin.take().ok_or_else(|| "no ffmpeg stdin".to_string())?;

    let frame_bytes = rw_u * rh_u * 4;
    let mut last = vec![0u8; frame_bytes];
    let start = Instant::now();
    let mut written: u64 = 0;
    let mut error: Option<String> = None;

    'outer: while !stop.load(Ordering::SeqCst) {
        match cap.frame() {
            Ok(frame) => {
                let stride = frame.len() / sh;
                for y in 0..rh_u {
                    let src_start = (ry + y) * stride + rx * 4;
                    let src = &frame[src_start..src_start + rw_u * 4];
                    let dst_start = y * rw_u * 4;
                    last[dst_start..dst_start + rw_u * 4].copy_from_slice(src);
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {} // screen unchanged: reuse last frame
            Err(e) => {
                error = Some(e.to_string());
                break;
            }
        }
        // keep real-time speed: write as many frames as are "due"
        let due = (start.elapsed().as_secs_f64() * fps as f64) as u64 + 1;
        let n = due.saturating_sub(written).max(1).min(fps as u64);
        for _ in 0..n {
            if stdin.write_all(&last).is_err() {
                break 'outer; // ffmpeg exited; its error is read below
            }
            written += 1;
        }
        let target = start + Duration::from_secs_f64(written as f64 / fps as f64);
        if let Some(wait) = target.checked_duration_since(Instant::now()) {
            thread::sleep(wait);
        }
    }

    drop(stdin); // closing the pipe tells ffmpeg to finish the file
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if let Some(e) = error {
        return Err(e);
    }
    if !output.status.success() {
        let msg = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("ffmpeg failed:\n{}", msg.trim()));
    }
    Ok(out)
}
