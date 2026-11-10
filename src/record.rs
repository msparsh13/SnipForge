//! Screen recording. Video frames are piped into ffmpeg (H.264); audio (microphone and/or device
//! audio) is captured by SnipForge itself into raw files and muxed in at the end. Supports pause.
//! Regions inside the primary screen use fast DXGI capture; anything else (other monitors, or a region
//! spanning several monitors) uses GDI.
use std::fs::File;
use std::io::{BufWriter, ErrorKind, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eframe::egui;
use scrap::{Capturer, Display};

use crate::audio::{self, AudioCapture};
use crate::gdi::GdiGrabber;
use crate::util::no_window;

#[derive(Clone, Copy)]
pub struct RecOptions {
    pub fps: u32,
    pub mic: bool,
    pub system: bool,
}

#[derive(Default)]
struct Shared {
    stop: AtomicBool,
    paused: Arc<AtomicBool>,
    mic_on: Arc<AtomicBool>,
    sys_on: Arc<AtomicBool>,
    mic_failed: AtomicBool,
    sys_failed: AtomicBool,
    mic_level: Arc<AtomicU32>,
    sys_level: Arc<AtomicU32>,
    mic_err: Mutex<String>,
    sys_err: Mutex<String>,
    active_ms: AtomicU64,
    note: Mutex<String>,
}

pub struct Recorder {
    shared: Arc<Shared>,
    handle: Option<JoinHandle<Result<PathBuf, String>>>,
}

impl Recorder {
    /// region = (x, y, width, height) in screen pixels. x/y may be negative (monitors left of / above the primary).
    pub fn start(
        ctx: egui::Context,
        region: (i32, i32, u32, u32),
        opts: RecOptions,
        out: PathBuf,
        ffmpeg: PathBuf,
    ) -> Recorder {
        let shared = Arc::new(Shared::default());
        shared.mic_on.store(opts.mic, Ordering::SeqCst);
        shared.sys_on.store(opts.system, Ordering::SeqCst);
        let s2 = shared.clone();
        let handle = thread::spawn(move || {
            let r = run(region, opts, out, s2, ffmpeg);
            ctx.request_repaint();
            r
        });
        Recorder {
            shared,
            handle: Some(handle),
        }
    }

    pub fn request_stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
    }
    pub fn toggle_pause(&self) {
        let v = self.shared.paused.load(Ordering::SeqCst);
        self.shared.paused.store(!v, Ordering::SeqCst);
    }
    pub fn paused(&self) -> bool {
        self.shared.paused.load(Ordering::SeqCst)
    }
    /// Recorded time, not counting pauses.
    pub fn active_secs(&self) -> f32 {
        self.shared.active_ms.load(Ordering::Relaxed) as f32 / 1000.0
    }
    /// True if the microphone / device audio could not be started (no device, access denied...).
    pub fn mic_failed(&self) -> bool {
        self.shared.mic_failed.load(Ordering::Relaxed)
    }
    pub fn sys_failed(&self) -> bool {
        self.shared.sys_failed.load(Ordering::Relaxed)
    }
    /// Current loudness 0.0 .. 1.0 of the microphone / device audio.
    pub fn mic_level(&self) -> f32 {
        self.shared.mic_level.load(Ordering::Relaxed) as f32 / 1000.0
    }
    pub fn sys_level(&self) -> f32 {
        self.shared.sys_level.load(Ordering::Relaxed) as f32 / 1000.0
    }
    /// Why the microphone / device audio could not start (empty if it did).
    pub fn mic_error(&self) -> String {
        self.shared
            .mic_err
            .lock()
            .map(|e| e.clone())
            .unwrap_or_default()
    }
    pub fn sys_error(&self) -> String {
        self.shared
            .sys_err
            .lock()
            .map(|e| e.clone())
            .unwrap_or_default()
    }
    pub fn mic_on(&self) -> bool {
        self.shared.mic_on.load(Ordering::Relaxed)
    }
    pub fn sys_on(&self) -> bool {
        self.shared.sys_on.load(Ordering::Relaxed)
    }
    pub fn toggle_mic(&self) {
        let v = self.shared.mic_on.load(Ordering::SeqCst);
        self.shared.mic_on.store(!v, Ordering::SeqCst);
    }
    pub fn toggle_sys(&self) {
        let v = self.shared.sys_on.load(Ordering::SeqCst);
        self.shared.sys_on.store(!v, Ordering::SeqCst);
    }
    /// Warnings collected while recording (e.g. "no microphone found").
    pub fn note(&self) -> String {
        self.shared
            .note
            .lock()
            .map(|n| n.clone())
            .unwrap_or_default()
    }

    /// Returns Some(result) once the recording has been finalised.
    pub fn poll(&mut self) -> Option<Result<PathBuf, String>> {
        let finished = self
            .handle
            .as_ref()
            .map(|h| h.is_finished())
            .unwrap_or(false);
        if !finished {
            return None;
        }
        let h = self.handle.take()?;
        Some(
            h.join()
                .unwrap_or_else(|_| Err("recorder crashed".to_string())),
        )
    }
}

