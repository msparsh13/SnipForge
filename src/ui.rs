//! Shared look & feel: LIGHT theme palette, painted icons and cards.
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};

// ---- palette (light) ----
pub const BG: Color32 = Color32::from_rgb(245, 246, 250); // window background
pub const PANEL: Color32 = Color32::from_rgb(255, 255, 255); // toolbars / side panels
pub const CARD: Color32 = Color32::from_rgb(255, 255, 255);
pub const CARD_HOVER: Color32 = Color32::from_rgb(238, 243, 255);
pub const BORDER: Color32 = Color32::from_rgb(216, 222, 230);
pub const CANVAS: Color32 = Color32::from_rgb(228, 231, 238); // behind the screenshot
pub const ACCENT: Color32 = Color32::from_rgb(10, 126, 148); // deep teal, like a lens
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(220, 240, 244);
pub const TEXT: Color32 = Color32::from_rgb(28, 32, 41);
pub const TEXT_DIM: Color32 = Color32::from_rgb(98, 108, 122);
pub const DANGER: Color32 = Color32::from_rgb(214, 52, 60);
pub const WARN: Color32 = Color32::from_rgb(200, 110, 0);
pub const SUCCESS: Color32 = Color32::from_rgb(20, 126, 76);
/// Used only on top of the dimmed screenshot in the capture overlay.
pub const OVERLAY: Color32 = Color32::from_rgb(76, 194, 255);

pub fn apply_theme(ctx: &egui::Context) {
    load_fonts(ctx);
    let mut v = egui::Visuals::light();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::WHITE; // text boxes
    v.faint_bg_color = Color32::from_rgb(241, 243, 248);
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_rounding = egui::Rounding::same(12.0);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(10, 126, 148, 70);
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
    v.widgets.hovered.bg_fill = Color32::from_rgb(200, 228, 235);
    v.widgets.hovered.weak_bg_fill = ACCENT_SOFT;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, TEXT);

    v.widgets.active.rounding = r;
    v.widgets.active.bg_fill = Color32::from_rgb(176, 216, 226);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(192, 224, 232);
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

// ---------------------------------------------------------------- typography
/// Windows' own Segoe UI (regular + semibold) when available, so the app looks native.
fn load_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(b) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
        fonts.font_data.insert("segoe".to_owned(), egui::FontData::from_owned(b));
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "segoe".to_owned());
    }
    let mut semibold_stack = fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
    if let Ok(b) = std::fs::read(r"C:\Windows\Fonts\seguisb.ttf") {
        fonts.font_data.insert("segoe_sb".to_owned(), egui::FontData::from_owned(b));
        semibold_stack.insert(0, "segoe_sb".to_owned());
    }
    fonts.families.insert(egui::FontFamily::Name("semibold".into()), semibold_stack);
    ctx.set_fonts(fonts);
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("semibold".into()))
}

fn text_width(ui: &egui::Ui, text: &str, font: &FontId) -> f32 {
    ui.fonts(|f| f.layout_no_wrap(text.to_string(), font.clone(), TEXT).size().x)
}

// ---------------------------------------------------------------- navigation
/// Text tabs with an underline for the current one.
pub fn tab_bar(ui: &mut egui::Ui, labels: &[&str], current: &mut usize) {
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
    let p = ui.painter_at(bar);
    p.line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0, BORDER));
    let mut x = bar.min.x;
    for (i, label) in labels.iter().enumerate() {
        let font = semibold(15.0);
        let w = text_width(ui, label, &font);
        let r = Rect::from_min_size(pos2(x, bar.min.y), vec2(w + 8.0, 38.0));
        let resp = ui.interact(r, ui.id().with(("tab", i)), Sense::click());
        let selected = *current == i;
        let col = if selected {
            ACCENT
        } else if resp.hovered() {
            TEXT
        } else {
            TEXT_DIM
        };
        p.text(pos2(r.min.x + 4.0, r.center().y - 2.0), Align2::LEFT_CENTER, *label, font, col);
        if selected {
            p.rect_filled(Rect::from_min_max(pos2(r.min.x, bar.max.y - 3.0), pos2(r.max.x, bar.max.y)), 1.5, ACCENT);
        }
        if resp.clicked() {
            *current = i;
        }
        x += w + 8.0 + 22.0;
    }
}

pub fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(12.0);
    ui.label(egui::RichText::new(text).font(semibold(14.0)).color(TEXT_DIM));
    ui.add_space(2.0);
}

// ---------------------------------------------------------------- choosing what to capture
/// A rounded choice chip.
pub fn pill(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    let font = FontId::proportional(13.5);
    let w = text_width(ui, text, &font);
    let (rect, resp) = ui.allocate_exact_size(vec2(w + 30.0, 30.0), Sense::click());
    let p = ui.painter_at(rect);
    let (bg, fg, edge) = if selected {
        (ACCENT, Color32::WHITE, ACCENT)
    } else if resp.hovered() {
        (CARD_HOVER, TEXT, ACCENT)
    } else {
        (CARD, TEXT, BORDER)
    };
    p.rect_filled(rect, 15.0, bg);
    p.rect_stroke(rect, 15.0, Stroke::new(1.0, edge));
    p.text(rect.center(), Align2::CENTER_CENTER, text, font, fg);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// "Select an area" / "Full screen" / "Screen 1" ... Returns true when the choice changed.
/// target is "area", "full" or "m1", "m2" ...
pub fn target_pills(ui: &mut egui::Ui, target: &mut String, monitors: usize, all_screens: bool) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        if pill(ui, "Select an area", target == "area").clicked() {
            *target = "area".to_string();
            changed = true;
        }
        let full_label = if monitors > 1 && all_screens { "All screens" } else { "Full screen" };
        if pill(ui, full_label, target == "full").clicked() {
            *target = "full".to_string();
            changed = true;
        }
        if monitors > 1 {
            for i in 1..=monitors {
                let key = format!("m{i}");
                if pill(ui, &format!("Screen {i}"), *target == key).clicked() {
                    *target = key;
                    changed = true;
                }
            }
        }
    });
    changed
}

