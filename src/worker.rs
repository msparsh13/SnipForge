//! Capture worker: one short-lived process per capture. It grabs the screen FIRST,
//! then shows the selection overlay immediately, so nothing has to be clicked twice.
use std::path::PathBuf;
use std::process::Command;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Vec2};
use image::RgbaImage;

use crate::config::Settings;
use crate::editor::{Editor, EditorEvent};
use crate::record::{RecOptions, Recorder};
use crate::perms::{self, MicAccess};
use crate::stitch::{ScrollJob, ScrollOutcome};
use crate::ui::{self, ACCENT, ACCENT_SOFT, BG, BORDER, CARD, DANGER, OVERLAY, SUCCESS, TEXT_DIM, WARN};
use crate::{capture, ocr, render, setup, util, winapi};

#[derive(Clone, Copy, PartialEq)]
enum Purpose {
    Edit,
    Ocr,
    Record,
    Scroll,
    Pin,
}

/// What to start once the countdown has finished.
#[derive(Clone, Copy)]
enum Then {
    Record(ScreenRect),
    Scroll(ScreenRect),
}

/// What to capture: drag an area, everything, or one particular screen.
#[derive(Clone, Copy, PartialEq)]
enum Target {
    Area,
    Full,
    Monitor(usize), // 1-based
}

enum WMsg {
    Ocr(Result<String, String>),
    EditorOcr(Result<String, String>),
    Ffmpeg(Result<PathBuf, String>),
}

enum OcrPopup {
    Busy,
    Text(String),
    Failed(String),
}

enum WMode {
    Selecting {
        shot: Arc<RgbaImage>,
        tex: Option<egui::TextureHandle>,
        drag: Option<(Pos2, Pos2)>,
        purpose: Purpose,
    },
    Editing(Box<Editor>),
    OcrBusy,
    OcrDone(String),
    NeedFfmpeg { installing: bool, error: Option<String> },
    Recording { rec: Recorder, started: Instant, stopping: bool },
    RecDone(Result<PathBuf, String>),
    Message(String),
    Scrolling { job: ScrollJob },
    Countdown { total: u32, ends: Option<Instant>, then: Option<Then> },
    AudioPrompt { region: ScreenRect, reason: String },
}

#[derive(Clone, Copy)]
enum View {
    Selecting,
    Editing,
    OcrBusy,
    OcrDone,
    NeedFfmpeg,
    Recording,
    RecDone,
    Message,
    Countdown,
    Scrolling,
    AudioPrompt,
}

pub struct WorkerApp {
    mode: WMode,
    status: String,
    settings: Settings,
    font: Option<Arc<fontdue::Font>>,
    tx: mpsc::Sender<WMsg>,
    rx: mpsc::Receiver<WMsg>,
    inited: bool,
    screen_pts: Vec2,
    excluded: bool,
    ocr_popup: Option<OcrPopup>,
    progress: Arc<setup::Progress>,
    cover_frames: u32,
    virt: ScreenRect,
    monitors: Vec<ScreenRect>,
    kind: String, // which capture this process was started for (used to restart after installing ffmpeg)
    kind_arg: Option<String>,
    rec_region: Option<ScreenRect>,
    mic_vu: f32,
    sys_vu: f32,
    toast: Option<(String, Instant)>,
}

// ------------------------------------------------------------------ startup
type ScreenRect = (i32, i32, u32, u32); // x, y, width, height in screen pixels

struct Prepared {
    mode: WMode,
    overlay: Option<ScreenRect>, // Some(rect) = borderless overlay covering that area
    title: String,
    monitors: Vec<ScreenRect>,
}

fn msg_mode(m: String) -> Prepared {
    Prepared { mode: WMode::Message(m), overlay: None, title: "SnipForge".to_string(), monitors: Vec::new() }
}

/// Shows a small "3 - 2 - 1" window and waits for it. The window is closed before the screen is
/// grabbed, so it never appears in the screenshot. Esc / Cancel in that window aborts the capture.
fn run_countdown(secs: u32) {
    if let Ok(exe) = std::env::current_exe() {
        if let Ok(mut child) = Command::new(exe).arg("--worker").arg("countdown").arg(secs.to_string()).spawn() {
            if let Ok(status) = child.wait() {
                if status.code() == Some(2) {
                    std::process::exit(0); // cancelled by the user
                }
                return;
            }
        }
    }
    thread::sleep(Duration::from_secs(secs as u64)); // fallback: silent wait
}

fn parse_target(arg: &Option<String>) -> Target {
    match arg.as_deref() {
        Some("full") => Target::Full,
        Some(s) if s.starts_with('m') => s[1..].parse::<usize>().map(Target::Monitor).unwrap_or(Target::Area),
        _ => Target::Area,
    }
}

fn target_rect(t: Target, settings: &Settings) -> Result<ScreenRect, String> {
    match t {
        Target::Full => Ok(if settings.all_screens {
            winapi::virtual_rect()
        } else {
            let (pw, ph) = capture::primary_size().unwrap_or((1920, 1080));
            (0, 0, pw, ph)
        }),
        Target::Monitor(n) => winapi::monitors().get(n.wrapping_sub(1)).copied().ok_or_else(|| format!("Screen {n} is not connected.")),
        Target::Area => Err("no fixed area".to_string()),
    }
}

