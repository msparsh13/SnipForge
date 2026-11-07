//! Shared look & feel: LIGHT theme palette, painted icons and cards.
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};

// ---- palette (light) ----
pub const BG: Color32 = Color32::from_rgb(245, 246, 250); // window background
pub const PANEL: Color32 = Color32::from_rgb(255, 255, 255); // toolbars / side panels
pub const CARD: Color32 = Color32::from_rgb(255, 255, 255);
pub const CARD_HOVER: Color32 = Color32::from_rgb(238, 243, 255);
pub const BORDER: Color32 = Color32::from_rgb(220, 225, 234);
pub const CANVAS: Color32 = Color32::from_rgb(228, 231, 238); // behind the screenshot
pub const ACCENT: Color32 = Color32::from_rgb(37, 99, 235); // blue
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(225, 235, 255);
pub const TEXT: Color32 = Color32::from_rgb(28, 32, 41);
pub const TEXT_DIM: Color32 = Color32::from_rgb(104, 112, 128);
pub const DANGER: Color32 = Color32::from_rgb(220, 53, 69);
pub const WARN: Color32 = Color32::from_rgb(200, 110, 0);
pub const SUCCESS: Color32 = Color32::from_rgb(18, 128, 74);
/// Used only on top of the dimmed screenshot in the capture overlay.
pub const OVERLAY: Color32 = Color32::from_rgb(76, 194, 255);

pub fn apply_theme(ctx: &egui::Context) {
    let mut v = egui::Visuals::light();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::WHITE; // text boxes
    v.faint_bg_color = Color32::from_rgb(241, 243, 248);
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_rounding = egui::Rounding::same(12.0);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(37, 99, 235, 70);
    v.selection.stroke = Stroke::new(1.0, ACCENT);

    let r = egui::Rounding::same(8.0);
    let soft = Color32::from_rgb(226, 230, 238); // slider rails, check boxes

    v.widgets.noninteractive.rounding = r;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);

    v.widgets.inactive.rounding = r;
    v.widgets.inactive.bg_fill = soft;
    v.widgets.inactive.weak_bg_fill = Color32::WHITE; // buttons
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);

    v.widgets.hovered.rounding = r;
    v.widgets.hovered.bg_fill = Color32::from_rgb(214, 224, 248);
    v.widgets.hovered.weak_bg_fill = ACCENT_SOFT;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, TEXT);

    v.widgets.active.rounding = r;
    v.widgets.active.bg_fill = Color32::from_rgb(190, 210, 250);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(205, 222, 255);
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.active.fg_stroke = Stroke::new(2.0, TEXT);

    v.widgets.open.rounding = r;
    v.widgets.open.bg_fill = soft;
    v.widgets.open.weak_bg_fill = Color32::WHITE;
    v.widgets.open.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.open.fg_stroke = Stroke::new(1.0, TEXT);

    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = vec2(8.0, 8.0);
        s.spacing.button_padding = vec2(12.0, 7.0);
    });
}

/// Texture-ready copy of an image. Very large multi-monitor images are shrunk for DISPLAY only
/// (GPUs limit texture size, often to 8192 px). Saved files and selection coordinates keep full resolution.
pub fn color_image(img: &image::RgbaImage) -> egui::ColorImage {
    const MAX: u32 = 8192;
    let (w, h) = img.dimensions();
    if w <= MAX && h <= MAX {
        return egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
    }
    let s = (MAX as f32 / w as f32).min(MAX as f32 / h as f32);
    let (nw, nh) = (((w as f32 * s) as u32).max(1), ((h as f32 * s) as u32).max(1));
    let small = image::imageops::resize(img, nw, nh, image::imageops::FilterType::Triangle);
    egui::ColorImage::from_rgba_unmultiplied([nw as usize, nh as usize], small.as_raw())
}

#[derive(Clone, Copy)]
pub enum Icon {
    Region,
    Screen,
    Text,
    Record,
    RecordScreen,
    Scroll,
    Pin,
    History,
    Film,
}

