//! Annotation editor: arrow, rect, ellipse, pen, highlighter, text, numbered steps, blur, crop.
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke};
use image::RgbaImage;

use crate::capture;
use crate::render::{self, Ann};
use crate::ui::{ACCENT, ACCENT_SOFT, BORDER, CANVAS, PANEL, TEXT_DIM};
use crate::util;

#[derive(Clone, Copy, PartialEq)]
pub enum Tool {
    Arrow,
    Rect,
    Ellipse,
    Pen,
    Highlight,
    Text,
    Step,
    Blur,
    Crop,
    Read,
}

const TOOLS: [(Tool, &str); 10] = [
    (Tool::Arrow, "Arrow"),
    (Tool::Rect, "Rectangle"),
    (Tool::Ellipse, "Ellipse"),
    (Tool::Pen, "Pen"),
    (Tool::Highlight, "Marker"),
    (Tool::Text, "Text"),
    (Tool::Step, "Step"),
    (Tool::Blur, "Blur"),
    (Tool::Crop, "Crop"),
    (Tool::Read, "Read text"),
];

const SWATCHES: [[u8; 3]; 8] = [
    [255, 59, 48],
    [255, 149, 0],
    [255, 204, 0],
    [52, 199, 89],
    [10, 132, 255],
    [175, 82, 222],
    [255, 255, 255],
    [0, 0, 0],
];

pub enum EditorEvent {
    None,
    NewRegion,
    NewFull,
    ExtractText(RgbaImage),
}

type CropRect = (u32, u32, u32, u32);

enum Edit {
    Add(Ann),
    Crop { before_base: RgbaImage, before_anns: Vec<Ann>, rect: CropRect },
}

enum Redo {
    Add(Ann),
    Crop(CropRect),
}

struct Drag {
    start: Pos2,
    cur: Pos2,
    pts: Vec<Pos2>,
}

pub struct Editor {
    base: RgbaImage,
    path: PathBuf,
    anns: Vec<Ann>,
    undo_stack: Vec<Edit>,
    redo_stack: Vec<Redo>,
    tex: Option<egui::TextureHandle>,
    dirty: bool,
    tool: Tool,
    color: [u8; 3],
    width: f32,
    drag: Option<Drag>,
    pending_text: Option<(Pos2, String)>,
    pending_crop: Option<Rect>, // image coordinates
    font: Option<Arc<fontdue::Font>>,
}

fn ellipse_points(c: Pos2, rx: f32, ry: f32) -> Vec<Pos2> {
    (0..64)
        .map(|i| {
            let t = i as f32 / 64.0 * std::f32::consts::TAU;
            pos2(c.x + rx * t.cos(), c.y + ry * t.sin())
        })
        .collect()
}

fn draw_tool_icon(p: &egui::Painter, r: Rect, tool: Tool, c: Color32) {
    let s = Stroke::new(2.0, c);
    match tool {
        Tool::Arrow => p.arrow(r.left_bottom() + vec2(2.0, -2.0), vec2(r.width() - 4.0, -(r.height() - 4.0)), s),
        Tool::Rect => {
            p.rect_stroke(r.shrink(2.0), 2.0, s);
        }
        Tool::Ellipse => {
            let pts = ellipse_points(r.center(), r.width() / 2.0 - 1.0, r.height() / 2.0 - 1.0);
            p.add(Shape::closed_line(pts, s));
        }
        Tool::Pen => {
            let pts: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let t = i as f32 / 12.0;
                    pos2(r.min.x + t * r.width(), r.center().y + (t * 9.0).sin() * 6.0)
                })
                .collect();
            p.add(Shape::line(pts, s));
        }
        Tool::Highlight => {
            p.line_segment([r.left_center(), r.right_center()], Stroke::new(10.0, c.gamma_multiply(0.45)));
            p.line_segment([r.left_center(), r.right_center()], Stroke::new(1.5, c));
        }
        Tool::Text => {
            p.text(r.center(), Align2::CENTER_CENTER, "T", FontId::proportional(21.0), c);
        }
        Tool::Step => {
            p.circle_filled(r.center(), 10.0, c);
            p.text(r.center(), Align2::CENTER_CENTER, "1", FontId::proportional(13.0), Color32::WHITE);
        }
        Tool::Blur => {
            let cell = r.height() / 3.0;
            for ix in 0..3 {
                for iy in 0..3 {
                    if (ix + iy) % 2 == 0 {
                        let min = pos2(r.center().x - 1.5 * cell + ix as f32 * cell, r.min.y + iy as f32 * cell);
                        p.rect_filled(Rect::from_min_size(min, vec2(cell, cell)), 0.0, c.gamma_multiply(0.8));
                    }
                }
            }
        }
        Tool::Read => {
            p.text(r.center(), Align2::CENTER_CENTER, "Aa", FontId::proportional(15.0), c);
            p.rect_stroke(r.shrink(1.0), 3.0, Stroke::new(1.5, c.gamma_multiply(0.7)));
        }
        Tool::Crop => {
            let l = 7.0;
            let q = r.shrink(2.0);
            p.line_segment([q.left_top() + vec2(0.0, l), q.left_top()], s);
            p.line_segment([q.left_top(), q.left_top() + vec2(l, 0.0)], s);
            p.line_segment([q.right_bottom() - vec2(0.0, l), q.right_bottom()], s);
            p.line_segment([q.right_bottom(), q.right_bottom() - vec2(l, 0.0)], s);
            p.rect_stroke(q.shrink(5.0), 0.0, Stroke::new(1.0, c.gamma_multiply(0.6)));
        }
    }
}

