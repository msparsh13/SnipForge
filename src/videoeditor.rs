//! Video editor: preview, cut ranges out, speed, captions (by hand or .srt) and export.
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, RichText, Sense, Shape, Stroke};

use crate::captions::{fmt_time, keeps_intersect, keeps_subtract, map_captions, read_srt, write_srt, Caption};
use crate::media::{self, ExportJob, ExportParams, Fmt, PlayJob, Quality, VideoInfo};
use crate::ui::{self, ACCENT, BG, BORDER, DANGER, PANEL, TEXT, TEXT_DIM};
use crate::{setup, util};

const SPEEDS: [f64; 6] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];
const CAP_ASS_SIZES: [u32; 3] = [16, 22, 30];
const CAP_NAMES: [&str; 3] = ["Small", "Medium", "Large"];

struct ErrorApp(String);

impl eframe::App for ErrorApp {
    fn update(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Video editor");
            ui.label(self.0.as_str());
            if ui.button("Close").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
}

pub fn run(arg: Option<String>) -> Result<(), eframe::Error> {
    let path = match arg {
        Some(p) => PathBuf::from(p),
        None => match rfd::FileDialog::new().add_filter("Video", &["mp4", "webm", "mkv", "mov", "avi"]).pick_file() {
            Some(p) => p,
            None => return Ok(()),
        },
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("SnipForge - Video editor").with_inner_size([1320.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "SnipForge",
        options,
        Box::new(move |cc| {
            ui::apply_theme(&cc.egui_ctx);
            let app: Box<dyn eframe::App> = match VideoApp::new(cc.egui_ctx.clone(), path) {
                Ok(a) => Box::new(a),
                Err(e) => Box::new(ErrorApp(e)),
            };
            app
        }),
    )
}

struct VideoApp {
    ff: PathBuf,
    path: PathBuf,
    info: VideoInfo,
    pw: u32,
    ph: u32,
    keeps: Vec<(f64, f64)>,
    undo: Vec<Vec<(f64, f64)>>,
    sel: Option<(f64, f64)>,
    anchor: Option<f64>,
    caps: Vec<Caption>,
    cap_size: usize,
    cap_top: bool,
    preview_caps: bool,
    playhead: f64,
    speed_idx: usize,
    frame_tex: Option<egui::TextureHandle>,
    req_tx: mpsc::Sender<f64>,
    frame_rx: mpsc::Receiver<egui::ColorImage>,
    play: Option<PlayJob>,
    export: Option<ExportJob>,
    last_export: Option<PathBuf>,
    fmt: Fmt,
    res: u32,
    quality: Quality,
    burn: bool,
    save_srt: bool,
    status: String,
    last_save: Instant,
    srt_path: PathBuf,
    ctx: egui::Context,
}

impl VideoApp {
    fn new(ctx: egui::Context, path: PathBuf) -> Result<VideoApp, String> {
        let ff = setup::ffmpeg_path().ok_or_else(|| {
            "The recording engine (ffmpeg) is needed to edit videos.\nOpen SnipForge and press 'Install now' in the Components box.".to_string()
        })?;
        let info = media::probe(&ff, &path)?;
        let pw = info.width.min(640) & !1;
        let ph = (((info.height as f64 * pw as f64 / info.width as f64) as u32) & !1).max(2);

        let (req_tx, req_rx) = mpsc::channel::<f64>();
        let (frame_tx, frame_rx) = mpsc::channel::<egui::ColorImage>();
        {
            let (ff2, p2, c2) = (ff.clone(), path.clone(), ctx.clone());
            thread::spawn(move || {
                while let Ok(mut t) = req_rx.recv() {
                    while let Ok(n) = req_rx.try_recv() {
                        t = n; // only the newest scrub position matters
                    }
                    if let Some(img) = media::grab_frame(&ff2, &p2, t, pw, ph) {
                        if frame_tx.send(img).is_err() {
                            break;
                        }
                        c2.request_repaint();
                    }
                }
            });
        }
        let srt_path = path.with_extension("srt");
        let caps = if srt_path.exists() { read_srt(&srt_path).unwrap_or_default() } else { Vec::new() };
        let _ = req_tx.send(0.0);
        let keeps = vec![(0.0, info.duration)];
        Ok(VideoApp {
            ff,
            path,
            info,
            pw,
            ph,
            keeps,
            undo: Vec::new(),
            sel: None,
            anchor: None,
            caps,
            cap_size: 1,
            cap_top: false,
            preview_caps: true,
            playhead: 0.0,
            speed_idx: 2,
            frame_tex: None,
            req_tx,
            frame_rx,
            play: None,
            export: None,
            last_export: None,
            fmt: Fmt::Mp4,
            res: 0,
            quality: Quality::Balanced,
            burn: true,
            save_srt: false,
            status: "Drag on the timeline to select a range, then delete it or keep only it.".to_string(),
            last_save: Instant::now(),
            srt_path,
            ctx,
        })
    }

    fn speed(&self) -> f64 {
        SPEEDS[self.speed_idx]
    }

    fn total_kept(&self) -> f64 {
        self.keeps.iter().map(|(s, e)| e - s).sum()
    }

    fn in_keeps(&self, t: f64) -> bool {
        self.keeps.iter().any(|&(s, e)| t >= s - 0.001 && t < e)
    }

    fn next_keep_start(&self, t: f64) -> Option<f64> {
        self.keeps.iter().find(|&&(s, _)| s > t).map(|&(s, _)| s)
    }

    // ------------------------------------------------------------ playback
    fn stop_play(&mut self) {
        if let Some(p) = self.play.take() {
            p.stop();
        }
    }

    fn seek(&mut self, t: f64) {
        self.stop_play();
        self.playhead = t.clamp(0.0, self.info.duration);
        let _ = self.req_tx.send(self.playhead);
    }

    fn start_play(&mut self) {
        let mut t = self.playhead;
        if !self.in_keeps(t) {
            t = self.next_keep_start(t).or_else(|| self.keeps.first().map(|k| k.0)).unwrap_or(0.0);
        }
        self.playhead = t;
        self.play = Some(PlayJob::start(self.ctx.clone(), self.ff.clone(), self.path.clone(), t, self.pw, self.ph, self.speed()));
    }

    fn tick_play(&mut self, ctx: &egui::Context) {
        let mut ended = false;
        let mut frame = None;
        let mut t = self.playhead;
        if let Some(p) = &self.play {
            loop {
                match p.rx.try_recv() {
                    Ok(ci) => frame = Some(ci),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        ended = true;
                        break;
                    }
                }
            }
            t = p.start_t + p.frames.load(Ordering::SeqCst) as f64 / media::PREVIEW_FPS as f64;
        }
        if let Some(ci) = frame {
            self.frame_tex = Some(ctx.load_texture("frame", ci, egui::TextureOptions::LINEAR));
        }
        if self.play.is_some() {
            self.playhead = t.min(self.info.duration);
            if ended || t >= self.info.duration - 0.05 {
                self.stop_play();
                return;
            }
            if !self.in_keeps(t) {
                // skip over a cut-out part
                match self.next_keep_start(t) {
                    Some(s) => {
                        self.stop_play();
                        self.playhead = s;
                        self.start_play();
                    }
                    None => self.stop_play(),
                }
            }
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    // ------------------------------------------------------------ editing
    fn delete_selection(&mut self) {
        let Some((a, b)) = self.sel else {
            self.status = "Drag on the timeline to select the part to delete first.".to_string();
            return;
        };
        let n = keeps_subtract(&self.keeps, a, b);
        if n.is_empty() {
            self.status = "That would remove everything.".to_string();
            return;
        }
        self.undo.push(self.keeps.clone());
        self.keeps = n;
        self.sel = None;
    }

    fn keep_selection(&mut self) {
        let Some((a, b)) = self.sel else {
            self.status = "Drag on the timeline to select the part to keep first.".to_string();
            return;
        };
        let n = keeps_intersect(&self.keeps, a, b);
        if n.is_empty() {
            self.status = "The selection is inside a part you already cut.".to_string();
            return;
        }
        self.undo.push(self.keeps.clone());
        self.keeps = n;
        self.sel = None;
    }

    // ------------------------------------------------------------ export
    fn start_export(&mut self, ctx: &egui::Context) {
        let stem = self.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "video".to_string());
        let dir = self.path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let Some(mut out) = rfd::FileDialog::new()
            .set_directory(dir)
            .set_file_name(format!("{stem}_edited.{}", self.fmt.ext()))
            .add_filter(self.fmt.label(), &[self.fmt.ext()])
            .save_file()
        else {
            return;
        };
        if out.extension().is_none() {
            out = out.with_extension(self.fmt.ext());
        }
        let speed = self.speed();
        let captions = map_captions(&self.caps, &self.keeps, speed);
        if self.save_srt && !captions.is_empty() {
            let _ = write_srt(&captions, &out.with_extension("srt"));
        }
        let params = ExportParams {
            ff: self.ff.clone(),
            src: self.path.clone(),
            out,
            keeps: self.keeps.clone(),
            speed,
            fmt: self.fmt,
            scale_h: self.res,
            quality: self.quality,
            src_w: self.info.width,
            src_h: self.info.height,
            has_audio: self.info.has_audio,
            captions,
            burn: self.burn,
            cap_size: CAP_ASS_SIZES[self.cap_size],
            cap_top: self.cap_top,
        };
        self.export = Some(ExportJob::start(ctx.clone(), params));
        self.status = "Exporting...".to_string();
    }

    // ------------------------------------------------------------ timeline
    fn ui_timeline(&mut self, ui: &mut egui::Ui) {
        let dur = self.info.duration.max(0.1);
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 88.0), Sense::click_and_drag());
        let (l, r) = (rect.min.x + 10.0, rect.max.x - 10.0);
        let x_of = |t: f64| l + (t / dur) as f32 * (r - l);
        let t_of = |x: f32| (((x - l) / (r - l)) as f64).clamp(0.0, 1.0) * dur;

        if resp.drag_started() {
            if let Some(p) = resp.interact_pointer_pos() {
                self.anchor = Some(t_of(p.x));
            }
        }
        if resp.dragged() {
            if let (Some(a), Some(p)) = (self.anchor, resp.interact_pointer_pos()) {
                let t = t_of(p.x);
                self.sel = Some((a.min(t), a.max(t)));
            }
        }
        if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                self.sel = None;
                self.seek(t_of(p.x));
            }
        }

