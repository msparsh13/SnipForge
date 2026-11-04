//! Capture worker: one short-lived process per capture. It grabs the screen FIRST,
//! then shows the selection overlay immediately, so nothing has to be clicked twice.
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use image::RgbaImage;

use crate::config::Settings;
use crate::editor::{Editor, EditorEvent};
use crate::record::Recorder;
use crate::ui::{self, ACCENT, BG, CARD, DANGER};
use crate::{capture, ocr, render, setup, util, winapi};

#[derive(Clone, Copy, PartialEq)]
enum Purpose {
    Edit,
    Ocr,
    Record,
}

enum WMsg {
    Ocr(Result<String, String>),
    Ffmpeg(Result<PathBuf, String>),
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
}

// ------------------------------------------------------------------ startup
fn msg_mode(m: String) -> (WMode, bool, String) {
    (WMode::Message(m), false, "SnipForge".to_string())
}

fn prepare(kind: &str, arg: Option<String>, settings: &Settings, font: &Option<Arc<fontdue::Font>>, status: &mut String) -> (WMode, bool, String) {
    match kind {
        "open" => {
            let Some(p) = arg else { return msg_mode("No file given.".to_string()) };
            match image::open(&p) {
                Ok(img) => {
                    *status = format!("Editing {p}");
                    (
                        WMode::Editing(Box::new(Editor::new(img.to_rgba8(), PathBuf::from(&p), font.clone()))),
                        false,
                        "SnipForge - Editor".to_string(),
                    )
                }
                Err(e) => msg_mode(format!("Could not open image: {e}")),
            }
        }
        "region" | "ocr" | "record" | "full" => {
            if kind == "record" && setup::ffmpeg_path().is_none() {
                return (WMode::NeedFfmpeg { installing: false, error: None }, false, "SnipForge - Setup".to_string());
            }
            winapi::minimize_other_windows();
            thread::sleep(Duration::from_millis(450 + settings.delay as u64 * 1000));
            let img = match capture::grab_primary() {
                Ok(i) => i,
                Err(e) => return msg_mode(format!("Capture failed: {e}")),
            };
            if kind == "full" {
                let path = util::captures_dir().join(format!("snip_{}.png", util::timestamp()));
                let _ = img.save(&path);
                if settings.auto_copy {
                    let _ = util::copy_image(&img);
                }
                *status = format!("Saved to {}", path.display());
                (WMode::Editing(Box::new(Editor::new(img, path, font.clone()))), false, "SnipForge - Editor".to_string())
            } else {
                let purpose = match kind {
                    "ocr" => Purpose::Ocr,
                    "record" => Purpose::Record,
                    _ => Purpose::Edit,
                };
                (
                    WMode::Selecting { shot: Arc::new(img), tex: None, drag: None, purpose },
                    true,
                    "SnipForge - Select".to_string(),
                )
            }
        }
        _ => msg_mode(format!("Unknown mode: {kind}")),
    }
}

