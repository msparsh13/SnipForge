//! Shared look & feel: theme colours and painted icons / cards.
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};

pub const ACCENT: Color32 = Color32::from_rgb(76, 194, 255);
pub const BG: Color32 = Color32::from_rgb(24, 26, 30);
pub const PANEL: Color32 = Color32::from_rgb(32, 35, 40);
pub const CARD: Color32 = Color32::from_rgb(42, 46, 53);
pub const CARD_HOVER: Color32 = Color32::from_rgb(54, 59, 68);
pub const DANGER: Color32 = Color32::from_rgb(255, 90, 95);

pub fn apply_theme(ctx: &egui::Context) {
    ctx.style_mut(|s| {
        let v = &mut s.visuals;
        v.dark_mode = true;
        v.panel_fill = BG;
        v.window_fill = PANEL;
        v.extreme_bg_color = Color32::from_rgb(18, 19, 22);
        v.faint_bg_color = PANEL;
        v.window_rounding = egui::Rounding::same(10.0);
        v.selection.bg_fill = ACCENT.gamma_multiply(0.45);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.hyperlink_color = ACCENT;
        let r = egui::Rounding::same(7.0);
        v.widgets.noninteractive.rounding = r;
        v.widgets.inactive.rounding = r;
        v.widgets.hovered.rounding = r;
        v.widgets.active.rounding = r;
        v.widgets.open.rounding = r;
        v.widgets.inactive.bg_fill = CARD;
        v.widgets.inactive.weak_bg_fill = CARD;
        v.widgets.hovered.bg_fill = CARD_HOVER;
        v.widgets.hovered.weak_bg_fill = CARD_HOVER;
        v.widgets.active.bg_fill = ACCENT.gamma_multiply(0.5);
        v.widgets.active.weak_bg_fill = ACCENT.gamma_multiply(0.5);
        s.spacing.item_spacing = vec2(8.0, 8.0);
        s.spacing.button_padding = vec2(12.0, 7.0);
    });
}

#[derive(Clone, Copy)]
pub enum Icon {
    Region,
    Screen,
    Text,
    Record,
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
    if hovered {
        p.rect_stroke(rect, 12.0, Stroke::new(1.5, ACCENT));
    }
    let ic = Rect::from_center_size(pos2(rect.min.x + 36.0, rect.center().y), vec2(34.0, 34.0));
    draw_big_icon(&p, ic, icon);
    p.text(
        pos2(rect.min.x + 72.0, rect.center().y - 11.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(16.0),
        Color32::WHITE,
    );
    p.text(
        pos2(rect.min.x + 72.0, rect.center().y + 12.0),
        Align2::LEFT_CENTER,
        sub,
        FontId::proportional(12.0),
        Color32::from_gray(150),
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Primary (accent) button.
pub fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(egui::RichText::new(text).color(Color32::BLACK).strong()).fill(ACCENT))
}

pub fn danger_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(egui::RichText::new(text).color(Color32::WHITE).strong()).fill(Color32::from_rgb(210, 55, 60)))
}