// ------------------------------------------------------------------ audio files
struct Track {
    cap: AudioCapture,
    file: BufWriter<File>,
    path: PathBuf,
    written: u64, // f32 values written
}

impl Track {
    fn open(loopback: bool, shared: &Shared, path: PathBuf) -> Result<Track, String> {
        let enabled = if loopback {
            shared.sys_on.clone()
        } else {
            shared.mic_on.clone()
        };
        let level = if loopback {
            shared.sys_level.clone()
        } else {
            shared.mic_level.clone()
        };
        let cap = audio::start_capture(loopback, shared.paused.clone(), enabled, level)?;
        let file = BufWriter::new(File::create(&path).map_err(|e| e.to_string())?);
        Ok(Track {
            cap,
            file,
            path,
            written: 0,
        })
    }

    /// Move captured samples to disk, and pad with silence so the track keeps real-time length
    /// (device audio delivers nothing while the computer is silent).
    fn pump(&mut self, active_secs: f64) {
        while let Ok(buf) = self.cap.rx.try_recv() {
            if self.file.write_all(&buf).is_ok() {
                self.written += (buf.len() / 4) as u64;
            }
        }
        let ch = self.cap.channels as u64;
        let per_sec = self.cap.rate as f64 * ch as f64;
        let expected = (active_secs * per_sec) as u64;
        if (self.written + (per_sec * 0.15) as u64) < expected {
            let deficit = (expected - self.written) / ch * ch;
            let zeros = [0u8; 4096];
            let mut left = deficit * 4;
            while left > 0 {
                let n = left.min(zeros.len() as u64) as usize;
                if self.file.write_all(&zeros[..n]).is_err() {
                    break;
                }
                left -= n as u64;
            }
            self.written += deficit;
        }
    }

    fn finish(mut self, active_secs: f64) -> (PathBuf, u32, u16) {
        self.cap.stop();
        self.pump(active_secs);
        let _ = self.file.flush();
        (self.path, self.cap.rate, self.cap.channels)
    }
}

fn wait_timeout(child: &mut Child, secs: u64) -> Result<ExitStatus, String> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return Ok(s),
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(secs) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("ffmpeg did not finish in time".to_string());
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn read_stderr(child: &mut Child) -> String {
    let mut s = String::new();
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut s);
    }
    s.trim().to_string()
}

fn add_note(shared: &Shared, text: &str) {
    if let Ok(mut n) = shared.note.lock() {
        if !n.is_empty() {
            n.push_str("\n");
        }
        n.push_str(text);
    }
}