/// A miniature of the real monitor layout. Click a monitor to choose just that screen.
/// "All screens" lights up every monitor. Returns true when the choice changed.
pub fn screen_map(ui: &mut egui::Ui, monitors: &[(i32, i32, u32, u32)], target: &mut String) -> bool {
    let mons: Vec<(i32, i32, u32, u32)> = if monitors.is_empty() { vec![(0, 0, 1920, 1080)] } else { monitors.to_vec() };
    let min_x = mons.iter().map(|m| m.0).min().unwrap_or(0);
    let min_y = mons.iter().map(|m| m.1).min().unwrap_or(0);
    let max_x = mons.iter().map(|m| m.0 + m.2 as i32).max().unwrap_or(1920);
    let max_y = mons.iter().map(|m| m.1 + m.3 as i32).max().unwrap_or(1080);
    let (vw, vh) = (((max_x - min_x) as f32).max(1.0), ((max_y - min_y) as f32).max(1.0));

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 156.0), Sense::hover());
    let inner = rect.shrink2(vec2(24.0, 22.0));
    let k = (inner.width() / vw).min(inner.height() / vh);
    let size = vec2(vw * k, vh * k);
    let origin = inner.center() - size / 2.0;

    let p = ui.painter_at(rect);
    p.rect_filled(rect, 14.0, Color32::from_rgb(232, 238, 241)); // the desk the monitors stand on
    let all = target == "full";
    if mons.len() > 1 {
        let frame = Rect::from_min_size(origin - vec2(7.0, 7.0), size + vec2(14.0, 14.0));
        p.rect_stroke(frame, 10.0, Stroke::new(if all { 2.5 } else { 1.0 }, if all { ACCENT } else { Color32::from_rgb(190, 200, 208) }));
    }
    let mut changed = false;
    for (i, (x, y, w, h)) in mons.iter().enumerate() {
        let r = Rect::from_min_size(origin + vec2((*x - min_x) as f32 * k, (*y - min_y) as f32 * k), vec2(*w as f32 * k, *h as f32 * k)).shrink(2.5);
        let key = format!("m{}", i + 1);
        let resp = ui.interact(r, ui.id().with(("monitor", i)), Sense::click());
        let selected = all || *target == key || (mons.len() == 1 && target == "area");
        let on = all || *target == key;
        p.rect_filled(r, 6.0, if on { ACCENT_SOFT } else if resp.hovered() { CARD_HOVER } else { Color32::WHITE });
        p.rect_stroke(r, 6.0, Stroke::new(if on { 2.5 } else { 1.0 }, if on { ACCENT } else { Color32::from_rgb(160, 172, 182) }));
        let _ = selected;
        p.text(r.center() - vec2(0.0, 9.0), Align2::CENTER_CENTER, format!("{}", i + 1), semibold(24.0), if on { ACCENT } else { TEXT });
        p.text(r.center() + vec2(0.0, 14.0), Align2::CENTER_CENTER, format!("{} x {}", w, h), FontId::proportional(11.0), TEXT_DIM);
        if resp.clicked() {
            *target = if mons.len() == 1 { "full".to_string() } else { key };
            changed = true;
        }
        resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    changed
}

// ---------------------------------------------------------------- actions
/// A wide action. The main action is filled; the others are outlined. `keys` is shown as a small tag.
pub fn action_row(ui: &mut egui::Ui, title: &str, sub: &str, keys: &str, primary: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 62.0), Sense::click());
    let p = ui.painter_at(rect);
    let hov = resp.hovered();
    let (bg, fg, fg2) = if primary {
        (if hov { ACCENT.gamma_multiply(0.92) } else { ACCENT }, Color32::WHITE, Color32::from_white_alpha(215))
    } else {
        (if hov { CARD_HOVER } else { CARD }, TEXT, TEXT_DIM)
    };
    p.rect_filled(rect, 10.0, bg);
    if !primary {
        p.rect_stroke(rect, 10.0, Stroke::new(1.0, if hov { ACCENT } else { BORDER }));
    }
    p.text(pos2(rect.min.x + 20.0, rect.min.y + 22.0), Align2::LEFT_CENTER, title, semibold(17.0), fg);
    p.text(pos2(rect.min.x + 20.0, rect.min.y + 43.0), Align2::LEFT_CENTER, sub, FontId::proportional(12.5), fg2);
    if !keys.is_empty() {
        let font = FontId::monospace(12.0);
        let w = text_width(ui, keys, &font);
        let tag = Rect::from_center_size(pos2(rect.max.x - 20.0 - (w + 18.0) / 2.0, rect.center().y), vec2(w + 18.0, 24.0));
        p.rect_filled(tag, 5.0, if primary { Color32::from_white_alpha(38) } else { Color32::from_rgb(238, 242, 245) });
        p.text(tag.center(), Align2::CENTER_CENTER, keys, font, if primary { Color32::WHITE } else { TEXT_DIM });
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A plain list row: title and description on the left, one button on the right.
pub fn tool_row(ui: &mut egui::Ui, title: &str, desc: &str, button: &str) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(title).font(semibold(15.0)));
            ui.label(egui::RichText::new(desc).size(12.5).color(TEXT_DIM));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(button).clicked() {
                clicked = true;
            }
        });
    });
    ui.add_space(2.0);
    ui.separator();
    clicked
}