fn prepare(kind0: &str, arg: Option<String>, settings: &Settings, font: &Option<Arc<fontdue::Font>>, status: &mut String) -> Prepared {
    // "recordfull" is the old name for "record the whole screen"
    let (kind, arg) = if kind0 == "recordfull" { ("record", Some("full".to_string())) } else { (kind0, arg) };
    match kind {
        "open" => {
            let Some(p) = arg else { return msg_mode("No file given.".to_string()) };
            match image::open(&p) {
                Ok(img) => {
                    *status = format!("Editing {p}");
                    Prepared {
                        mode: WMode::Editing(Box::new(Editor::new(img.to_rgba8(), PathBuf::from(&p), font.clone()))),
                        overlay: None,
                        title: "SnipForge - Editor".to_string(),
                        monitors: Vec::new(),
                    }
                }
                Err(e) => msg_mode(format!("Could not open image: {e}")),
            }
        }
        "countdown" => {
            let secs: u32 = arg.as_deref().and_then(|a| a.parse().ok()).unwrap_or(3);
            Prepared {
                mode: WMode::Countdown { total: secs.max(1), ends: None, then: None },
                overlay: None,
                title: "SnipForge Countdown".to_string(),
                monitors: Vec::new(),
            }
        }
        "region" | "full" | "ocr" | "record" | "scroll" | "pinregion" => {
            let mut target = if matches!(kind, "full" | "record" | "scroll") { parse_target(&arg) } else { Target::Area };
            if kind == "full" && target == Target::Area {
                target = Target::Full;
            }
            if kind == "record" && setup::ffmpeg_path().is_none() {
                return Prepared {
                    mode: WMode::NeedFfmpeg { installing: false, error: None },
                    overlay: None,
                    title: "SnipForge - Setup".to_string(),
                    monitors: Vec::new(),
                };
            }
            winapi::minimize_other_windows();

            // recording / scrolling a fixed screen: no selection step, just the countdown card
            if matches!(kind, "record" | "scroll") && target != Target::Area {
                let rect = match target_rect(target, settings) {
                    Ok(r) => r,
                    Err(e) => return msg_mode(e),
                };
                thread::sleep(Duration::from_millis(350));
                let then = if kind == "record" { Then::Record(rect) } else { Then::Scroll(rect) };
                return Prepared {
                    mode: WMode::Countdown { total: settings.rec_countdown, ends: None, then: Some(then) },
                    overlay: None,
                    title: "SnipForge Countdown".to_string(),
                    monitors: Vec::new(),
                };
            }

            // screenshots honour the delay timer (with a visible countdown); recording and scrolling count down later
            if matches!(kind, "region" | "full" | "ocr" | "pinregion") && settings.delay > 0 {
                run_countdown(settings.delay);
                thread::sleep(Duration::from_millis(250));
            } else {
                thread::sleep(Duration::from_millis(450));
            }

            if kind == "full" {
                let rect = match target_rect(target, settings) {
                    Ok(r) => r,
                    Err(e) => return msg_mode(e),
                };
                let img = match capture::grab_rect(rect.0, rect.1, rect.2, rect.3) {
                    Ok(i) => i,
                    Err(e) => return msg_mode(format!("Capture failed: {e}")),
                };
                let path = settings.shots_path().join(format!("snip_{}.png", util::timestamp()));
                let _ = img.save(&path);
                if settings.auto_copy {
                    let _ = util::copy_image(&img);
                }
                *status = format!("Saved to {}", path.display());
                return Prepared {
                    mode: WMode::Editing(Box::new(Editor::new(img, path, font.clone()))),
                    overlay: None,
                    title: "SnipForge - Editor".to_string(),
                    monitors: Vec::new(),
                };
            }

            // drag-a-selection modes: grab the screen first, then show the overlay
            let (img, origin) = match capture::grab_screens(settings.all_screens) {
                Ok(v) => v,
                Err(e) => return msg_mode(format!("Capture failed: {e}")),
            };
            let purpose = match kind {
                "ocr" => Purpose::Ocr,
                "record" => Purpose::Record,
                "scroll" => Purpose::Scroll,
                "pinregion" => Purpose::Pin,
                _ => Purpose::Edit,
            };
            let rect = (origin.0, origin.1, img.width(), img.height());
            let mut monitors = winapi::monitors();
            // keep only monitors that are inside the captured area
            monitors.retain(|m| m.0 >= rect.0 && m.1 >= rect.1 && m.0 + m.2 as i32 <= rect.0 + rect.2 as i32 + 1 && m.1 + m.3 as i32 <= rect.1 + rect.3 as i32 + 1);
            Prepared {
                mode: WMode::Selecting { shot: Arc::new(img), tex: None, drag: None, purpose },
                overlay: Some(rect),
                title: "SnipForge - Select".to_string(),
                monitors,
            }
        }
        _ => msg_mode(format!("Unknown mode: {kind}")),
    }
}

pub fn run(kind: &str, arg: Option<String>) -> Result<(), eframe::Error> {
    let settings = Settings::load();
    let font = render::load_font().map(Arc::new);
    let mut status = String::new();
    let kind_arg = arg.clone();
    let prep = prepare(kind, arg, &settings, &font, &mut status);
    let Prepared { mode, overlay, title, monitors } = prep;

    let mut vb = egui::ViewportBuilder::default().with_title(title).with_inner_size([940.0, 640.0]);
    if let Some((x, y, w, h)) = overlay {
        // borderless window over ALL screens; exact size is enforced with Win32 after creation
        vb = vb
            .with_decorations(false)
            .with_position([x as f32, y as f32])
            .with_inner_size([w as f32, h as f32]);
    }
    if matches!(mode, WMode::Countdown { .. }) {
        vb = vb.with_decorations(false).with_inner_size([190.0, 220.0]);
    }
    let kind_s = kind.to_string();
    let virt = overlay.unwrap_or((0, 0, 0, 0));
    let options = eframe::NativeOptions { viewport: vb, ..Default::default() };
    eframe::run_native(
        "SnipForge",
        options,
        Box::new(move |cc| {
            ui::apply_theme(&cc.egui_ctx);
            let (tx, rx) = mpsc::channel();
            Box::new(WorkerApp {
                mode,
                status,
                settings,
                font,
                tx,
                rx,
                inited: false,
                screen_pts: vec2(1920.0, 1080.0),
                excluded: false,
                ocr_popup: None,
                progress: Arc::new(setup::Progress::default()),
                cover_frames: 0,
                virt,
                monitors,
                kind: kind_s,
                kind_arg,
                rec_region: None,
                mic_vu: 0.0,
                sys_vu: 0.0,
                toast: None,
            })
        }),
    )
}

// ------------------------------------------------------------------ helpers
impl WorkerApp {
    fn view(&self) -> View {
        match &self.mode {
            WMode::Selecting { .. } => View::Selecting,
            WMode::Editing(_) => View::Editing,
            WMode::OcrBusy => View::OcrBusy,
            WMode::OcrDone(_) => View::OcrDone,
            WMode::NeedFfmpeg { .. } => View::NeedFfmpeg,
            WMode::Recording { .. } => View::Recording,
            WMode::RecDone(_) => View::RecDone,
            WMode::Message(_) => View::Message,
            WMode::Countdown { .. } => View::Countdown,
            WMode::Scrolling { .. } => View::Scrolling,
            WMode::AudioPrompt { .. } => View::AudioPrompt,
        }
    }