pub fn run(kind: &str, arg: Option<String>) -> Result<(), eframe::Error> {
    let settings = Settings::load();
    let font = render::load_font().map(Arc::new);
    let mut status = String::new();
    let (mode, fullscreen, title) = prepare(kind, arg, &settings, &font, &mut status);

    let mut vb = egui::ViewportBuilder::default().with_title(title).with_inner_size([940.0, 640.0]);
    if fullscreen {
        vb = vb.with_fullscreen(true).with_decorations(false);
    }
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
        }
    }

    fn windowed(&self, ctx: &egui::Context, size: Vec2, title: &str) {
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
        }
    }

    fn init(&mut self, ctx: &egui::Context) {
        self.inited = true;
        let ppp = ctx.pixels_per_point();
        if let Some((w, h)) = capture::primary_size() {
            self.screen_pts = vec2(w as f32 / ppp, h as f32 / ppp);
        }
        match self.view() {
            View::Selecting => ctx.send_viewport_cmd(egui::ViewportCommand::Focus),
            View::Editing => self.show_editor_window(ctx),
            View::Recording | View::RecDone => {}
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
                        self.mode = WMode::OcrDone(text);
                    }
                    Err(e) => self.mode = WMode::Message(e),
                },
                WMsg::Ffmpeg(res) => match res {
                    Ok(_) => {
                        util::spawn_worker("record", None);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    Err(e) => self.mode = WMode::NeedFfmpeg { installing: false, error: Some(e) },
                },
            }
        }

        // recording progress / stop requests
        let mut finished: Option<Result<PathBuf, String>> = None;
        if let WMode::Recording { rec, stopping, .. } = &mut self.mode {
            util::touch_marker();
            if util::take_stop_request() {
                rec.request_stop();
                *stopping = true;
            }
            finished = rec.poll();
            if !self.excluded && winapi::exclude_from_capture("SnipForge Recording") {
                self.excluded = true;
            }
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        if let Some(res) = finished {
            util::clear_marker();
            self.mode = WMode::RecDone(res);
            self.windowed(ctx, vec2(500.0, 250.0), "SnipForge - Recording");
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
                let path = util::captures_dir().join(format!("snip_{}.png", util::timestamp()));
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
            Purpose::Record => self.start_recording(ctx, (x, y, w, h)),
        }
    }

    fn start_recording(&mut self, ctx: &egui::Context, region: (u32, u32, u32, u32)) {
        let Some(ff) = setup::ffmpeg_path() else {
            self.mode = WMode::NeedFfmpeg { installing: false, error: None };
            self.windowed(ctx, vec2(520.0, 300.0), "SnipForge - Setup");
            return;
        };
        let out = util::captures_dir().join(format!("rec_{}.mp4", util::timestamp()));
        let rec = Recorder::start(ctx.clone(), region, self.settings.fps, self.settings.mic, out, ff);
        util::touch_marker();
        self.mode = WMode::Recording { rec, started: Instant::now(), stopping: false };
        // shrink to a small floating bar at the top centre
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Title("SnipForge Recording".to_string()));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(330.0, 58.0)));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos2(((self.screen_pts.x - 330.0) / 2.0).max(0.0), 8.0)));
    }

    // ------------------------------------------------------------------ screens
    fn ui_selecting(&mut self, ctx: &egui::Context) {
        let mut result: Option<Option<(Rect, Rect)>> = None; // Some(None)=cancelled
        if let WMode::Selecting { shot, tex, drag, purpose } = &mut self.mode {
            if tex.is_none() {
                let ci = egui::ColorImage::from_rgba_unmultiplied([shot.width() as usize, shot.height() as usize], shot.as_raw());
                *tex = Some(ctx.load_texture("overlay", ci, egui::TextureOptions::LINEAR));
            }
            let hint = match purpose {
                Purpose::Edit => "Drag to select an area to capture",
                Purpose::Ocr => "Drag over the text you want to copy",
                Purpose::Record => "Drag to select the area to record",
            };
            let shot_w = shot.width() as f32;
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
            egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
                let rect = ui.max_rect();
                let resp = ui.allocate_rect(rect, Sense::drag());
                let painter = ui.painter_at(rect);
                if let Some(t) = tex.as_ref() {
                    painter.image(t.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                let dim = Color32::from_black_alpha(125);

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
                        painter.rect_stroke(sel, 0.0, Stroke::new(2.0, ACCENT));
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
                        let chip = Rect::from_center_size(rect.center_top() + vec2(0.0, 52.0), vec2(460.0, 44.0));
                        painter.rect_filled(chip, 22.0, Color32::from_rgb(20, 22, 26));
                        painter.text(
                            chip.center(),
                            Align2::CENTER_CENTER,
                            format!("{hint}     Esc to cancel"),
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
            ui.label(RichText::new("Text copied to clipboard").size(18.0).strong().color(ACCENT));
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
        let mut close = false;
        egui::CentralPanel::default().frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::same(20.0))).show(ctx, |ui| {
            ui.label(RichText::new("One-time setup for recording").size(20.0).strong().color(ACCENT));
            ui.add_space(6.0);
            ui.label("Screen recording needs a small free component (ffmpeg, about 90 MB). SnipForge can download and set it up for you automatically.");
            ui.add_space(10.0);
            if let WMode::NeedFfmpeg { installing, error } = &self.mode {
                if *installing {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Downloading... this can take a minute.");
                    });
                } else {
                    if let Some(e) = error {
                        ui.colored_label(DANGER, e.as_str());
                    }
                    ui.horizontal(|ui| {
                        if ui::primary_button(ui, "Download and install").clicked() {
                            install = true;
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
            thread::spawn(move || {
                let _ = tx.send(WMsg::Ffmpeg(setup::install_ffmpeg()));
                ctx2.request_repaint();
            });
        }
        if close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(Duration::from_millis(300));
    }

    fn ui_recording(&mut self, ctx: &egui::Context) {
        let mut stop = false;
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::from_rgb(28, 30, 34)).inner_margin(egui::Margin::symmetric(14.0, 10.0)))
            .show(ctx, |ui| {
                if let WMode::Recording { started, stopping, .. } = &self.mode {
                    let secs = started.elapsed().as_secs();
                    ui.horizontal_centered(|ui| {
                        let blink = (started.elapsed().as_millis() / 500) % 2 == 0;
                        let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 6.0, if blink { DANGER } else { DANGER.gamma_multiply(0.35) });
                        ui.label(RichText::new(format!("REC  {:02}:{:02}", secs / 60, secs % 60)).size(17.0).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if *stopping {
                                ui.label("Saving...");
                            } else if ui::danger_button(ui, "Stop").clicked() {
                                stop = true;
                            }
                        });
                    });
                }
            });
        if stop {
            if let WMode::Recording { rec, stopping, .. } = &mut self.mode {
                rec.request_stop();
                *stopping = true;
            }
        }
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
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui::primary_button(ui, "Play").clicked() {
                                util::open_file(p);
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
        }
    }
}