fn tool_button(ui: &mut egui::Ui, selected: bool, tool: Tool, label: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(62.0, 52.0), Sense::click());
    let p = ui.painter_at(rect);
    let bg = if selected {
        ACCENT_SOFT
    } else if resp.hovered() {
        Color32::from_black_alpha(10)
    } else {
        Color32::TRANSPARENT
    };
    p.rect_filled(rect, 8.0, bg);
    if selected {
        p.rect_stroke(rect, 8.0, Stroke::new(1.5, ACCENT));
    }
    let icon_rect = Rect::from_center_size(rect.center_top() + vec2(0.0, 19.0), vec2(26.0, 22.0));
    draw_tool_icon(&p, icon_rect, tool, if selected { ACCENT } else { Color32::from_rgb(58, 64, 78) });
    p.text(
        rect.center_bottom() - vec2(0.0, 9.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.0),
        TEXT_DIM,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

impl Editor {
    pub fn new(base: RgbaImage, path: PathBuf, font: Option<Arc<fontdue::Font>>) -> Self {
        Editor {
            base,
            path,
            anns: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            tex: None,
            dirty: true,
            tool: Tool::Arrow,
            color: SWATCHES[0],
            width: 4.0,
            drag: None,
            pending_text: None,
            pending_crop: None,
            font,
        }
    }

    pub fn image_size(&self) -> (u32, u32) {
        self.base.dimensions()
    }

    fn color32(&self) -> Color32 {
        Color32::from_rgb(self.color[0], self.color[1], self.color[2])
    }

    fn rendered(&self) -> RgbaImage {
        render::render(&self.base, &self.anns, self.font.as_deref())
    }

    fn refresh_tex(&mut self, ctx: &egui::Context) {
        let img = self.rendered();
        let ci = crate::ui::color_image(&img);
        self.tex = Some(ctx.load_texture("shot", ci, egui::TextureOptions::LINEAR));
        self.dirty = false;
    }

    // ---------------------------------------------------------------- edits
    fn commit_ann(&mut self, a: Ann) {
        self.anns.push(a.clone());
        self.undo_stack.push(Edit::Add(a));
        self.redo_stack.clear();
        self.dirty = true;
    }

    fn crop_to(&mut self, r: CropRect) {
        let (x, y, w, h) = r;
        let before_base = self.base.clone();
        let before_anns = self.anns.clone();
        self.base = capture::crop(&self.base, x, y, w, h);
        for a in self.anns.iter_mut() {
            a.translate(-(x as f32), -(y as f32));
        }
        self.undo_stack.push(Edit::Crop { before_base, before_anns, rect: r });
        self.dirty = true;
    }

    fn rect_to_crop(&self, r: Rect) -> Option<CropRect> {
        let (iw, ih) = (self.base.width() as f32, self.base.height() as f32);
        let x0 = r.min.x.clamp(0.0, iw);
        let y0 = r.min.y.clamp(0.0, ih);
        let x1 = r.max.x.clamp(0.0, iw);
        let y1 = r.max.y.clamp(0.0, ih);
        let (x, y, w, h) = (x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32);
        if w < 4 || h < 4 {
            None
        } else {
            Some((x, y, w, h))
        }
    }

    fn apply_pending_crop(&mut self) {
        if let Some(r) = self.pending_crop.take() {
            if let Some(c) = self.rect_to_crop(r) {
                self.crop_to(c);
                self.redo_stack.clear();
            }
        }
    }

    fn undo(&mut self) {
        self.pending_crop = None;
        match self.undo_stack.pop() {
            Some(Edit::Add(a)) => {
                self.anns.pop();
                self.redo_stack.push(Redo::Add(a));
                self.dirty = true;
            }
            Some(Edit::Crop { before_base, before_anns, rect }) => {
                self.base = before_base;
                self.anns = before_anns;
                self.redo_stack.push(Redo::Crop(rect));
                self.dirty = true;
            }
            None => {}
        }
    }

    fn redo(&mut self) {
        self.pending_crop = None;
        match self.redo_stack.pop() {
            Some(Redo::Add(a)) => {
                self.anns.push(a.clone());
                self.undo_stack.push(Edit::Add(a));
                self.dirty = true;
            }
            Some(Redo::Crop(r)) => self.crop_to(r),
            None => {}
        }
    }

    fn make_ann(&self, d: &Drag) -> Option<Ann> {
        let col = self.color32();
        let moved = (d.cur - d.start).length();
        match self.tool {
            Tool::Arrow if moved > 3.0 => Some(Ann::Arrow { a: d.start, b: d.cur, color: col, w: self.width }),
            Tool::Rect if moved > 3.0 => Some(Ann::Rect { a: d.start, b: d.cur, color: col, w: self.width }),
            Tool::Ellipse if moved > 3.0 => Some(Ann::Ellipse { a: d.start, b: d.cur, color: col, w: self.width }),
            Tool::Blur if moved > 3.0 => Some(Ann::Blur { a: d.start, b: d.cur }),
            Tool::Pen if d.pts.len() > 1 => Some(Ann::Pen { pts: d.pts.clone(), color: col, w: self.width }),
            Tool::Highlight if d.pts.len() > 1 => Some(Ann::Highlight { pts: d.pts.clone(), color: col, w: self.width }),
            _ => None,
        }
    }

    // ---------------------------------------------------------------- file ops
    fn save(&self) -> Result<(), String> {
        self.rendered().save(&self.path).map_err(|e| e.to_string())
    }

    fn save_as(&mut self) -> Result<(), String> {
        let name = self.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .set_file_name(&name)
            .save_file()
        {
            self.rendered().save(&p).map_err(|e| e.to_string())?;
            self.path = p;
        }
        Ok(())
    }

    fn do_copy(&self, status: &mut String) {
        *status = match util::copy_image(&self.rendered()) {
            Ok(()) => "Copied to clipboard".to_string(),
            Err(e) => format!("Copy failed: {e}"),
        };
    }

    fn do_save(&self, status: &mut String) {
        *status = match self.save() {
            Ok(()) => format!("Saved  {}", self.path.display()),
            Err(e) => format!("Save failed: {e}"),
        };
    }

    // ---------------------------------------------------------------- UI
    pub fn ui(&mut self, ctx: &egui::Context, status: &mut String) -> EditorEvent {
        if self.dirty || self.tex.is_none() {
            self.refresh_tex(ctx);
        }
        let mut ev = EditorEvent::None;
        let typing = self.pending_text.is_some();

        // keyboard shortcuts
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Z)) {
            self.undo();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Y)) {
            self.redo();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::S)) {
            self.do_save(status);
        }
        if !typing && ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy))) {
            self.do_copy(status);
        }
        if self.pending_crop.is_some() && !typing {
            if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.apply_pending_crop();
            } else if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.pending_crop = None;
            }
        }

        // ---- top bar
        egui::TopBottomPanel::top("actions")
            .frame(egui::Frame::none().fill(PANEL).inner_margin(egui::Margin::symmetric(12.0, 8.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.add_enabled(!self.undo_stack.is_empty(), egui::Button::new("Undo")).clicked() {
                        self.undo();
                    }
                    if ui.add_enabled(!self.redo_stack.is_empty(), egui::Button::new("Redo")).clicked() {
                        self.redo();
                    }
                    ui.separator();
                    if crate::ui::primary_button(ui, "Copy").clicked() {
                        self.do_copy(status);
                    }
                    if ui.button("Save").clicked() {
                        self.do_save(status);
                    }
                    if ui.button("Pin to screen").clicked() {
                        let path = util::temp_png("snipforge_pin");
                        if self.rendered().save(&path).is_ok() {
                            let p = path.to_string_lossy().to_string();
                            util::spawn_worker("pin", Some(&p));
                        }
                    }
                    if ui.button("Extract text").clicked() {
                        ev = EditorEvent::ExtractText(self.base.clone());
                    }
                    if ui.button("Save as...").clicked() {
                        *status = match self.save_as() {
                            Ok(()) => format!("Saved  {}", self.path.display()),
                            Err(e) => format!("Save failed: {e}"),
                        };
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("New full screen").clicked() {
                            ev = EditorEvent::NewFull;
                        }
                        if ui.button("New region").clicked() {
                            ev = EditorEvent::NewRegion;
                        }
                    });
                });
            });

        // ---- crop confirmation bar
        if let Some(r) = self.pending_crop {
            let size = self.rect_to_crop(r).map(|c| format!("{} x {} px", c.2, c.3)).unwrap_or_default();
            egui::TopBottomPanel::top("crop_bar")
                .frame(egui::Frame::none().fill(ACCENT_SOFT).inner_margin(egui::Margin::symmetric(12.0, 6.0)))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("Crop to {size}"));
                        if crate::ui::primary_button(ui, "Apply crop  (Enter)").clicked() {
                            self.apply_pending_crop();
                        }
                        if ui.button("Cancel  (Esc)").clicked() {
                            self.pending_crop = None;
                        }
                    });
                });
        }

        // ---- left tool palette
        egui::SidePanel::left("tools")
            .exact_width(84.0)
            .resizable(false)
            .frame(egui::Frame::none().fill(PANEL).inner_margin(egui::Margin::same(10.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (t, label) in TOOLS {
                        if tool_button(ui, self.tool == t, t, label).clicked() {
                            self.tool = t;
                            if t != Tool::Crop {
                                self.pending_crop = None;
                            }
                        }
                    }
                    ui.add_space(4.0);
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                        for c in SWATCHES {
                            let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                            ui.painter().rect_filled(r, 5.0, Color32::from_rgb(c[0], c[1], c[2]));
                            ui.painter().rect_stroke(r, 5.0, Stroke::new(1.0, BORDER));
                            if self.color == c {
                                ui.painter().rect_stroke(r.expand(2.0), 6.0, Stroke::new(2.0, ACCENT));
                            }
                            if resp.clicked() {
                                self.color = c;
                            }
                        }
                    });
                    ui.color_edit_button_srgb(&mut self.color);
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Size").size(11.0).color(TEXT_DIM));
                    ui.spacing_mut().slider_width = 56.0;
                    ui.add(egui::Slider::new(&mut self.width, 1.0..=24.0).show_value(false));
                });
            });

        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::none().fill(PANEL).inner_margin(egui::Margin::symmetric(12.0, 5.0)))
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(status.as_str()).size(12.0).color(TEXT_DIM));
            });

        // ---- canvas
        let col = self.color32();
        let tool = self.tool;
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(CANVAS).inner_margin(egui::Margin::same(12.0)))
            .show(ctx, |ui| {
                let avail = ui.available_rect_before_wrap();
                let (iw, ih) = (self.base.width() as f32, self.base.height() as f32);
                let ppp = ctx.pixels_per_point();
                let scale = (avail.width() / iw).min(avail.height() / ih).min(1.0 / ppp);
                let rect = Rect::from_center_size(avail.center(), vec2(iw * scale, ih * scale));
                let resp = ui.allocate_rect(rect, Sense::click_and_drag());
                let painter = ui.painter_at(avail);

                painter.rect_filled(rect.expand(1.0), 0.0, Color32::from_rgb(190, 196, 208));
                if let Some(tex) = &self.tex {
                    painter.image(tex.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                if resp.hovered() && self.pending_text.is_none() {
                    ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                }

                let to_img = |p: Pos2| pos2((p.x - rect.min.x) / scale, (p.y - rect.min.y) / scale);
                let to_scr = |p: Pos2| pos2(rect.min.x + p.x * scale, rect.min.y + p.y * scale);

                match tool {
                    Tool::Step | Tool::Text => {
                        if resp.clicked() {
                            if let Some(p) = resp.interact_pointer_pos() {
                                let ip = to_img(p);
                                if tool == Tool::Step {
                                    let n = self.anns.iter().filter(|a| matches!(a, Ann::Step { .. })).count() as u32 + 1;
                                    let w = self.width;
                                    self.commit_ann(Ann::Step { p: ip, n, color: col, w });
                                } else {
                                    self.pending_text = Some((ip, String::new()));
                                }
                            }
                        }
                    }
                    _ => {
                        if resp.drag_started() {
                            if let Some(p) = resp.interact_pointer_pos() {
                                let ip = to_img(p);
                                self.drag = Some(Drag { start: ip, cur: ip, pts: vec![ip] });
                            }
                        }
                        if resp.dragged() {
                            if let Some(p) = resp.interact_pointer_pos() {
                                let ip = to_img(p);
                                if let Some(d) = self.drag.as_mut() {
                                    d.cur = ip;
                                    if tool == Tool::Pen || tool == Tool::Highlight {
                                        d.pts.push(ip);
                                    }
                                }
                            }
                        }
                        if resp.drag_released() {
                            if let Some(d) = self.drag.take() {
                                if tool == Tool::Crop {
                                    if (d.cur - d.start).length() > 8.0 {
                                        self.pending_crop = Some(Rect::from_two_pos(d.start, d.cur));
                                    }
                                } else if tool == Tool::Read {
                                    if (d.cur - d.start).length() > 8.0 {
                                        if let Some(c) = self.rect_to_crop(Rect::from_two_pos(d.start, d.cur)) {
                                            ev = EditorEvent::ExtractText(capture::crop(&self.base, c.0, c.1, c.2, c.3));
                                        }
                                    }
                                } else if let Some(a) = self.make_ann(&d) {
                                    self.commit_ann(a);
                                }
                            }
                        }
                    }
                }

                // live preview while dragging (final look comes from the renderer)
                if let Some(d) = &self.drag {
                    let stroke = Stroke::new((self.width * scale).max(1.0), col);
                    let (a, b) = (to_scr(d.start), to_scr(d.cur));
                    match tool {
                        Tool::Arrow => painter.arrow(a, b - a, stroke),
                        Tool::Rect => {
                            painter.rect_stroke(Rect::from_two_pos(a, b), 0.0, stroke);
                        }
                        Tool::Ellipse => {
                            let r = Rect::from_two_pos(a, b);
                            painter.add(Shape::closed_line(ellipse_points(r.center(), r.width() / 2.0, r.height() / 2.0), stroke));
                        }
                        Tool::Blur => {
                            let r = Rect::from_two_pos(a, b);
                            painter.rect_filled(r, 0.0, Color32::from_black_alpha(90));
                            painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE));
                        }
                        Tool::Pen | Tool::Highlight => {
                            let pts: Vec<Pos2> = d.pts.iter().map(|p| to_scr(*p)).collect();
                            let (c, w) = if tool == Tool::Highlight {
                                (
                                    Color32::from_rgba_unmultiplied(self.color[0], self.color[1], self.color[2], 100),
                                    self.width * 4.0 * scale + 6.0,
                                )
                            } else {
                                (col, stroke.width)
                            };
                            painter.add(Shape::line(pts, Stroke::new(w, c)));
                        }
                        Tool::Crop | Tool::Read => {
                            let r = Rect::from_two_pos(a, b);
                            draw_crop_overlay(&painter, rect, r);
                        }
                        _ => {}
                    }
                }
                if let Some(cr) = self.pending_crop {
                    let r = Rect::from_two_pos(to_scr(cr.min), to_scr(cr.max)).intersect(rect);
                    draw_crop_overlay(&painter, rect, r);
                }
            });

        // ---- text entry popup
        let mut commit: Option<(Pos2, String)> = None;
        let mut cancel = false;
        if let Some((p, s)) = &mut self.pending_text {
            egui::Window::new("Add text").collapsible(false).resizable(false).show(ctx, |ui| {
                let r = ui.add(egui::TextEdit::multiline(s).desired_rows(3).desired_width(260.0));
                if !r.has_focus() && s.is_empty() {
                    r.request_focus();
                }
                ui.horizontal(|ui| {
                    if crate::ui::primary_button(ui, "Add").clicked() {
                        commit = Some((*p, s.clone()));
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        }
        if let Some((p, s)) = commit {
            if !s.trim().is_empty() {
                let size = 14.0 + self.width * 3.0;
                self.commit_ann(Ann::Text { p, text: s, color: col, size });
            }
            self.pending_text = None;
        } else if cancel {
            self.pending_text = None;
        }
        ev
    }
}

fn draw_crop_overlay(p: &egui::Painter, image_rect: Rect, sel: Rect) {
    let dim = Color32::from_black_alpha(140);
    p.rect_filled(Rect::from_min_max(image_rect.min, pos2(image_rect.max.x, sel.min.y)), 0.0, dim);
    p.rect_filled(Rect::from_min_max(pos2(image_rect.min.x, sel.max.y), image_rect.max), 0.0, dim);
    p.rect_filled(Rect::from_min_max(pos2(image_rect.min.x, sel.min.y), pos2(sel.min.x, sel.max.y)), 0.0, dim);
    p.rect_filled(Rect::from_min_max(pos2(sel.max.x, sel.min.y), pos2(image_rect.max.x, sel.max.y)), 0.0, dim);
    p.rect_stroke(sel, 0.0, Stroke::new(2.0, ACCENT));
}
