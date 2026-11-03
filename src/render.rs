//! CPU renderer for annotations. The editor preview and the saved file both come
//! from this code, so what you see is exactly what you save.
use eframe::egui::{Color32, Pos2};
use image::RgbaImage;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

#[derive(Clone, Debug)]
pub enum Ann {
    Arrow { a: Pos2, b: Pos2, color: Color32, w: f32 },
    Rect { a: Pos2, b: Pos2, color: Color32, w: f32 },
    Ellipse { a: Pos2, b: Pos2, color: Color32, w: f32 },
    Pen { pts: Vec<Pos2>, color: Color32, w: f32 },
    Highlight { pts: Vec<Pos2>, color: Color32, w: f32 },
    Text { p: Pos2, text: String, color: Color32, size: f32 },
    Step { p: Pos2, n: u32, color: Color32, w: f32 },
    Blur { a: Pos2, b: Pos2 },
}

impl Ann {
    /// Shift the annotation (used when the image is cropped).
    pub fn translate(&mut self, dx: f32, dy: f32) {
        let d = eframe::egui::vec2(dx, dy);
        match self {
            Ann::Arrow { a, b, .. } | Ann::Rect { a, b, .. } | Ann::Ellipse { a, b, .. } | Ann::Blur { a, b } => {
                *a += d;
                *b += d;
            }
            Ann::Pen { pts, .. } | Ann::Highlight { pts, .. } => {
                for p in pts.iter_mut() {
                    *p += d;
                }
            }
            Ann::Text { p, .. } | Ann::Step { p, .. } => {
                *p += d;
            }
        }
    }
}

pub fn load_font() -> Option<fontdue::Font> {
    for p in [
        r"C:\Windows\Fonts\segoeuib.ttf",
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\arial.ttf",
    ] {
        if let Ok(bytes) = std::fs::read(p) {
            if let Ok(f) = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()) {
                return Some(f);
            }
        }
    }
    None
}

pub fn render(base: &RgbaImage, anns: &[Ann], font: Option<&fontdue::Font>) -> RgbaImage {
    let (w, h) = base.dimensions();
    let mut pm = match Pixmap::new(w, h) {
        Some(p) => p,
        None => return base.clone(),
    };
    pm.data_mut().copy_from_slice(base.as_raw());
    for ann in anns {
        draw(&mut pm, ann, font);
    }
    RgbaImage::from_raw(w, h, pm.take()).unwrap_or_else(|| base.clone())
}

fn paint_for(c: Color32, alpha: f32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c.r(), c.g(), c.b(), (255.0 * alpha) as u8);
    p.anti_alias = true;
    p
}

fn stroke_of(width: f32, cap: LineCap) -> Stroke {
    Stroke {
        width,
        line_cap: cap,
        line_join: LineJoin::Round,
        ..Stroke::default()
    }
}

fn norm_rect(a: Pos2, b: Pos2) -> Option<tiny_skia::Rect> {
    tiny_skia::Rect::from_ltrb(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y))
}

fn poly_path(pts: &[Pos2]) -> Option<tiny_skia::Path> {
    if pts.len() < 2 {
        return None;
    }
    let mut pb = PathBuilder::new();
    pb.move_to(pts[0].x, pts[0].y);
    for p in &pts[1..] {
        pb.line_to(p.x, p.y);
    }
    pb.finish()
}