fn draw_big_icon(p: &egui::Painter, r: Rect, icon: Icon) {
    let s = Stroke::new(2.5, ACCENT);
    match icon {
        Icon::Region => {
            let l = 10.0;
            let (a, b) = (r.min, r.max);
            for (c, dx, dy) in [
                (pos2(a.x, a.y), 1.0, 1.0),
                (pos2(b.x, a.y), -1.0, 1.0),
                (pos2(a.x, b.y), 1.0, -1.0),
                (pos2(b.x, b.y), -1.0, -1.0),
            ] {
                p.line_segment([c, c + vec2(dx * l, 0.0)], s);
                p.line_segment([c, c + vec2(0.0, dy * l)], s);
            }
        }
        Icon::Screen => {
            p.rect_stroke(Rect::from_min_max(r.min, pos2(r.max.x, r.max.y - 8.0)), 3.0, s);
            p.line_segment([pos2(r.center().x - 9.0, r.max.y - 1.0), pos2(r.center().x + 9.0, r.max.y - 1.0)], s);
            p.line_segment([pos2(r.center().x, r.max.y - 8.0), pos2(r.center().x, r.max.y - 1.0)], s);
        }
        Icon::Text => {
            p.text(r.center(), Align2::CENTER_CENTER, "Aa", FontId::proportional(24.0), ACCENT);
        }
        Icon::Scroll => {
            p.rect_stroke(Rect::from_min_max(pos2(r.min.x + 6.0, r.min.y), pos2(r.max.x - 6.0, r.max.y)), 4.0, s);
            for dy in [8.0, 14.0, 20.0] {
                p.line_segment([pos2(r.min.x + 12.0, r.min.y + dy), pos2(r.max.x - 12.0, r.min.y + dy)], Stroke::new(1.5, ACCENT));
            }
            p.arrow(pos2(r.center().x, r.min.y + 24.0), vec2(0.0, 10.0), s);
        }
        Icon::Pin => {
            p.circle_filled(pos2(r.center().x, r.min.y + 9.0), 8.0, ACCENT);
            p.line_segment([pos2(r.center().x, r.min.y + 15.0), pos2(r.center().x, r.max.y)], s);
        }
        Icon::History => {
            p.circle_stroke(r.center(), 16.0, s);
            p.line_segment([r.center(), r.center() + vec2(0.0, -10.0)], s);
            p.line_segment([r.center(), r.center() + vec2(8.0, 4.0)], s);
        }
        Icon::Film => {
            p.rect_stroke(Rect::from_min_max(pos2(r.min.x, r.min.y + 4.0), pos2(r.max.x, r.max.y - 4.0)), 4.0, s);
            p.add(egui::Shape::convex_polygon(
                vec![pos2(r.center().x - 5.0, r.center().y - 7.0), pos2(r.center().x - 5.0, r.center().y + 7.0), pos2(r.center().x + 8.0, r.center().y)],
                ACCENT,
                Stroke::NONE,
            ));
        }
        Icon::RecordScreen => {
            let sd = Stroke::new(2.5, DANGER);
            p.rect_stroke(Rect::from_min_max(r.min, pos2(r.max.x, r.max.y - 8.0)), 3.0, sd);
            p.line_segment([pos2(r.center().x - 9.0, r.max.y - 1.0), pos2(r.center().x + 9.0, r.max.y - 1.0)], sd);
            p.line_segment([pos2(r.center().x, r.max.y - 8.0), pos2(r.center().x, r.max.y - 1.0)], sd);
            p.circle_filled(pos2(r.center().x, r.center().y - 4.0), 6.0, DANGER);
        }
        Icon::Record => {
            p.circle_stroke(r.center(), 16.0, Stroke::new(2.5, DANGER));
            p.circle_filled(r.center(), 8.0, DANGER);
        }
    }
}

/// A big clickable action card with a painted icon.
pub fn card(ui: &mut egui::Ui, icon: Icon, title: &str, sub: &str, size: egui::Vec2) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter_at(rect);
    let hovered = resp.hovered();
    p.rect_filled(rect, 12.0, if hovered { CARD_HOVER } else { CARD });
    p.rect_stroke(rect, 12.0, Stroke::new(if hovered { 1.5 } else { 1.0 }, if hovered { ACCENT } else { BORDER }));
    let ic = Rect::from_center_size(pos2(rect.min.x + 36.0, rect.center().y), vec2(34.0, 34.0));
    draw_big_icon(&p, ic, icon);
    p.text(
        pos2(rect.min.x + 72.0, rect.center().y - 11.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(16.0),
        TEXT,
    );
    p.text(
        pos2(rect.min.x + 72.0, rect.center().y + 12.0),
        Align2::LEFT_CENTER,
        sub,
        FontId::proportional(12.0),
        TEXT_DIM,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Primary (accent) button.
pub fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(egui::RichText::new(text).color(Color32::WHITE).strong()).fill(ACCENT))
}

pub fn danger_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(egui::RichText::new(text).color(Color32::WHITE).strong()).fill(DANGER))
}
