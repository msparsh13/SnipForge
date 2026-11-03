//! SnipForge (Rust edition) - screenshot, annotate, OCR and record on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod editor;
mod hotkeys;
mod ocr;
mod record;
mod render;
mod util;
mod ui;

use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke};
use image::RgbaImage;

use editor::{Editor, EditorEvent};
use hotkeys::Cmd;
use record::Recorder;

#[derive(Clone, Copy, PartialEq)]
enum Purpose {
    Edit,
    Ocr,
    Record,
}

enum Msg {
    Shot { img: Result<RgbaImage, String>, purpose: Purpose, full: bool },
    Ocr(Result<String, String>),
}

enum Mode {
    Home,
    Waiting,
    Selecting {
        shot: Arc<RgbaImage>,
        tex: egui::TextureHandle,
        drag: Option<(Pos2, Pos2)>,
        purpose: Purpose,
    },
    Editing(Box<Editor>),
}

enum Act {
    Region,
    Full,
    Ocr,
    Record,
    StopRecord,
    OpenImage,
    OpenFolder,
}

struct App {
    mode: Mode,
    status: String,
    tx: mpsc::Sender<Msg>,
    rx: mpsc::Receiver<Msg>,
    _hk_manager: Option<global_hotkey::GlobalHotKeyManager>,
    font: Option<Arc<fontdue::Font>>,
    delay: u32,
    fps: u32,
    mic: bool,
    auto_copy: bool,
    recorder: Option<(Recorder, Instant)>,
    ocr_text: String,
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([560.0, 640.0])
            .with_title("SnipForge"),
        ..Default::default()
    };
    eframe::run_native("SnipForge", options, Box::new(|cc| Box::new(App::new(cc))))
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = mpsc::channel();
        let mgr = hotkeys::setup();
        App {
            mode: Mode::Home,
            status: "Ready.  Hotkeys: Ctrl+Alt+A region, F full screen, T grab text, R record".to_string(),
            tx,
            rx,
            _hk_manager: mgr,
            font: render::load_font().map(Arc::new),
            delay: 0,
            fps: 30,
            mic: true,
            auto_copy: true,
            recorder: None,
            ocr_text: String::new(),
        }
    }

    // ------------------------------------------------------------ capture flow
    fn begin_capture(&mut self, ctx: &egui::Context, purpose: Purpose, full: bool) {
        if !matches!(self.mode, Mode::Home | Mode::Editing(_)) {
            return;
        }
        if self.recorder.is_some() {
            self.status = "A recording is in progress. Stop it first (Ctrl+Alt+R).".to_string();
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        self.mode = Mode::Waiting;
        let tx = self.tx.clone();
        let ctx2 = ctx.clone();
        let delay_ms = 450 + self.delay as u64 * 1000;
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(delay_ms));
            let img = capture::grab_primary();
            let _ = tx.send(Msg::Shot { img, purpose, full });
            ctx2.request_repaint();
        });
    }

    fn restore_window(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    fn open_editor(&mut self, ctx: &egui::Context, img: RgbaImage, path: std::path::PathBuf, is_new: bool) {
        if is_new {
            let _ = img.save(&path);
            if self.auto_copy {
                let _ = util::copy_image(&img);
            }
        }
        self.status = if is_new {
            format!("Saved to {}", path.display())
        } else {
            format!("Editing {}", path.display())
        };
        self.mode = Mode::Editing(Box::new(Editor::new(img, path, self.font.clone())));
        self.restore_window(ctx);
    }

    fn handle_msg(&mut self, ctx: &egui::Context, m: Msg) {
        match m {
            Msg::Shot { img, purpose, full } => match img {
                Err(e) => {
                    self.status = format!("Capture failed: {e}");
                    self.mode = Mode::Home;
                    self.restore_window(ctx);
                }
                Ok(img) => {
                    if full && purpose == Purpose::Edit {
                        let path = util::captures_dir().join(format!("snip_{}.png", util::timestamp()));
                        self.open_editor(ctx, img, path, true);
                    } else {
                        let ci = egui::ColorImage::from_rgba_unmultiplied(
                            [img.width() as usize, img.height() as usize],
                            img.as_raw(),
                        );
                        let tex = ctx.load_texture("overlay", ci, egui::TextureOptions::LINEAR);
                        self.mode = Mode::Selecting { shot: Arc::new(img), tex, drag: None, purpose };
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                }
            },
            Msg::Ocr(res) => {
                match res {
                    Ok(text) if text.is_empty() => self.status = "No text found.".to_string(),
                    Ok(text) => {
                        let _ = util::copy_text(&text);
                        self.status = "Text copied to clipboard.".to_string();
                        self.ocr_text = text;
                    }
                    Err(e) => self.status = e,
                }
                self.mode = Mode::Home;
                self.restore_window(ctx);
            }
        }
    }

    fn finish_selection(&mut self, ctx: &egui::Context, shot: Arc<RgbaImage>, purpose: Purpose, sel: Rect, rect: Rect) {
        let (sw, sh) = (shot.width(), shot.height());
        let ratio = sw as f32 / rect.width();
        let x = (((sel.min.x - rect.min.x) * ratio).max(0.0) as u32).min(sw.saturating_sub(1));
        let y = (((sel.min.y - rect.min.y) * ratio).max(0.0) as u32).min(sh.saturating_sub(1));
        let w = ((sel.width() * ratio) as u32).min(sw - x);
        let h = ((sel.height() * ratio) as u32).min(sh - y);
        if w < 4 || h < 4 {
            self.mode = Mode::Home;
            self.restore_window(ctx);
            return;
        }
        match purpose {
            Purpose::Edit => {
                let img = capture::crop(&shot, x, y, w, h);
                let path = util::captures_dir().join(format!("snip_{}.png", util::timestamp()));
                self.open_editor(ctx, img, path, true);
            }
            Purpose::Ocr => {
                let img = capture::crop(&shot, x, y, w, h);
                self.status = "Reading text...".to_string();
                self.mode = Mode::Home;
                self.restore_window(ctx);
                let tx = self.tx.clone();
                let ctx2 = ctx.clone();
                thread::spawn(move || {
                    let _ = tx.send(Msg::Ocr(ocr::ocr(&img)));
                    ctx2.request_repaint();
                });
            }
            Purpose::Record => {
                let out = util::captures_dir().join(format!("rec_{}.mp4", util::timestamp()));
let ffmpeg = util::ffmpeg_path();

let rec = Recorder::start(
    ctx.clone(),
    (x, y, w, h),
    self.fps,
    self.mic,
    out,
    ffmpeg,
);
                self.recorder = Some((rec, Instant::now()));
                self.status = "Recording...  press Ctrl+Alt+R to stop (or restore this window)".to_string();
                self.mode = Mode::Home;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
        }
    }

    fn toggle_record(&mut self, ctx: &egui::Context) {
        if let Some((rec, _)) = &self.recorder {
            rec.request_stop();
            self.status = "Saving recording...".to_string();
        } else {
            self.begin_capture(ctx, Purpose::Record, false);
        }
    }

    // ------------------------------------------------------------ event pump
    fn pump(&mut self, ctx: &egui::Context) {
       
        while let Ok(m) = self.rx.try_recv() {
            self.handle_msg(ctx, m);
        }
        let mut finished = None;
        if let Some((rec, _)) = &mut self.recorder {
            finished = rec.poll();
        }
        if let Some(res) = finished {
            self.recorder = None;
            match res {
                Ok(p) => self.status = format!("Recording saved: {}", p.display()),
                Err(e) => self.status = format!("Recording failed: {e}"),
            }
            self.restore_window(ctx);
        }
        if self.recorder.is_some() {
            ctx.request_repaint_after(Duration::from_millis(300));
        }
    }

    // ------------------------------------------------------------ screens
    fn ui_home(&mut self, ctx: &egui::Context) {
        let mut act: Option<Act> = None;
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("SnipForge");
            ui.label(self.status.as_str());
            ui.add_space(8.0);

            let recording = self.recorder.is_some();
            let big = |ui: &mut egui::Ui, text: &str| ui.add_sized([ui.available_width(), 42.0], egui::Button::new(text)).clicked();
            if big(ui, "Region capture   (Ctrl+Alt+A)") {
                act = Some(Act::Region);
            }
            if big(ui, "Full screen   (Ctrl+Alt+F)") {
                act = Some(Act::Full);
            }
            if big(ui, "Grab text with OCR   (Ctrl+Alt+T)") {
                act = Some(Act::Ocr);
            }
            if recording {
                let secs = self.recorder.as_ref().map(|(_, t)| t.elapsed().as_secs()).unwrap_or(0);
                if big(ui, &format!("Stop recording   ({:02}:{:02})", secs / 60, secs % 60)) {
                    act = Some(Act::StopRecord);
                }
            } else if big(ui, "Record region   (Ctrl+Alt+R)") {
                act = Some(Act::Record);
            }
            ui.horizontal(|ui| {
                if ui.button("Open image to edit...").clicked() {
                    act = Some(Act::OpenImage);
                }
                if ui.button("Open captures folder").clicked() {
                    act = Some(Act::OpenFolder);
                }
            });

            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Delay (s):");
                ui.add(egui::Slider::new(&mut self.delay, 0..=15));
                ui.label("FPS:");
                egui::ComboBox::from_id_source("fps")
                    .selected_text(self.fps.to_string())
                    .show_ui(ui, |ui| {
                        for f in [10u32, 15, 24, 30, 60] {
                            ui.selectable_value(&mut self.fps, f, f.to_string());
                        }
                    });
            });
            ui.checkbox(&mut self.mic, "Record microphone with screen recordings");
            ui.checkbox(&mut self.auto_copy, "Copy screenshots to clipboard automatically");

            if !self.ocr_text.is_empty() {
                ui.separator();
                ui.label("Extracted text (already copied):");
                ui.add(egui::TextEdit::multiline(&mut self.ocr_text).desired_rows(6).desired_width(f32::INFINITY));
            }
        });

        match act {
            Some(Act::Region) => self.begin_capture(ctx, Purpose::Edit, false),
            Some(Act::Full) => self.begin_capture(ctx, Purpose::Edit, true),
            Some(Act::Ocr) => self.begin_capture(ctx, Purpose::Ocr, false),
            Some(Act::Record) => self.begin_capture(ctx, Purpose::Record, false),
            Some(Act::StopRecord) => self.toggle_record(ctx),
            Some(Act::OpenFolder) => util::open_in_explorer(&util::captures_dir()),
            Some(Act::OpenImage) => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("Images", &["png", "jpg", "jpeg", "bmp"])
                    .pick_file()
                {
                    match image::open(&p) {
                        Ok(i) => self.open_editor(ctx, i.to_rgba8(), p, false),
                        Err(e) => self.status = format!("Could not open image: {e}"),
                    }
                }
            }
            None => {}
        }
    }

    fn ui_selecting(&mut self, ctx: &egui::Context) {
        let mut result: Option<Option<(Rect, Rect)>> = None; // Some(None)=cancelled, Some(Some)=(selection, full rect)
        if let Mode::Selecting { tex, drag, .. } = &mut self.mode {
            egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
                let rect = ui.max_rect();
                let resp = ui.allocate_rect(rect, Sense::drag());
                let painter = ui.painter_at(rect);
                painter.image(tex.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                let dim = Color32::from_black_alpha(130);

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
                        painter.rect_stroke(sel, 0.0, Stroke::new(2.0, Color32::from_rgb(76, 194, 255)));
                    }
                    None => {
                        painter.rect_filled(rect, 0.0, dim);
                        painter.text(
                            rect.center_top() + vec2(0.0, 40.0),
                            Align2::CENTER_TOP,
                            "Drag to select an area   |   Esc to cancel",
                            FontId::proportional(24.0),
                            Color32::WHITE,
                        );
                    }
                }
                if resp.drag_stopped() {
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
            let old = std::mem::replace(&mut self.mode, Mode::Home);
            if let Mode::Selecting { shot, purpose, .. } = old {
                match res {
                    None => self.restore_window(ctx),
                    Some((sel, rect)) => self.finish_selection(ctx, shot, purpose, sel, rect),
                }
            }
        }
    }

    fn ui_editing(&mut self, ctx: &egui::Context) {
        let ev = if let Mode::Editing(ed) = &mut self.mode {
            ed.ui(ctx, &mut self.status)
        } else {
            EditorEvent::None
        };
        match ev {
            EditorEvent::None => {}
            EditorEvent::NewRegion => self.begin_capture(ctx, Purpose::Edit, false),
            EditorEvent::NewFull => self.begin_capture(ctx, Purpose::Edit, true),
            EditorEvent::Home => self.mode = Mode::Home,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump(ctx);
        match self.mode {
            Mode::Home => self.ui_home(ctx),
            Mode::Waiting => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.heading("Capturing...");
                });
            }
            Mode::Selecting { .. } => self.ui_selecting(ctx),
            Mode::Editing(_) => self.ui_editing(ctx),
        }
    }
}