fn draw(pm: &mut Pixmap, ann: &Ann, font: Option<&fontdue::Font>) {
    match ann {
        Ann::Arrow { a, b, color, w } => {
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            if (dx * dx + dy * dy).sqrt() < 1.0 {
                return;
            }
            let ang = dy.atan2(dx);
            let head = 10.0 + *w * 2.5;
            let h1 = (b.x - head * (ang - 0.45).cos(), b.y - head * (ang - 0.45).sin());
            let h2 = (b.x - head * (ang + 0.45).cos(), b.y - head * (ang + 0.45).sin());
            let mid = ((h1.0 + h2.0) / 2.0, (h1.1 + h2.1) / 2.0);
            let paint = paint_for(*color, 1.0);

            let mut pb = PathBuilder::new();
            pb.move_to(a.x, a.y);
            pb.line_to(mid.0, mid.1);
            if let Some(path) = pb.finish() {
                pm.stroke_path(&path, &paint, &stroke_of(*w, LineCap::Round), Transform::identity(), None);
            }
            let mut pb = PathBuilder::new();
            pb.move_to(b.x, b.y);
            pb.line_to(h1.0, h1.1);
            pb.line_to(h2.0, h2.1);
            pb.close();
            if let Some(path) = pb.finish() {
                pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
            }
        }
        Ann::Rect { a, b, color, w } => {
            if let Some(r) = norm_rect(*a, *b) {
                let path = PathBuilder::from_rect(r);
                pm.stroke_path(&path, &paint_for(*color, 1.0), &stroke_of(*w, LineCap::Butt), Transform::identity(), None);
            }
        }
        Ann::Ellipse { a, b, color, w } => {
            if let Some(r) = norm_rect(*a, *b) {
                if let Some(path) = PathBuilder::from_oval(r) {
                    pm.stroke_path(&path, &paint_for(*color, 1.0), &stroke_of(*w, LineCap::Butt), Transform::identity(), None);
                }
            }
        }
        Ann::Pen { pts, color, w } => {
            if let Some(path) = poly_path(pts) {
                pm.stroke_path(&path, &paint_for(*color, 1.0), &stroke_of(*w, LineCap::Round), Transform::identity(), None);
            }
        }
        Ann::Highlight { pts, color, w } => {
            if let Some(path) = poly_path(pts) {
                pm.stroke_path(&path, &paint_for(*color, 0.38), &stroke_of(*w * 4.0 + 6.0, LineCap::Round), Transform::identity(), None);
            }
        }
        Ann::Text { p, text, color, size } => {
            if let Some(f) = font {
                draw_text(pm, f, *p, text, *size, *color);
            }
        }
        Ann::Step { p, n, color, w } => {
            let r = 13.0 + *w * 1.5;
            if let Some(path) = PathBuilder::from_circle(p.x, p.y, r) {
                pm.fill_path(&path, &paint_for(*color, 1.0), FillRule::Winding, Transform::identity(), None);
                pm.stroke_path(&path, &paint_for(Color32::WHITE, 1.0), &stroke_of(2.0, LineCap::Butt), Transform::identity(), None);
            }
            if let Some(f) = font {
                let size = r * 1.2;
                let label = n.to_string();
                let tw: f32 = label.chars().map(|c| f.metrics(c, size).advance_width).sum();
                draw_text(pm, f, Pos2::new(p.x - tw / 2.0, p.y - size * 0.64), &label, size, Color32::WHITE);
            }
        }
        Ann::Blur { a, b } => pixelate(pm, *a, *b, 12),
    }
}

fn draw_text(pm: &mut Pixmap, f: &fontdue::Font, p: Pos2, text: &str, size: f32, c: Color32) {
    let (pw, ph) = (pm.width() as i32, pm.height() as i32);
    let rgb = [c.r() as u32, c.g() as u32, c.b() as u32];
    let data = pm.data_mut();
    let mut top = p.y;
    for line in text.lines() {
        let mut x = p.x;
        let baseline = (top + size) as i32;
        for ch in line.chars() {
            let (m, bmp) = f.rasterize(ch, size);
            let gx = x as i32 + m.xmin;
            let gy = baseline - m.ymin - m.height as i32;
            for row in 0..m.height {
                for col in 0..m.width {
                    let cov = bmp[row * m.width + col] as u32;
                    if cov == 0 {
                        continue;
                    }
                    let px = gx + col as i32;
                    let py = gy + row as i32;
                    if px < 0 || py < 0 || px >= pw || py >= ph {
                        continue;
                    }
                    let idx = ((py * pw + px) * 4) as usize;
                    for k in 0..3 {
                        let dst = data[idx + k] as u32;
                        data[idx + k] = ((rgb[k] * cov + dst * (255 - cov)) / 255) as u8;
                    }
                }
            }
            x += m.advance_width;
        }
        top += size * 1.25;
    }
}

fn pixelate(pm: &mut Pixmap, a: Pos2, b: Pos2, block: i32) {
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    let x0 = (a.x.min(b.x) as i32).clamp(0, w);
    let x1 = (a.x.max(b.x) as i32).clamp(0, w);
    let y0 = (a.y.min(b.y) as i32).clamp(0, h);
    let y1 = (a.y.max(b.y) as i32).clamp(0, h);
    let data = pm.data_mut();
    let mut by = y0;
    while by < y1 {
        let mut bx = x0;
        while bx < x1 {
            let ex = (bx + block).min(x1);
            let ey = (by + block).min(y1);
            let mut sum = [0u32; 4];
            let mut n = 0u32;
            for y in by..ey {
                for x in bx..ex {
                    let i = ((y * w + x) * 4) as usize;
                    for k in 0..4 {
                        sum[k] += data[i + k] as u32;
                    }
                    n += 1;
                }
            }
            if n > 0 {
                let avg = [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8, (sum[3] / n) as u8];
                for y in by..ey {
                    for x in bx..ex {
                        let i = ((y * w + x) * 4) as usize;
                        data[i..i + 4].copy_from_slice(&avg);
                    }
                }
            }
            bx += block;
        }
        by += block;
    }
}
