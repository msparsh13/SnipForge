//! Pin to screen: a borderless, always-on-top window that shows an image.
//! Drag to move, mouse wheel to resize, double-click or Esc to close, right-click for more.
use eframe::egui::{self, pos2, vec2, Color32, Rect, Sense, Stroke, Vec2};
use image::RgbaImage;

use crate::ui::{self, ACCENT};
use crate::{capture, util, winapi};

struct PinApp {
    img: RgbaImage,
    tex: Option<egui::TextureHandle>,
    title: String,
    base: Vec2,
    scale: f32,
    inited: bool,
    frames: u32,
}

pub fn run(path: String) -> Result<(), eframe::Error> {
    let img = match image::open(&path) {
        Ok(i) => i.to_rgba8(),
        Err(_) => return Ok(()),
    };
    let (w, h) = img.dimensions();
    // the title must NOT start with "SnipForge": other captures minimise those windows, pins must stay put
    let title = format!("Pin {} - SnipForge", std::process::id());
    let vb = egui::ViewportBuilder::default()
        .with_title(title.clone())
        .with_decorations(false)
        .with_inner_size([w as f32, h as f32]);
    let options = eframe::NativeOptions {
        viewport: vb,
        ..Default::default()
    };
    eframe::run_native(
        "SnipForge",
        options,
        Box::new(move |cc| {
            ui::apply_theme(&cc.egui_ctx);
            Box::new(PinApp {
                img,
                tex: None,
                title,
                base: vec2(w as f32, h as f32),
                scale: 1.0,
                inited: false,
                frames: 0,
            })
        }),
    )
}

impl eframe::App for PinApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.inited {
            self.inited = true;
            let ppp = ctx.pixels_per_point();
            let (sw, sh) = capture::primary_size()
                .map(|(w, h)| (w as f32 / ppp, h as f32 / ppp))
                .unwrap_or((1920.0, 1080.0));
            let mut b = vec2(
                self.img.width() as f32 / ppp,
                self.img.height() as f32 / ppp,
            );
            let k = (sw * 0.7 / b.x).min(sh * 0.7 / b.y).min(1.0);
            b *= k;
            self.base = b;
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(b));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos2(80.0, 80.0)));
        }
        if self.frames < 12 {
            self.frames += 1;
            winapi::make_topmost(&self.title);
            ctx.request_repaint();
        }

        let scroll = ctx.input(|i| i.raw_scroll_delta.y);
        if scroll.abs() > 0.1 {
            self.scale = (self.scale * if scroll > 0.0 { 1.1 } else { 1.0 / 1.1 }).clamp(0.15, 4.0);
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(self.base * self.scale));
        }

        if self.tex.is_none() {
            self.tex = Some(ctx.load_texture(
                "pin",
                ui::color_image(&self.img),
                egui::TextureOptions::LINEAR,
            ));
        }
        let tex_id = self.tex.as_ref().map(|t| t.id());

        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let resp = ui.allocate_rect(rect, Sense::click_and_drag());
                if let Some(id) = tex_id {
                    ui.painter().image(
                        id,
                        rect,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                ui.painter()
                    .rect_stroke(rect, 0.0, Stroke::new(2.0, ACCENT));
                if resp.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if resp.double_clicked() {
                    close = true;
                }
                resp.context_menu(|ui| {
                    if ui.button("Copy image").clicked() {
                        let _ = util::copy_image(&self.img);
                        ui.close_menu();
                    }
                    if ui.button("Save as...").clicked() {
                        if let Some(p) = rfd::FileDialog::new()
                            .add_filter("PNG", &["png"])
                            .set_file_name("pinned.png")
                            .save_file()
                        {
                            let _ = self.img.save(p);
                        }
                        ui.close_menu();
                    }
                    if ui.button("Reset size").clicked() {
                        self.scale = 1.0;
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(self.base));
                        ui.close_menu();
                    }
                    if ui.button("Close pin").clicked() {
                        close = true;
                        ui.close_menu();
                    }
                });
            });
        if close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