    fn windowed(&self, ctx: &egui::Context, size: Vec2, title: &str) {
        winapi::release_topmost("SnipForge - Select");
        winapi::release_topmost("SnipForge Recording");
        winapi::release_topmost("SnipForge Scrolling");
        winapi::release_topmost("SnipForge Countdown");
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.to_string()));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        let sr = self.screen_pts;
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos2(
            ((sr.x - size.x) / 2.0).max(0.0),
            ((sr.y - size.y) / 2.0).max(0.0),
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    fn editor_size(&self, ctx: &egui::Context, iw: u32, ih: u32) -> Vec2 {
        let ppp = ctx.pixels_per_point();
        let sr = self.screen_pts;
        vec2(
            (iw as f32 / ppp + 130.0).clamp(860.0, (sr.x * 0.94).max(860.0)),
            (ih as f32 / ppp + 170.0).clamp(580.0, (sr.y * 0.92).max(580.0)),
        )
    }

    fn show_editor_window(&self, ctx: &egui::Context) {
        if let WMode::Editing(ed) = &self.mode {
            let (w, h) = ed.image_size();
            self.windowed(ctx, self.editor_size(ctx, w, h), "SnipForge - Editor");
            let ppp = ctx.pixels_per_point();
            let big = (w as f32 / ppp) > self.screen_pts.x * 0.55 || (h as f32 / ppp) > self.screen_pts.y * 0.55;
            if big {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            }
        }
    }

    fn init(&mut self, ctx: &egui::Context) {
        self.inited = true;
        let ppp = ctx.pixels_per_point();
        if let Some((w, h)) = capture::primary_size() {
            self.screen_pts = vec2(w as f32 / ppp, h as f32 / ppp);
        }
        // recording with the microphone: if Windows blocks it, ask the person before counting down
        let mut prompt: Option<(ScreenRect, String)> = None;
        if let WMode::Countdown { then: Some(Then::Record(r)), .. } = &self.mode {
            if self.settings.mic {
                if let MicAccess::Blocked(reason) = perms::mic_access() {
                    prompt = Some((*r, reason));
                }
            }
        }
        if let Some((region, reason)) = prompt {
            self.mode = WMode::AudioPrompt { region, reason };
            self.windowed(ctx, vec2(560.0, 400.0), "SnipForge - Microphone");
            return;
        }
        match self.view() {
            View::Selecting => ctx.send_viewport_cmd(egui::ViewportCommand::Focus),
            View::Editing => self.show_editor_window(ctx),
            View::Recording | View::RecDone | View::Scrolling => {}
            View::Countdown => {
                if let WMode::Countdown { total, ends, .. } = &mut self.mode {
                    *ends = Some(Instant::now() + Duration::from_secs(*total as u64));
                }
                self.place_countdown(ctx);
            }
            _ => self.windowed(ctx, vec2(520.0, 300.0), "SnipForge"),
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                WMsg::Ocr(res) => match res {
                    Ok(text) if text.is_empty() => self.mode = WMode::Message("No text was found in that area.".to_string()),
                    Ok(text) => {
                        let _ = util::copy_text(&text);
                        self.status = self.save_text_note(&text);
                        self.mode = WMode::OcrDone(text);
                    }
                    Err(e) => self.mode = WMode::Message(e),
                },
                WMsg::EditorOcr(res) => {
                    self.ocr_popup = Some(match res {
                        Ok(t) if t.is_empty() => OcrPopup::Failed("No text was found in the image.".to_string()),
                        Ok(t) => {
                            let _ = util::copy_text(&t);
                            self.status = self.save_text_note(&t);
                            OcrPopup::Text(t)
                        }
                        Err(e) => OcrPopup::Failed(e),
                    });
                }
                WMsg::Ffmpeg(res) => match res {
                    Ok(_) => {
                        util::spawn_worker(&self.kind, self.kind_arg.as_deref());
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    Err(e) => self.mode = WMode::NeedFfmpeg { installing: false, error: Some(e) },
                },
            }
        }

        // recording progress / stop requests
        // scrolling capture progress / finish
        let mut scroll_done: Option<Result<ScrollOutcome, String>> = None;
        if let WMode::Scrolling { job } = &mut self.mode {
            util::touch_marker();
            if util::take_stop_request() {
                job.request_stop();
            }
            if !self.excluded && winapi::exclude_from_capture("SnipForge Scrolling") {
                self.excluded = true;
            }
            scroll_done = job.poll();
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        if let Some(res) = scroll_done {
            util::clear_marker();
            match res {
                Ok(o) => {
                    self.open_new_image(ctx, o.image);
                    if !o.note.is_empty() {
                        self.status = format!("{}   Note: {}", self.status, o.note);
                    }
                }
                Err(e) => {
                    self.mode = WMode::Message(e);
                    self.windowed(ctx, vec2(520.0, 300.0), "SnipForge");
                }
            }
            return;
        }

        let mut finished: Option<Result<PathBuf, String>> = None;
        let mut rec_note = String::new();
        let mut want_snap = false;
        if let WMode::Recording { rec, stopping, .. } = &mut self.mode {
            util::touch_marker();
            if util::take_stop_request() {
                rec.request_stop();
                *stopping = true;
            }
            if util::take_pause_request() {
                rec.toggle_pause();
            }
            if util::take_snap_request() {
                want_snap = true;
            }
            finished = rec.poll();
            if finished.is_some() {
                rec_note = rec.note();
            }
            if !self.excluded && winapi::exclude_from_capture("SnipForge Recording") {
                self.excluded = true;
            }
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        if want_snap {
            self.take_rec_screenshot();
        }
        if let Some(res) = finished {
            util::clear_marker();
            self.status = rec_note;
            self.mode = WMode::RecDone(res);
            self.windowed(ctx, vec2(500.0, 250.0), "SnipForge - Recording");
        }
    }

    fn save_text_note(&self, text: &str) -> String {
        if self.settings.save_ocr_text {
            let path = self.settings.text_path().join(format!("text_{}.txt", util::timestamp()));
            match std::fs::write(&path, text) {
                Ok(()) => format!("Copied to clipboard and saved to {}", path.display()),
                Err(e) => format!("Copied to clipboard (could not save the file: {e})"),
            }
        } else {
            "Copied to clipboard".to_string()
        }
    }

    // ------------------------------------------------------------------ selection result
    fn finish_selection(&mut self, ctx: &egui::Context, shot: Arc<RgbaImage>, purpose: Purpose, sel: Rect, rect: Rect) {
        let (sw, sh) = (shot.width(), shot.height());
        let ratio = sw as f32 / rect.width();
        let x = (((sel.min.x - rect.min.x) * ratio).max(0.0) as u32).min(sw.saturating_sub(1));
        let y = (((sel.min.y - rect.min.y) * ratio).max(0.0) as u32).min(sh.saturating_sub(1));
        let w = ((sel.width() * ratio) as u32).min(sw - x);
        let h = ((sel.height() * ratio) as u32).min(sh - y);
        if w < 4 || h < 4 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        match purpose {
            Purpose::Edit => {
                let img = capture::crop(&shot, x, y, w, h);
                let path = self.settings.shots_path().join(format!("snip_{}.png", util::timestamp()));
                let _ = img.save(&path);
                if self.settings.auto_copy {
                    let _ = util::copy_image(&img);
                }
                self.status = format!("Saved to {}   (Ctrl+Z undo, Ctrl+S save, Ctrl+C copy)", path.display());
                self.mode = WMode::Editing(Box::new(Editor::new(img, path, self.font.clone())));
                self.show_editor_window(ctx);
            }
            Purpose::Ocr => {
                let img = capture::crop(&shot, x, y, w, h);
                self.mode = WMode::OcrBusy;
                self.windowed(ctx, vec2(560.0, 420.0), "SnipForge - Text");
                let tx = self.tx.clone();
                let ctx2 = ctx.clone();
                thread::spawn(move || {
                    let _ = tx.send(WMsg::Ocr(ocr::ocr(&img)));
                    ctx2.request_repaint();
                });
            }
            Purpose::Record => {
                let abs = (x as i32 + self.virt.0, y as i32 + self.virt.1, w, h);
                self.begin_record_countdown(ctx, abs);
            }
            Purpose::Scroll => {
                let abs = (x as i32 + self.virt.0, y as i32 + self.virt.1, w, h);
                self.begin_scroll_countdown(ctx, abs);
            }
            Purpose::Pin => {
                let img = capture::crop(&shot, x, y, w, h);
                let path = util::temp_png("snipforge_pin");
                if img.save(&path).is_ok() {
                    let p = path.to_string_lossy().to_string();
                    util::spawn_worker("pin", Some(&p));
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// Shows the 3-2-1 card, then starts recording (skipped when the countdown is set to 0).
    /// Recording: check the microphone permission first, then count down.
    fn begin_record_countdown(&mut self, ctx: &egui::Context, region: ScreenRect) {
        if self.settings.mic {
            if let MicAccess::Blocked(reason) = perms::mic_access() {
                self.mode = WMode::AudioPrompt { region, reason };
                self.windowed(ctx, vec2(560.0, 400.0), "SnipForge - Microphone");
                return;
            }
        }
        self.begin_countdown(ctx, Then::Record(region));
    }

    fn begin_scroll_countdown(&mut self, ctx: &egui::Context, region: ScreenRect) {
        self.begin_countdown(ctx, Then::Scroll(region));
    }

    /// Shows the 3-2-1 card, then starts (skipped when the countdown is set to 0).
    fn begin_countdown(&mut self, ctx: &egui::Context, then: Then) {
        let secs = self.settings.rec_countdown;
        if secs == 0 {
            self.run_then(ctx, then);
            return;
        }
        self.mode = WMode::Countdown { total: secs, ends: Some(Instant::now() + Duration::from_secs(secs as u64)), then: Some(then) };
        self.cover_frames = 0;
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Title("SnipForge Countdown".to_string()));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(190.0, 220.0)));
        self.place_countdown(ctx);
    }

    fn run_then(&mut self, ctx: &egui::Context, then: Then) {
        match then {
            Then::Record(r) => self.start_recording(ctx, r),
            Then::Scroll(r) => self.start_scroll(ctx, r),
        }
    }

    /// Save a picture of the area being recorded, without stopping the recording.
    fn take_rec_screenshot(&mut self) {
        let Some((x, y, w, h)) = self.rec_region else { return };
        let msg = match capture::grab_rect(x, y, w, h) {
            Ok(img) => {
                let path = self.settings.shots_path().join(format!("snip_{}.png", util::timestamp()));
                match img.save(&path) {
                    Ok(()) => {
                        if self.settings.auto_copy {
                            let _ = util::copy_image(&img);
                        }
                        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                        format!("Screenshot saved: {name}")
                    }
                    Err(e) => format!("Could not save the screenshot: {e}"),
                }
            }
            Err(e) => format!("Screenshot failed: {e}"),
        };
        self.toast = Some((msg, Instant::now()));
    }

    fn ui_audio_prompt(&mut self, ctx: &egui::Context) {
        #[derive(Clone, Copy)]
        enum A {
            Settings,
            Retry,
            WithoutMic,
            Cancel,
        }
        let mut act: Option<A> = None;
        egui::CentralPanel::default().frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::same(24.0))).show(ctx, |ui| {
            if let WMode::AudioPrompt { reason, .. } = &self.mode {
                ui.label(RichText::new("Windows is blocking the microphone").size(22.0).strong().color(ACCENT));
                ui.add_space(6.0);
                ui.label(reason.as_str());
                ui.add_space(10.0);
                ui.label("To record your voice, switch it on in Windows:");
                ui.label("1.  Press \"Open microphone settings\" below.");
                ui.label("2.  Turn on \"Microphone access\" and \"Let desktop apps access your microphone\".");
                ui.label("3.  Come back here and press \"Try again\".");
                ui.add_space(14.0);
                ui.horizontal_wrapped(|ui| {
                    if ui::primary_button(ui, "Open microphone settings").clicked() {
                        act = Some(A::Settings);
                    }
                    if ui.button("Try again").clicked() {
                        act = Some(A::Retry);
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Record without the microphone").clicked() {
                        act = Some(A::WithoutMic);
                    }
                    if ui.button("Cancel").clicked() {
                        act = Some(A::Cancel);
                    }
                });
                ui.add_space(10.0);
                ui.label(RichText::new("Device audio (what you hear) does not need this permission.").size(12.0).color(TEXT_DIM));
            }
        });
        let region = if let WMode::AudioPrompt { region, .. } = &self.mode { Some(*region) } else { None };
        match (act, region) {
            (Some(A::Settings), _) => perms::open_mic_settings(),
            (Some(A::Retry), Some(r)) => match perms::mic_access() {
                MicAccess::Allowed => self.begin_record_countdown(ctx, r),
                MicAccess::Blocked(reason) => self.mode = WMode::AudioPrompt { region: r, reason },
            },
            (Some(A::WithoutMic), Some(r)) => {
                self.settings.mic = false;
                self.begin_record_countdown(ctx, r);
            }
            (Some(A::Cancel), _) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            _ => {}
        }
    }

    /// Save a freshly captured image, copy it if asked, and open the editor.
    fn open_new_image(&mut self, ctx: &egui::Context, img: RgbaImage) {
        let path = self.settings.shots_path().join(format!("snip_{}.png", util::timestamp()));
        let _ = img.save(&path);
        if self.settings.auto_copy {
            let _ = util::copy_image(&img);
        }
        self.status = format!("Saved to {}", path.display());
        self.mode = WMode::Editing(Box::new(Editor::new(img, path, self.font.clone())));
        self.show_editor_window(ctx);
    }

    fn start_scroll(&mut self, ctx: &egui::Context, region: ScreenRect) {
        util::touch_marker();
        self.excluded = false;
        let job = ScrollJob::start(ctx.clone(), region);
        self.mode = WMode::Scrolling { job };
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Title("SnipForge Scrolling".to_string()));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(640.0, 62.0)));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos2(((self.screen_pts.x - 640.0) / 2.0).max(0.0), 8.0)));
    }

    fn ui_scrolling(&mut self, ctx: &egui::Context) {
        let mut stop = false;
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(CARD).stroke(Stroke::new(1.0, BORDER)).inner_margin(egui::Margin::symmetric(14.0, 8.0)))
            .show(ctx, |ui| {
                if let WMode::Scrolling { job } = &self.mode {
                    ui.horizontal_centered(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(format!("Scrolling: {} steps, {} px tall", job.steps(), job.rows())).size(15.0).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui::danger_button(ui, "Stop and save").clicked() {
                                stop = true;
                            }
                            ui.label(RichText::new("keep the pointer over the page").size(11.0).color(TEXT_DIM));
                        });
                    });
                }
            });
        if stop {
            if let WMode::Scrolling { job } = &self.mode {
                job.request_stop();
            }
        }
    }

    fn start_recording(&mut self, ctx: &egui::Context, region: ScreenRect) {
        let Some(ff) = setup::ffmpeg_path() else {
            self.mode = WMode::NeedFfmpeg { installing: false, error: None };
            self.windowed(ctx, vec2(520.0, 300.0), "SnipForge - Setup");
            return;
        };
        let out = self.settings.videos_path().join(format!("rec_{}.mp4", util::timestamp()));
        let opts = RecOptions { fps: self.settings.fps, mic: self.settings.mic, system: self.settings.system_audio };
        let rec = Recorder::start(ctx.clone(), region, opts, out, ff);
        util::touch_marker();
        self.excluded = false;
        self.rec_region = Some(region);
        self.mode = WMode::Recording { rec, started: Instant::now(), stopping: false };
        // shrink to a small floating bar at the top centre
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Title("SnipForge Recording".to_string()));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(880.0, 62.0)));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos2(((self.screen_pts.x - 880.0) / 2.0).max(0.0), 8.0)));
    }

    fn place_countdown(&self, ctx: &egui::Context) {
        let ppp = ctx.pixels_per_point();
        if let Some((pw, _)) = capture::primary_size() {
            let (w, h) = ((190.0 * ppp) as i32, (220.0 * ppp) as i32);
            winapi::cover_screen("SnipForge Countdown", (pw as i32 - w) / 2, (28.0 * ppp) as i32, w, h);
        }
    }

    fn ui_countdown(&mut self, ctx: &egui::Context) {
        if self.cover_frames < 30 {
            self.cover_frames += 1;
            self.place_countdown(ctx); // small, centred at the top, above other windows
        }
        let mut done = false;
        let mut then_action: Option<Then> = None;
        let mut cancel = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if let WMode::Countdown { total, ends: Some(end), then } = &self.mode {
            then_action = *then;
            let heading = match then {
                Some(Then::Record(_)) => "Recording starts in",
                Some(Then::Scroll(_)) => "Scrolling starts in",
                None => "Capturing in",
            };
            let now = Instant::now();
            let left = if now >= *end { Duration::ZERO } else { *end - now };
            let left_s = left.as_secs_f32();
            let frac = if *total == 0 { 0.0 } else { (left_s / *total as f32).clamp(0.0, 1.0) };
            let shown = (left_s.ceil() as u32).max(1);
            done = now >= *end;
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(BG).stroke(Stroke::new(1.0, BORDER)))
                .show(ctx, |ui| {
                    let rect = ui.max_rect();
                    let p = ui.painter_at(rect);
                    p.text(pos2(rect.center().x, rect.min.y + 22.0), Align2::CENTER_CENTER, heading, FontId::proportional(14.0), TEXT_DIM);
                    let c = pos2(rect.center().x, rect.min.y + 98.0);
                    p.circle_stroke(c, 58.0, Stroke::new(8.0, BORDER));
                    let n = 72;
                    let sweep = frac * std::f32::consts::TAU;
                    let pts: Vec<Pos2> = (0..=n)
                        .map(|i| {
                            let a = -std::f32::consts::FRAC_PI_2 + sweep * (i as f32 / n as f32);
                            pos2(c.x + 58.0 * a.cos(), c.y + 58.0 * a.sin())
                        })
                        .collect();
                    p.add(Shape::line(pts, Stroke::new(8.0, ACCENT)));
                    p.text(c, Align2::CENTER_CENTER, format!("{shown}"), FontId::proportional(60.0), ACCENT);
                    let btn = Rect::from_center_size(pos2(rect.center().x, rect.max.y - 26.0), vec2(96.0, 30.0));
                    if ui.put(btn, egui::Button::new("Cancel")).clicked() {
                        cancel = true;
                    }
                });
        }
        if cancel {
            if then_action.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close); // nothing was started yet
            } else {
                std::process::exit(2); // tells the capture process to stop
            }
            return;
        }
        if done {
            match then_action {
                Some(t) => {
                    self.run_then(ctx, t);
                    return;
                }
                None => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
        ctx.request_repaint_after(Duration::from_millis(40));
    }

    fn ui_selecting(&mut self, ctx: &egui::Context) {
        let mut result: Option<Option<(Rect, Rect)>> = None; // Some(None)=cancelled
        let monitors = self.monitors.clone();
        let virt = self.virt;
        if let WMode::Selecting { shot, tex, drag, purpose } = &mut self.mode {
            if tex.is_none() {
                let ci = ui::color_image(&**shot);
                *tex = Some(ctx.load_texture("overlay", ci, egui::TextureOptions::LINEAR));
            }
            let hint = match purpose {
                Purpose::Edit => "Drag to select an area to capture",
                Purpose::Ocr => "Drag over the text you want to copy",
                Purpose::Record => "Drag to select the area to record",
                Purpose::Scroll => "Drag over the scrollable area (it scrolls by itself)",
                Purpose::Pin => "Drag the area to pin on your screen",
            };
            let shot_w = shot.width() as f32;
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
            // force the overlay to cover ALL screens exactly, above the taskbar (re-applied if Windows resizes it)
            let ppp = ctx.pixels_per_point();
            let sr = ctx.screen_rect();
            let cw = (sr.width() * ppp).round() as i32;
            let ch = (sr.height() * ppp).round() as i32;
            if self.cover_frames < 150 && (self.cover_frames < 6 || (cw - virt.2 as i32).abs() > 2 || (ch - virt.3 as i32).abs() > 2) {
                self.cover_frames += 1;
                winapi::cover_screen("SnipForge - Select", virt.0, virt.1, virt.2 as i32, virt.3 as i32);
                ctx.request_repaint();
            }
            let multi = monitors.len() > 1;
            let extra = if multi { format!("   A = all screens, 1-{} = one screen", monitors.len().min(5)) } else { String::new() };
            egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
                let rect = ui.max_rect();
                let resp = ui.allocate_rect(rect, Sense::drag());
                let painter = ui.painter_at(rect);
                if let Some(t) = tex.as_ref() {
                    painter.image(t.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                let dim = Color32::from_black_alpha(125);

                // keyboard shortcuts: A = every screen, 1..5 = that one screen
                let kratio = shot_w / rect.width();
                let mut pick: Option<Rect> = None;
                if ui.input(|i| i.key_pressed(egui::Key::A)) {
                    pick = Some(rect);
                }
                let nums = [egui::Key::Num1, egui::Key::Num2, egui::Key::Num3, egui::Key::Num4, egui::Key::Num5];
                for (idx, key) in nums.iter().enumerate() {
                    if ui.input(|i| i.key_pressed(*key)) {
                        if let Some(m) = monitors.get(idx) {
                            let min = rect.min + vec2((m.0 - virt.0) as f32 / kratio, (m.1 - virt.1) as f32 / kratio);
                            pick = Some(Rect::from_min_size(min, vec2(m.2 as f32 / kratio, m.3 as f32 / kratio)));
                        }
                    }
                }
                if let Some(sel) = pick {
                    result = Some(Some((sel, rect)));
                }

                if resp.drag_started() {
                    if let Some(p) = resp.interact_pointer_pos() {
                        *drag = Some((p, p));
                    }
                }
                if resp.dragged() {
                    if let (Some(d), Some(p)) = (drag.as_mut(), resp.interact_pointer_pos()) {
                        d.1 = p;
                    }
                }
                match *drag {
                    Some((a, b)) => {
                        let sel = Rect::from_two_pos(a, b);
                        painter.rect_filled(Rect::from_min_max(rect.min, pos2(rect.max.x, sel.min.y)), 0.0, dim);
                        painter.rect_filled(Rect::from_min_max(pos2(rect.min.x, sel.max.y), rect.max), 0.0, dim);
                        painter.rect_filled(Rect::from_min_max(pos2(rect.min.x, sel.min.y), pos2(sel.min.x, sel.max.y)), 0.0, dim);
                        painter.rect_filled(Rect::from_min_max(pos2(sel.max.x, sel.min.y), pos2(rect.max.x, sel.max.y)), 0.0, dim);
                        painter.rect_stroke(sel, 0.0, Stroke::new(2.0, OVERLAY));
                        let ratio = shot_w / rect.width();
                        let label = format!("{} x {}", (sel.width() * ratio) as u32, (sel.height() * ratio) as u32);
                        let chip_pos = pos2(sel.min.x, (sel.min.y - 26.0).max(rect.min.y + 2.0));
                        let galley_rect = Rect::from_min_size(chip_pos, vec2(92.0, 22.0));
                        painter.rect_filled(galley_rect, 5.0, Color32::from_rgb(20, 22, 26));
                        painter.text(galley_rect.center(), Align2::CENTER_CENTER, label, FontId::proportional(13.0), Color32::WHITE);
                    }
                    None => {
                        painter.rect_filled(rect, 0.0, dim);
                        if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
                            let c = Stroke::new(1.0, Color32::from_white_alpha(120));
                            painter.line_segment([pos2(rect.min.x, p.y), pos2(rect.max.x, p.y)], c);
                            painter.line_segment([pos2(p.x, rect.min.y), pos2(p.x, rect.max.y)], c);
                        }
                        if multi {
                            for (i, m) in monitors.iter().enumerate() {
                                let min = rect.min + vec2((m.0 - virt.0) as f32 / kratio, (m.1 - virt.1) as f32 / kratio);
                                let badge = Rect::from_min_size(min + vec2(12.0, 12.0), vec2(30.0, 30.0));
                                painter.rect_filled(badge, 8.0, Color32::from_rgb(20, 22, 26));
                                painter.text(badge.center(), Align2::CENTER_CENTER, format!("{}", i + 1), FontId::proportional(18.0), Color32::WHITE);
                            }
                        }
                        let chip_w = if multi { 800.0 } else { 470.0 };
                        let chip = Rect::from_center_size(rect.center_top() + vec2(0.0, 52.0), vec2(chip_w, 44.0));
                        painter.rect_filled(chip, 22.0, Color32::from_rgb(20, 22, 26));
                        painter.text(
                            chip.center(),
                            Align2::CENTER_CENTER,
                            format!("{hint}{extra}     Esc to cancel"),
                            FontId::proportional(17.0),
                            Color32::WHITE,
                        );
                    }
                }
                if resp.drag_released() {
                    if let Some((a, b)) = *drag {
                        result = Some(Some((Rect::from_two_pos(a, b), rect)));
                    }
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    result = Some(None);
                }
            });
        }

        if let Some(res) = result {
            let old = std::mem::replace(&mut self.mode, WMode::OcrBusy);
            if let WMode::Selecting { shot, purpose, .. } = old {
                match res {
                    None => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                    Some((sel, rect)) => self.finish_selection(ctx, shot, purpose, sel, rect),
                }
            }
        }
    }

    fn ui_editing(&mut self, ctx: &egui::Context) {
        let ev = if let WMode::Editing(ed) = &mut self.mode {
            ed.ui(ctx, &mut self.status)
        } else {
            EditorEvent::None
        };
        match ev {
            EditorEvent::None => {}
            EditorEvent::NewRegion => util::spawn_worker("region", None),
            EditorEvent::NewFull => util::spawn_worker("full", None),
            EditorEvent::ExtractText(img) => self.start_editor_ocr(ctx, img),
        }
        self.draw_ocr_popup(ctx);
    }

    fn start_editor_ocr(&mut self, ctx: &egui::Context, img: RgbaImage) {
        self.ocr_popup = Some(OcrPopup::Busy);
        let tx = self.tx.clone();
        let ctx2 = ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(WMsg::EditorOcr(ocr::ocr(&img)));
            ctx2.request_repaint();
        });
    }

    fn draw_ocr_popup(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let note = self.status.clone();
        if let Some(p) = &mut self.ocr_popup {
            egui::Window::new("Extracted text").collapsible(false).default_width(460.0).show(ctx, |ui| match p {
                OcrPopup::Busy => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Reading text...");
                    });
                }
                OcrPopup::Text(t) => {
                    ui.label(RichText::new(note.as_str()).size(12.0).color(TEXT_DIM));
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        ui.add(egui::TextEdit::multiline(t).desired_width(f32::INFINITY).desired_rows(8));
                    });
                    ui.horizontal(|ui| {
                        if ui::primary_button(ui, "Copy").clicked() {
                            let _ = util::copy_text(t);
                        }
                        if ui.button("Close").clicked() {
                            close = true;
                        }
                    });
                }
                OcrPopup::Failed(e) => {
                    ui.colored_label(DANGER, e.as_str());
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                }
            });
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        if close {
            self.ocr_popup = None;
        }
    }

    fn ui_ocr_busy(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.spinner();
                ui.add_space(8.0);
                ui.label(RichText::new("Reading text...").size(18.0));
            });
        });
        ctx.request_repaint_after(Duration::from_millis(150));
    }

    fn ui_ocr_done(&mut self, ctx: &egui::Context) {
        let mut close = false;
        egui::CentralPanel::default().frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::same(16.0))).show(ctx, |ui| {
            ui.label(RichText::new("Text extracted").size(18.0).strong().color(ACCENT));
            ui.label(RichText::new(self.status.as_str()).size(12.0).color(TEXT_DIM));
            ui.add_space(4.0);
            if let WMode::OcrDone(text) = &mut self.mode {
                egui::ScrollArea::vertical().max_height(ui.available_height() - 52.0).show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(text).desired_width(f32::INFINITY).desired_rows(12));
                });
                ui.horizontal(|ui| {
                    if ui::primary_button(ui, "Copy again").clicked() {
                        let _ = util::copy_text(text);
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            }
        });
        if close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn ui_need_ffmpeg(&mut self, ctx: &egui::Context) {
        let mut install = false;
        let mut pick = false;
        let mut page = false;
        let mut close = false;
        egui::CentralPanel::default().frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::same(20.0))).show(ctx, |ui| {
            ui.label(RichText::new("One-time setup for recording").size(20.0).strong().color(ACCENT));
            ui.add_space(6.0);
            ui.label("Screen recording needs a small free component (ffmpeg, about 90 MB). SnipForge downloads and sets it up for you.");
            ui.add_space(10.0);
            if let WMode::NeedFfmpeg { installing, error } = &self.mode {
                if *installing {
                    ui.add(egui::ProgressBar::new(self.progress.fraction()).text(self.progress.text()));
                } else {
                    if let Some(e) = error {
                        ui.colored_label(DANGER, e.as_str());
                        ui.add_space(4.0);
                    }
                    let label = if error.is_some() { "Try again" } else { "Download and install" };
                    ui.horizontal_wrapped(|ui| {
                        if ui::primary_button(ui, label).clicked() {
                            install = true;
                        }
                        if ui.button("Choose ffmpeg.exe...").clicked() {
                            pick = true;
                        }
                        if ui.button("Open download page").clicked() {
                            page = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                }
            }
        });
        if install {
            self.mode = WMode::NeedFfmpeg { installing: true, error: None };
            let tx = self.tx.clone();
            let ctx2 = ctx.clone();
            let prog = self.progress.clone();
            prog.reset();
            thread::spawn(move || {
                let _ = tx.send(WMsg::Ffmpeg(setup::install_ffmpeg(prog)));
                ctx2.request_repaint();
            });
        }
        if page {
            util::open_url(setup::DOWNLOAD_PAGE);
        }
        if pick {
            if let Some(p) = rfd::FileDialog::new().add_filter("ffmpeg", &["exe"]).pick_file() {
                match setup::import_ffmpeg(&p) {
                    Ok(_) => {
                        util::spawn_worker(&self.kind, self.kind_arg.as_deref());
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    Err(e) => self.mode = WMode::NeedFfmpeg { installing: false, error: Some(e) },
                }
            }
        }
        if close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    fn ui_recording(&mut self, ctx: &egui::Context) {
        #[derive(Clone, Copy, PartialEq)]
        enum Act {
            Stop,
            Pause,
            Mic,
            Sys,
            Shot,
            FixMic,
        }
        let mut act: Option<Act> = None;
        // smooth the level meters so they are readable
        let (ml, sl) = if let WMode::Recording { rec, .. } = &self.mode { (rec.mic_level(), rec.sys_level()) } else { (0.0, 0.0) };
        self.mic_vu = ml.max(self.mic_vu * 0.88);
        self.sys_vu = sl.max(self.sys_vu * 0.88);
        let (mic_vu, sys_vu) = (self.mic_vu, self.sys_vu);
        let toast = self.toast.as_ref().filter(|(_, t)| t.elapsed() < Duration::from_secs(3)).map(|(s, _)| s.clone());

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(CARD).stroke(Stroke::new(1.0, BORDER)).inner_margin(egui::Margin::symmetric(14.0, 8.0)))
            .show(ctx, |ui| {
                if let WMode::Recording { rec, started, stopping } = &self.mode {
                    let secs = rec.active_secs() as u64;
                    let paused = rec.paused();
                    ui.horizontal_centered(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                        let blink = (started.elapsed().as_millis() / 500) % 2 == 0;
                        let col = if paused { WARN } else { DANGER };
                        ui.painter().circle_filled(r.center(), 6.0, if paused || blink { col } else { col.gamma_multiply(0.35) });
                        let label = if paused { "Paused" } else { "Recording" };
                        ui.label(RichText::new(format!("{label}  {:02}:{:02}", secs / 60, secs % 60)).size(15.0).strong());
                        if let Some(t) = &toast {
                            ui.label(RichText::new(t.as_str()).size(12.0).color(SUCCESS));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if *stopping {
                                ui.label("Saving...");
                                return;
                            }
                            if ui::danger_button(ui, "Stop").clicked() {
                                act = Some(Act::Stop);
                            }
                            if ui.button("Screenshot").on_hover_text("Save a picture of the recorded area (Ctrl+Alt+K)").clicked() {
                                act = Some(Act::Shot);
                            }
                            if ui.button(if paused { "Resume" } else { "Pause" }).clicked() {
                                act = Some(Act::Pause);
                            }
                            ui.separator();
                            let sys_ok = !rec.sys_failed();
                            let r = chip(ui, "Audio", rec.sys_on(), sys_ok);
                            if !sys_ok {
                                r.on_disabled_hover_text(rec.sys_error());
                            } else if r.clicked() {
                                act = Some(Act::Sys);
                            }
                            meter(ui, sys_vu, rec.sys_on() && sys_ok);
                            ui.separator();
                            let mic_ok = !rec.mic_failed();
                            if !mic_ok && ui.small_button("Fix mic").on_hover_text("Open the Windows microphone settings").clicked() {
                                act = Some(Act::FixMic);
                            }
                            let r = chip(ui, "Mic", rec.mic_on(), mic_ok);
                            if !mic_ok {
                                r.on_disabled_hover_text(rec.mic_error());
                            } else if r.clicked() {
                                act = Some(Act::Mic);
                            }
                            meter(ui, mic_vu, rec.mic_on() && mic_ok);
                        });
                    });
                }
            });
        if let WMode::Recording { rec, stopping, .. } = &mut self.mode {
            match act {
                Some(Act::Stop) => {
                    rec.request_stop();
                    *stopping = true;
                }
                Some(Act::Pause) => rec.toggle_pause(),
                Some(Act::Mic) => rec.toggle_mic(),
                Some(Act::Sys) => rec.toggle_sys(),
                _ => {}
            }
        }
        match act {
            Some(Act::Shot) => self.take_rec_screenshot(),
            Some(Act::FixMic) => perms::open_mic_settings(),
            _ => {}
        }
        ctx.request_repaint_after(Duration::from_millis(60)); // keeps the level meters moving
    }

    fn ui_rec_done(&mut self, ctx: &egui::Context) {
        let mut close = false;
        egui::CentralPanel::default().frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::same(20.0))).show(ctx, |ui| {
            if let WMode::RecDone(res) = &self.mode {
                match res {
                    Ok(p) => {
                        ui.label(RichText::new("Recording saved").size(20.0).strong().color(ACCENT));
                        ui.add_space(4.0);
                        ui.label(p.display().to_string());
                        if !self.status.is_empty() {
                            ui.colored_label(WARN, self.status.as_str());
                        }
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui::primary_button(ui, "Play").clicked() {
                                util::open_file(p);
                            }
                            if ui.button("Edit video").clicked() {
                                util::spawn_worker_path("video", p);
                            }
                            if ui.button("Show in folder").clicked() {
                                util::reveal_in_explorer(p);
                            }
                            if ui.button("Close").clicked() {
                                close = true;
                            }
                        });
                    }
                    Err(e) => {
                        ui.label(RichText::new("Recording failed").size(20.0).strong().color(DANGER));
                        ui.add_space(4.0);
                        ui.label(e.as_str());
                        ui.add_space(10.0);
                        if ui.button("Close").clicked() {
                            close = true;
                        }
                    }
                }
            }
        });
        if close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn ui_message(&mut self, ctx: &egui::Context) {
        let mut close = false;
        egui::CentralPanel::default().frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::same(20.0))).show(ctx, |ui| {
            if let WMode::Message(m) = &self.mode {
                ui.label(RichText::new("SnipForge").size(20.0).strong().color(ACCENT));
                ui.add_space(6.0);
                ui.label(m.as_str());
                ui.add_space(10.0);
                if ui.button("Close").clicked() {
                    close = true;
                }
            }
        });
        if close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let _ = CARD;
    }
}

