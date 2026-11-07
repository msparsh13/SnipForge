//! Everything that talks to ffmpeg for the video editor: probing, preview frames,
//! silent preview playback and export (cut / speed / burned-in captions).
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::captions::{write_srt, Caption};
use crate::util::no_window;

pub const PREVIEW_FPS: u32 = 15;

pub struct VideoInfo {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub has_audio: bool,
}

fn parse_hms(s: &str) -> Option<f64> {
    let mut total = 0.0;
    for part in s.trim().split(':') {
        total = total * 60.0 + part.trim().parse::<f64>().ok()?;
    }
    Some(total)
}

pub fn probe(ff: &Path, video: &Path) -> Result<VideoInfo, String> {
    let mut cmd = Command::new(ff);
    cmd.arg("-hide_banner").arg("-i").arg(video);
    no_window(&mut cmd);
    let out = cmd.output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stderr).to_string();

    let duration = text
        .lines()
        .find(|l| l.contains("Duration:"))
        .and_then(|l| l.split("Duration:").nth(1))
        .and_then(|rest| rest.split(',').next())
        .and_then(parse_hms)
        .ok_or_else(|| "Could not read the length of this video.".to_string())?;

    let vline = text.lines().find(|l| l.contains("Video:")).ok_or_else(|| "This file has no video.".to_string())?;
    let (mut width, mut height) = (0u32, 0u32);
    for seg in vline.split(',') {
        if let Some(tok) = seg.split_whitespace().next() {
            if let Some((a, b)) = tok.split_once('x') {
                if let (Ok(w), Ok(h)) = (a.parse::<u32>(), b.parse::<u32>()) {
                    if (16..20000).contains(&w) && (16..20000).contains(&h) {
                        width = w;
                        height = h;
                        break;
                    }
                }
            }
        }
    }
    if width == 0 {
        return Err("Could not read the picture size of this video.".to_string());
    }
    Ok(VideoInfo { duration, width, height, has_audio: text.contains("Audio:") })
}

/// One still frame (for scrubbing).
pub fn grab_frame(ff: &Path, video: &Path, t: f64, w: u32, h: u32) -> Option<egui::ColorImage> {
    let mut cmd = Command::new(ff);
    cmd.args(["-loglevel", "error", "-ss"]).arg(format!("{t:.3}")).arg("-i").arg(video);
    cmd.args(["-frames:v", "1", "-vf"]).arg(format!("scale={w}:{h}"));
    cmd.args(["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"]);
    cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    no_window(&mut cmd);
    let out = cmd.output().ok()?;
    if out.stdout.len() != (w * h * 4) as usize {
        return None;
    }
    Some(egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &out.stdout))
}

// ------------------------------------------------------------------ silent preview playback
pub struct PlayJob {
    stop: Arc<AtomicBool>,
    pub frames: Arc<AtomicU64>,
    pub rx: mpsc::Receiver<egui::ColorImage>,
    pub start_t: f64,
}