        let p = ui.painter_at(rect);
        let track = Rect::from_min_max(pos2(l, rect.min.y + 18.0), pos2(r, rect.min.y + 52.0));
        p.rect_filled(track, 6.0, Color32::from_rgb(244, 205, 208)); // cut-out parts
        for &(s, e) in &self.keeps {
            let seg = Rect::from_min_max(pos2(x_of(s), track.min.y), pos2(x_of(e).max(x_of(s) + 1.0), track.max.y));
            p.rect_filled(seg, 0.0, Color32::from_rgb(120, 160, 235));
        }
        for c in &self.caps {
            let seg = Rect::from_min_max(pos2(x_of(c.start), track.max.y + 6.0), pos2(x_of(c.end).max(x_of(c.start) + 2.0), track.max.y + 14.0));
            p.rect_filled(seg, 2.0, Color32::from_rgb(255, 196, 60));
        }
        if let Some((a, b)) = self.sel {
            let sr = Rect::from_min_max(pos2(x_of(a), track.min.y - 4.0), pos2(x_of(b), track.max.y + 4.0));
            p.rect_filled(sr, 0.0, Color32::from_black_alpha(40));
            p.rect_stroke(sr, 0.0, Stroke::new(1.5, TEXT));
        }
        let hx = x_of(self.playhead);
        p.line_segment([pos2(hx, rect.min.y + 8.0), pos2(hx, track.max.y + 16.0)], Stroke::new(2.0, DANGER));
        p.add(Shape::convex_polygon(
            vec![pos2(hx - 6.0, rect.min.y + 4.0), pos2(hx + 6.0, rect.min.y + 4.0), pos2(hx, rect.min.y + 14.0)],
            DANGER,
            Stroke::NONE,
        ));
        p.text(pos2(l, rect.max.y - 8.0), Align2::LEFT_CENTER, fmt_time(0.0), FontId::proportional(11.0), TEXT_DIM);
        p.text(pos2(r, rect.max.y - 8.0), Align2::RIGHT_CENTER, fmt_time(dur), FontId::proportional(11.0), TEXT_DIM);
    }
}