/// Small on/off toggle used on the recording bar.
fn chip(ui: &mut egui::Ui, name: &str, on: bool, usable: bool) -> egui::Response {
    if !usable {
        return ui.add_enabled(false, egui::Button::new(format!("{name} n/a")));
    }
    let (fill, col) = if on { (ACCENT_SOFT, ACCENT) } else { (Color32::from_rgb(236, 238, 243), TEXT_DIM) };
    let text = format!("{name} {}", if on { "on" } else { "off" });
    ui.add(egui::Button::new(RichText::new(text).color(col)).fill(fill))
}

/// A thin live level meter (so you can see at once whether sound is arriving).
fn meter(ui: &mut egui::Ui, level: f32, live: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(44.0, 8.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, Color32::from_rgb(226, 230, 238));
    let w = rect.width() * level.clamp(0.0, 1.0).sqrt(); // sqrt: quiet sounds still show
    if w > 0.5 {
        p.rect_filled(Rect::from_min_size(rect.min, vec2(w, rect.height())), 4.0, if live { SUCCESS } else { TEXT_DIM });
    }
}

impl eframe::App for WorkerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.inited {
            self.init(ctx);
        }
        self.poll(ctx);
        match self.view() {
            View::Selecting => self.ui_selecting(ctx),
            View::Editing => self.ui_editing(ctx),
            View::OcrBusy => self.ui_ocr_busy(ctx),
            View::OcrDone => self.ui_ocr_done(ctx),
            View::NeedFfmpeg => self.ui_need_ffmpeg(ctx),
            View::Recording => self.ui_recording(ctx),
            View::RecDone => self.ui_rec_done(ctx),
            View::Message => self.ui_message(ctx),
            View::Countdown => self.ui_countdown(ctx),
            View::Scrolling => self.ui_scrolling(ctx),
            View::AudioPrompt => self.ui_audio_prompt(ctx),
        }
    }
}