// ------------------------------------------------------------------ main recording thread
fn run(
    region: (i32, i32, u32, u32),
    opts: RecOptions,
    out: PathBuf,
    shared: Arc<Shared>,
    ff: PathBuf,
) -> Result<PathBuf, String> {
    let fps = opts.fps.max(1);
    let (ax, ay, rw, rh) = region;
    let (rw, rh) = (rw & !1, rh & !1); // encoders want even sizes
    if rw < 16 || rh < 16 {
        return Err("Region too small".to_string());
    }
    let (rw_u, rh_u) = (rw as usize, rh as usize);

    // choose the capture method
    let mut dxgi: Option<(Capturer, usize)> = None; // capturer + screen height
    let mut gdi: Option<GdiGrabber> = None;
    if let Ok(display) = Display::primary() {
        let (pw, ph) = (display.width(), display.height());
        let inside = ax >= 0 && ay >= 0 && (ax as usize + rw_u) <= pw && (ay as usize + rh_u) <= ph;
        if inside {
            if let Ok(c) = Capturer::new(display) {
                let sh = c.height();
                dxgi = Some((c, sh));
            }
        }
    }
    if dxgi.is_none() {
        gdi = Some(GdiGrabber::new(ax, ay, rw as i32, rh as i32)?);
    }

    // audio tracks are opened on demand: at the start if enabled, or later when Mic / Audio is switched on
    let stamp = crate::util::timestamp();
    let tmpdir = std::env::temp_dir();
    let mic_path = tmpdir.join(format!("snipforge_mic_{stamp}.raw"));
    let sys_path = tmpdir.join(format!("snipforge_sys_{stamp}.raw"));
    let mut mic: Option<Track> = None;
    let mut sys: Option<Track> = None;

    // video encoder: reads raw frames from stdin and ends when stdin is closed
    let video_tmp = out.with_extension("video.tmp.mp4");
    let mut cmd = Command::new(&ff);
    cmd.args([
        "-y",
        "-loglevel",
        "error",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "bgra",
    ]);
    cmd.arg("-video_size").arg(format!("{}x{}", rw, rh));
    cmd.arg("-framerate").arg(fps.to_string());
    cmd.args(["-i", "-", "-an"]);
    if rw > 3840 || rh > 2160 {
        // several monitors at once: keep the file/encoder manageable (max 3840 x 2160, aspect ratio kept)
        cmd.args(["-vf", "scale=w=3840:h=2160:force_original_aspect_ratio=decrease,scale=trunc(iw/2)*2:trunc(ih/2)*2"]);
        add_note(
            &shared,
            "The recorded area is very large, so the video was scaled down to fit 3840 px.",
        );
    }
    let preset = if (rw as u64) * (rh as u64) > 2_600_000 {
        "ultrafast"
    } else {
        "veryfast"
    };
    cmd.args([
        "-c:v", "libx264", "-preset", preset, "-crf", "23", "-pix_fmt", "yuv420p",
    ]);
    cmd.arg(&video_tmp);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    no_window(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| {
        format!("Could not start ffmpeg ({e}). Open SnipForge and use 'Install now' in the Components box.")
    })?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "no ffmpeg stdin".to_string())?;

    let frame_bytes = rw_u * rh_u * 4;
    let mut last = vec![0u8; frame_bytes];
    let start = Instant::now();
    let mut paused_total = Duration::ZERO;
    let mut pause_started: Option<Instant> = None;
    let mut written: u64 = 0;
    let mut error: Option<String> = None;
    let mut active = Duration::ZERO;

    'outer: while !shared.stop.load(Ordering::SeqCst) {
        // ---- start an audio source the first time it is switched on (earlier time is filled with silence)
        if mic.is_none()
            && !shared.mic_failed.load(Ordering::Relaxed)
            && shared.mic_on.load(Ordering::Relaxed)
        {
            match Track::open(false, &shared, mic_path.clone()) {
                Ok(t) => mic = Some(t),
                Err(e) => {
                    shared.mic_failed.store(true, Ordering::Relaxed);
                    if let Ok(mut m) = shared.mic_err.lock() {
                        *m = e.clone();
                    }
                    add_note(&shared, &format!("Microphone not recorded: {e}"));
                }
            }
        }
        if sys.is_none()
            && !shared.sys_failed.load(Ordering::Relaxed)
            && shared.sys_on.load(Ordering::Relaxed)
        {
            match Track::open(true, &shared, sys_path.clone()) {
                Ok(t) => sys = Some(t),
                Err(e) => {
                    shared.sys_failed.store(true, Ordering::Relaxed);
                    if let Ok(mut m) = shared.sys_err.lock() {
                        *m = e.clone();
                    }
                    add_note(&shared, &format!("Device audio not recorded: {e}"));
                }
            }
        }
        // ---- pause handling: the clock stops, no frames/samples are recorded
        let is_paused = shared.paused.load(Ordering::SeqCst);
        match (is_paused, pause_started) {
            (true, None) => pause_started = Some(Instant::now()),
            (false, Some(t)) => {
                paused_total += t.elapsed();
                pause_started = None;
            }
            _ => {}
        }
        if is_paused {
            thread::sleep(Duration::from_millis(30));
            continue;
        }

        // ---- grab a frame
        if let Some((cap, sh)) = dxgi.as_mut() {
            match cap.frame() {
                Ok(frame) => {
                    let stride = frame.len() / *sh;
                    for y in 0..rh_u {
                        let src_start = (ay as usize + y) * stride + (ax as usize) * 4;
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
        } else if let Some(g) = gdi.as_mut() {
            let data = g.grab();
            if data.len() == last.len() {
                last.copy_from_slice(data);
            }
        }

        // ---- keep real-time speed: write as many frames as are "due" (paused time excluded)
        active = start.elapsed().saturating_sub(paused_total);
        shared
            .active_ms
            .store(active.as_millis() as u64, Ordering::Relaxed);
        let due = (active.as_secs_f64() * fps as f64) as u64 + 1;
        let n = due.saturating_sub(written).max(1).min(fps as u64);
        for _ in 0..n {
            if stdin.write_all(&last).is_err() {
                break 'outer; // ffmpeg exited; its error is read below
            }
            written += 1;
        }
        if let Some(t) = mic.as_mut() {
            t.pump(active.as_secs_f64());
        }
        if let Some(t) = sys.as_mut() {
            t.pump(active.as_secs_f64());
        }
        let target = start + paused_total + Duration::from_secs_f64(written as f64 / fps as f64);
        if let Some(wait) = target.checked_duration_since(Instant::now()) {
            thread::sleep(wait);
        }
    }

    // ---- finish: close the pipe, wait for the video file (with a time limit so Stop can never hang)
    drop(stdin);
    let status = wait_timeout(&mut child, 60);
    let ff_err = read_stderr(&mut child);
    let secs = active.as_secs_f64();
    let mic_info = mic.take().map(|t| t.finish(secs));
    let sys_info = sys.take().map(|t| t.finish(secs));

    if let Some(e) = error {
        let _ = std::fs::remove_file(&video_tmp);
        return Err(e);
    }
    match status {
        Ok(s) if s.success() => {}
        Ok(_) => {
            let _ = std::fs::remove_file(&video_tmp);
            return Err(format!("ffmpeg failed:\n{ff_err}"));
        }
        Err(e) => {
            let _ = std::fs::remove_file(&video_tmp);
            return Err(e);
        }
    }

    // ---- no audio: the video file is the result
    if mic_info.is_none() && sys_info.is_none() {
        std::fs::rename(&video_tmp, &out)
            .map_err(|e| format!("could not save the recording: {e}"))?;
        return Ok(out);
    }

    // ---- add the audio
    let mut mux = Command::new(&ff);
    mux.args(["-y", "-loglevel", "error", "-i"]).arg(&video_tmp);
    for info in [&mic_info, &sys_info].into_iter().flatten() {
        mux.args(["-f", "f32le", "-ar"]).arg(info.1.to_string());
        mux.arg("-ac").arg(info.2.to_string());
        mux.arg("-i").arg(&info.0);
    }
    if mic_info.is_some() && sys_info.is_some() {
        mux.args([
            "-filter_complex",
            "[1:a]aresample=48000,aformat=channel_layouts=stereo[a1];[2:a]aresample=48000,aformat=channel_layouts=stereo[a2];[a1][a2]amix=inputs=2:duration=longest,volume=2,alimiter=limit=0.95[a]",
            "-map",
            "0:v",
            "-map",
            "[a]",
        ]);
    } else {
        mux.args(["-map", "0:v", "-map", "1:a"]);
    }
    mux.args(["-c:v", "copy", "-c:a", "aac", "-b:a", "160k"]);
    mux.arg("-t").arg(format!("{:.3}", secs.max(0.1)));
    mux.args(["-movflags", "+faststart"]);
    mux.arg(&out);
    mux.stdout(Stdio::null()).stderr(Stdio::piped());
    no_window(&mut mux);

    let result = match mux.spawn() {
        Ok(mut c) => match wait_timeout(&mut c, 120) {
            Ok(s) if s.success() => Ok(()),
            Ok(_) => Err(read_stderr(&mut c)),
            Err(e) => Err(e),
        },
        Err(e) => Err(e.to_string()),
    };
    let _ = std::fs::remove_file(mic_info.as_ref().map(|i| i.0.clone()).unwrap_or_default());
    let _ = std::fs::remove_file(sys_info.as_ref().map(|i| i.0.clone()).unwrap_or_default());
    match result {
        Ok(()) => {
            let _ = std::fs::remove_file(&video_tmp);
            Ok(out)
        }
        Err(e) => {
            // keep the picture even if adding sound failed
            std::fs::rename(&video_tmp, &out)
                .map_err(|e2| format!("could not save the recording: {e2}"))?;
            add_note(&shared, &format!("Saved without sound ({e})"));
            Ok(out)
        }
    }
}