impl eframe::App for VideoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(ci) = self.frame_rx.try_recv() {
            self.frame_tex = Some(ctx.load_texture("frame", ci, egui::TextureOptions::LINEAR));
        }
        self.tick_play(ctx);

        // export progress / result
        let mut finished = None;
        if let Some(job) = self.export.as_mut() {
            finished = job.poll();
            ctx.request_repaint_after(Duration::from_millis(150));
        }
        if let Some(res) = finished {
            self.export = None;
            match res {
                Ok(p) => {
                    self.status = format!("Exported: {}", p.display());
                    self.last_export = Some(p);
                }
                Err(e) if e == "Cancelled" => self.status = "Export cancelled.".to_string(),
                Err(e) => self.status = format!("Export failed: {e}"),
            }
        }
        // keep the captions safe next to the video (reloaded the next time you open it)
        if !self.caps.is_empty() && self.last_save.elapsed() > Duration::from_secs(5) {
            let _ = write_srt(&self.caps, &self.srt_path);
            self.last_save = Instant::now();
        }

        let dur = self.info.duration;
        let mut do_export = false;

        // ---------------------------------------------------------------- right panel
        egui::SidePanel::right("side")
            .exact_width(410.0)
            .frame(egui::Frame::none().fill(PANEL).inner_margin(egui::Margin::same(14.0)))
            .show(ctx, |ui| {
                ui.label(RichText::new("Captions").size(17.0).strong());
                ui.label(RichText::new("Type captions by hand, or import an .srt file.").size(12.0).color(TEXT_DIM));
                ui.horizontal_wrapped(|ui| {
                    if ui.button("+ Add at playhead").clicked() {
                        let t = self.playhead;
                        self.caps.push(Caption { start: t, end: (t + 2.0).min(dur), text: "New caption".to_string() });
                    }
                    if ui.button("Import .srt").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("Subtitles", &["srt"]).pick_file() {
                            match read_srt(&p) {
                                Ok(c) => self.caps = c,
                                Err(e) => self.status = format!("Could not read the file: {e}"),
                            }
                        }
                    }
                    if ui.button("Export .srt").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("Subtitles", &["srt"]).set_file_name("captions.srt").save_file() {
                            let _ = write_srt(&map_captions(&self.caps, &self.keeps, self.speed()), &p);
                            self.status = "Saved subtitles (timed for your edited video).".to_string();
                        }
                    }
                    if ui.button("Clear all").clicked() {
                        self.caps.clear();
                        let _ = std::fs::remove_file(&self.srt_path);
                    }
                });
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_source("capsize").selected_text(CAP_NAMES[self.cap_size]).show_ui(ui, |ui| {
                        for (i, n) in CAP_NAMES.iter().enumerate() {
                            ui.selectable_value(&mut self.cap_size, i, *n);
                        }
                    });
                    ui.radio_value(&mut self.cap_top, false, "Bottom");
                    ui.radio_value(&mut self.cap_top, true, "Top");
                    ui.checkbox(&mut self.preview_caps, "Show in preview");
                });

                let mut remove: Option<usize> = None;
                let mut jump: Option<f64> = None;
                egui::ScrollArea::vertical().max_height(300.0).auto_shrink([false, false]).show(ui, |ui| {
                    for (i, c) in self.caps.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut c.start).speed(0.05).fixed_decimals(2).clamp_range(0.0..=dur).suffix(" s"));
                            ui.add(egui::DragValue::new(&mut c.end).speed(0.05).fixed_decimals(2).clamp_range(0.0..=dur).suffix(" s"));
                            if ui.small_button("go").clicked() {
                                jump = Some(c.start);
                            }
                            if ui.small_button("x").clicked() {
                                remove = Some(i);
                            }
                        });
                        ui.add(egui::TextEdit::singleline(&mut c.text).desired_width(f32::INFINITY));
                        ui.add_space(4.0);
                    }
                });
                if let Some(i) = remove {
                    self.caps.remove(i);
                }
                if let Some(t) = jump {
                    self.seek(t);
                }

                ui.separator();
                ui.label(RichText::new("Export").size(17.0).strong());
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_source("fmt").selected_text(self.fmt.label()).show_ui(ui, |ui| {
                        for f in [Fmt::Mp4, Fmt::WebM, Fmt::Gif, Fmt::Mp3] {
                            ui.selectable_value(&mut self.fmt, f, f.label());
                        }
                    });
                    egui::ComboBox::from_id_source("res")
                        .selected_text(if self.res == 0 { "Original size".to_string() } else { format!("{}p", self.res) })
                        .show_ui(ui, |ui| {
                            for r in [0u32, 1080, 720, 480] {
                                ui.selectable_value(&mut self.res, r, if r == 0 { "Original size".to_string() } else { format!("{r}p") });
                            }
                        });
                    egui::ComboBox::from_id_source("qual").selected_text(self.quality.label()).show_ui(ui, |ui| {
                        for q in [Quality::High, Quality::Balanced, Quality::Small] {
                            ui.selectable_value(&mut self.quality, q, q.label());
                        }
                    });
                });
                ui.checkbox(&mut self.burn, "Burn captions into the picture");
                ui.checkbox(&mut self.save_srt, "Also save a .srt subtitle file");
                if let Some(job) = &self.export {
                    ui.add(egui::ProgressBar::new(job.progress.load(Ordering::Relaxed) as f32 / 1000.0).show_percentage());
                    if ui.button("Cancel export").clicked() {
                        job.cancel();
                    }
                } else if ui::primary_button(ui, "Export...").clicked() {
                    do_export = true;
                }
                if let Some(p) = self.last_export.clone() {
                    ui.horizontal(|ui| {
                        if ui.button("Play result").clicked() {
                            util::open_file(&p);
                        }
                        if ui.button("Show in folder").clicked() {
                            util::reveal_in_explorer(&p);
                        }
                    });
                }
            });

        // ---------------------------------------------------------------- centre
        egui::CentralPanel::default().frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::same(14.0))).show(ctx, |ui| {
            let avail_h = (ui.available_height() - 232.0).max(180.0);
            let k = (ui.available_width() / self.pw as f32).min(avail_h / self.ph as f32).max(0.1);
            let size = vec2(self.pw as f32 * k, self.ph as f32 * k);
            ui.vertical_centered(|ui| {
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                let p = ui.painter_at(rect);
                p.rect_filled(rect, 6.0, Color32::BLACK);
                if let Some(t) = &self.frame_tex {
                    p.image(t.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                if self.preview_caps {
                    if let Some(c) = self.caps.iter().find(|c| self.playhead >= c.start && self.playhead < c.end) {
                        let fs = rect.height() * [0.05, 0.065, 0.085][self.cap_size];
                        let text = c.text.replace('\n', " ");
                        let w = (text.chars().count() as f32 * fs * 0.55).min(rect.width() * 0.95);
                        let y = if self.cap_top { rect.min.y + fs * 0.9 + 10.0 } else { rect.max.y - fs * 0.9 - 12.0 };
                        let bg = Rect::from_center_size(pos2(rect.center().x, y), vec2(w + 18.0, fs * 1.5));
                        p.rect_filled(bg, 4.0, Color32::from_black_alpha(170));
                        p.text(bg.center(), Align2::CENTER_CENTER, text, FontId::proportional(fs), Color32::WHITE);
                    }
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let playing = self.play.is_some();
                if ui::primary_button(ui, if playing { "Pause" } else { "Play (silent preview)" }).clicked() {
                    if playing {
                        self.stop_play();
                    } else {
                        self.start_play();
                    }
                }
                ui.label(format!("{} / {}", fmt_time(self.playhead), fmt_time(dur)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::ComboBox::from_id_source("speed").selected_text(format!("{}x", SPEEDS[self.speed_idx])).show_ui(ui, |ui| {
                        for (i, s) in SPEEDS.iter().enumerate() {
                            ui.selectable_value(&mut self.speed_idx, i, format!("{s}x"));
                        }
                    });
                    ui.label("Speed");
                    ui.label(RichText::new(format!("Edited length {}", fmt_time(self.total_kept() / self.speed()))).color(TEXT_DIM));
                });
            });
            self.ui_timeline(ui);
            ui.horizontal_wrapped(|ui| {
                if ui.button("Delete selection").clicked() {
                    self.delete_selection();
                }
                if ui.button("Keep only selection").clicked() {
                    self.keep_selection();
                }
                if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Undo edit")).clicked() {
                    if let Some(k) = self.undo.pop() {
                        self.keeps = k;
                    }
                }
                if ui.button("Reset all cuts").clicked() {
                    self.undo.push(self.keeps.clone());
                    self.keeps = vec![(0.0, dur)];
                    self.sel = None;
                }
            });
            ui.label(RichText::new("Blue = kept, pink = cut out, yellow = captions. The preview has no sound; the exported video does.").size(11.0).color(TEXT_DIM));
            ui.label(RichText::new(self.status.as_str()).size(12.0).color(ACCENT));
            let _ = BORDER;
        });

        if do_export {
            self.start_export(ctx);
        }
    }
}