impl PlayJob {
    pub fn start(ctx: egui::Context, ff: PathBuf, video: PathBuf, start_t: f64, w: u32, h: u32, speed: f64) -> PlayJob {
        let stop = Arc::new(AtomicBool::new(false));
        let frames = Arc::new(AtomicU64::new(0));
        let (tx, rx) = mpsc::channel();
        let (s2, f2) = (stop.clone(), frames.clone());
        thread::spawn(move || {
            let mut cmd = Command::new(&ff);
            cmd.args(["-loglevel", "error", "-ss"]).arg(format!("{start_t:.3}")).arg("-i").arg(&video);
            cmd.arg("-vf").arg(format!("fps={PREVIEW_FPS},scale={w}:{h}"));
            cmd.args(["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"]);
            cmd.stdout(Stdio::piped()).stderr(Stdio::null());
            no_window(&mut cmd);
            let Ok(mut child) = cmd.spawn() else { return };
            let Some(mut out) = child.stdout.take() else { return };
            let mut buf = vec![0u8; (w * h * 4) as usize];
            let began = Instant::now();
            loop {
                if s2.load(Ordering::SeqCst) || out.read_exact(&mut buf).is_err() {
                    break;
                }
                let idx = f2.fetch_add(1, Ordering::SeqCst) + 1;
                let due = began + Duration::from_secs_f64(idx as f64 / (PREVIEW_FPS as f64 * speed));
                if let Some(wait) = due.checked_duration_since(Instant::now()) {
                    thread::sleep(wait);
                }
                let _ = tx.send(egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &buf));
                ctx.request_repaint();
            }
            let _ = child.kill();
            let _ = child.wait();
        });
        PlayJob { stop, frames, rx, start_t }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

// ------------------------------------------------------------------ export
#[derive(Clone, Copy, PartialEq)]
pub enum Fmt {
    Mp4,
    WebM,
    Gif,
    Mp3,
}

impl Fmt {
    pub fn ext(self) -> &'static str {
        match self {
            Fmt::Mp4 => "mp4",
            Fmt::WebM => "webm",
            Fmt::Gif => "gif",
            Fmt::Mp3 => "mp3",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Fmt::Mp4 => "MP4 (H.264)",
            Fmt::WebM => "WebM (VP9)",
            Fmt::Gif => "GIF (animated)",
            Fmt::Mp3 => "MP3 (audio only)",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Quality {
    High,
    Balanced,
    Small,
}

impl Quality {
    pub fn label(self) -> &'static str {
        match self {
            Quality::High => "High",
            Quality::Balanced => "Balanced",
            Quality::Small => "Small file",
        }
    }
    fn crf(self) -> u32 {
        match self {
            Quality::High => 20,
            Quality::Balanced => 24,
            Quality::Small => 30,
        }
    }
}

pub struct ExportParams {
    pub ff: PathBuf,
    pub src: PathBuf,
    pub out: PathBuf,
    pub keeps: Vec<(f64, f64)>,
    pub speed: f64,
    pub fmt: Fmt,
    pub scale_h: u32, // 0 = original
    pub quality: Quality,
    pub src_w: u32,
    pub src_h: u32,
    pub has_audio: bool,
    pub captions: Vec<Caption>, // already re-timed to the edited video
    pub burn: bool,
    pub cap_size: u32, // libass font size (288-line scale)
    pub cap_top: bool,
}

/// Builds the ffmpeg filter graph. Returns (graph, video label, audio label).
fn build_graph(p: &ExportParams, burn: bool) -> (String, Option<String>, Option<String>) {
    let audio = p.has_audio && p.fmt != Fmt::Gif;
    let video = p.fmt != Fmt::Mp3;
    let n = p.keeps.len();
    let mut parts: Vec<String> = Vec::new();
    for (i, (s, e)) in p.keeps.iter().enumerate() {
        if video {
            parts.push(format!("[0:v]trim=start={s:.3}:end={e:.3},setpts=PTS-STARTPTS[v{i}]"));
        }
        if audio {
            parts.push(format!("[0:a]atrim=start={s:.3}:end={e:.3},asetpts=PTS-STARTPTS[a{i}]"));
        }
    }
    let mut cat = String::new();
    for i in 0..n {
        if video {
            cat.push_str(&format!("[v{i}]"));
        }
        if audio {
            cat.push_str(&format!("[a{i}]"));
        }
    }
    let mut outs = String::new();
    if video {
        outs.push_str("[vc]");
    }
    if audio {
        outs.push_str("[ac]");
    }
    parts.push(format!("{cat}concat=n={n}:v={}:a={}{outs}", if video { 1 } else { 0 }, if audio { 1 } else { 0 }));

    let mut vlabel = None;
    let mut alabel = None;
    let fast = (p.speed - 1.0).abs() > 1e-3;
    if video {
        let mut v = "[vc]".to_string();
        if fast {
            parts.push(format!("{v}setpts=PTS/{}[vs]", p.speed));
            v = "[vs]".to_string();
        }
        if burn {
            let size = p.cap_size;
            let align = if p.cap_top { 8 } else { 2 };
            parts.push(format!(
                "{v}subtitles=cap.srt:force_style='FontSize={size},PrimaryColour=&H00FFFFFF,OutlineColour=&H80000000,BorderStyle=3,Outline=2,Shadow=0,MarginV=14,Alignment={align}'[vo]"
            ));
            v = "[vo]".to_string();
        }
        if p.fmt == Fmt::Gif {
            let mut chain = String::from("fps=12");
            if p.src_w > 720 {
                chain.push_str(",scale=720:-2");
            }
            parts.push(format!("{v}{chain},split[ga][gb];[ga]palettegen=stats_mode=diff[gp];[gb][gp]paletteuse=dither=bayer:bayer_scale=5[vg]"));
            v = "[vg]".to_string();
        } else if p.scale_h > 0 && p.src_h > p.scale_h {
            parts.push(format!("{v}scale=-2:{}[vf]", p.scale_h));
            v = "[vf]".to_string();
        }
        vlabel = Some(v);
    }
    if audio {
        let mut a = "[ac]".to_string();
        if fast {
            parts.push(format!("{a}atempo={}[as]", p.speed));
            a = "[as]".to_string();
        }
        alabel = Some(a);
    }
    (parts.join(";"), vlabel, alabel)
}

pub struct ExportJob {
    pub progress: Arc<AtomicU32>, // 0..=1000
    cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<Result<PathBuf, String>>>,
}

impl ExportJob {
    pub fn start(ctx: egui::Context, p: ExportParams) -> ExportJob {
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (pr, ca) = (progress.clone(), cancel.clone());
        let handle = thread::spawn(move || {
            let r = run_export(&ctx, p, &pr, &ca);
            ctx.request_repaint();
            r
        });
        ExportJob { progress, cancel, handle: Some(handle) }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn poll(&mut self) -> Option<Result<PathBuf, String>> {
        let done = self.handle.as_ref().map(|h| h.is_finished()).unwrap_or(false);
        if !done {
            return None;
        }
        let h = self.handle.take()?;
        Some(h.join().unwrap_or_else(|_| Err("export crashed".to_string())))
    }
}

fn run_export(ctx: &egui::Context, p: ExportParams, progress: &AtomicU32, cancel: &AtomicBool) -> Result<PathBuf, String> {
    if p.keeps.is_empty() {
        return Err("Nothing left to export (everything was cut).".to_string());
    }
    if p.fmt == Fmt::Mp3 && !p.has_audio {
        return Err("This video has no audio track.".to_string());
    }
    let total_secs = (p.keeps.iter().map(|(s, e)| e - s).sum::<f64>() / p.speed).max(0.1);
    let burn = p.burn && p.fmt != Fmt::Mp3 && !p.captions.is_empty();

    let tmp = std::env::temp_dir().join(format!("snipforge_export_{}_{}", crate::util::timestamp(), std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    if burn {
        write_srt(&p.captions, &tmp.join("cap.srt")).map_err(|e| e.to_string())?;
    }

    let (graph, vlabel, alabel) = build_graph(&p, burn);
    let mut cmd = Command::new(&p.ff);
    cmd.args(["-y", "-loglevel", "error", "-progress", "pipe:1", "-nostats", "-i"]).arg(&p.src);
    cmd.arg("-filter_complex").arg(&graph);
    if let Some(v) = &vlabel {
        cmd.arg("-map").arg(v);
    }
    if let Some(a) = &alabel {
        cmd.arg("-map").arg(a);
    }
    let crf = p.quality.crf();
    match p.fmt {
        Fmt::Mp4 => {
            cmd.args(["-c:v", "libx264", "-preset", "veryfast", "-crf"]).arg(crf.to_string());
            cmd.args(["-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "160k", "-movflags", "+faststart"]);
        }
        Fmt::WebM => {
            cmd.args(["-c:v", "libvpx-vp9", "-crf"]).arg((crf + 8).to_string());
            cmd.args(["-b:v", "0", "-deadline", "realtime", "-cpu-used", "6", "-row-mt", "1", "-pix_fmt", "yuv420p"]);
            cmd.args(["-c:a", "libopus", "-b:a", "128k"]);
        }
        Fmt::Gif => {
            cmd.args(["-loop", "0"]);
        }
        Fmt::Mp3 => {
            cmd.args(["-c:a", "libmp3lame", "-q:a", "3"]);
        }
    }
    cmd.arg(&p.out);
    cmd.current_dir(&tmp);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    no_window(&mut cmd);

    let mut child = cmd.spawn().map_err(|e| format!("Could not start ffmpeg: {e}"))?;
    let stdout = child.stdout.take().ok_or_else(|| "no ffmpeg output".to_string())?;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&p.out);
            let _ = std::fs::remove_dir_all(&tmp);
            return Err("Cancelled".to_string());
        }
        if let Some(v) = line.strip_prefix("out_time_us=") {
            if let Ok(us) = v.trim().parse::<f64>() {
                let frac = (us / 1_000_000.0 / total_secs).clamp(0.0, 1.0);
                progress.store((frac * 1000.0) as u32, Ordering::Relaxed);
                ctx.request_repaint();
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let mut err = String::new();
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut err);
    }
    let _ = std::fs::remove_dir_all(&tmp);
    if !status.success() {
        let _ = std::fs::remove_file(&p.out);
        return Err(format!("ffmpeg failed:\n{}", err.trim()));
    }
    progress.store(1000, Ordering::Relaxed);
    Ok(p.out)
}
