//! Scrolling capture: scroll the content under the pointer, grab frames, and stitch them.
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use eframe::egui;
use image::RgbaImage;

use crate::gdi::GdiGrabber;
use crate::winapi;

const MAX_STEPS: u32 = 120;

enum Shift {
    Moved(usize), // content moved up by this many pixels
    Same,         // nothing moved: end of the page
    Unknown,
}

fn luma(bgra: &[u8]) -> Vec<u8> {
    bgra.chunks_exact(4)
        .map(|p| ((p[2] as u32 * 77 + p[1] as u32 * 150 + p[0] as u32 * 29) >> 8) as u8)
        .collect()
}

/// How far did the page scroll between `prev` and `cur`? Takes a strip from the middle of the new frame
/// (so sticky headers are ignored) and finds where it was in the previous frame.
fn find_shift(prev: &[u8], cur: &[u8], w: usize, h: usize) -> Shift {
    if h < 80 || w < 40 {
        return Shift::Unknown;
    }
    let th = (h / 5).clamp(40, h / 2);
    let y0 = (h / 3).min(h - th);
    let (cstep, rstep) = (4usize, 2usize);

    // an almost uniform strip cannot be matched reliably
    let (mut sum, mut sum2, mut n) = (0f64, 0f64, 0f64);
    let mut ty = 0;
    while ty < th {
        let row = (y0 + ty) * w;
        let mut x = 0;
        while x < w {
            let v = cur[row + x] as f64;
            sum += v;
            sum2 += v * v;
            n += 1.0;
            x += cstep;
        }
        ty += rstep;
    }
    let var = sum2 / n - (sum / n) * (sum / n);
    if var < 4.0 {
        return Shift::Unknown;
    }

    let mut best = (u64::MAX, 0usize);
    for ym in 0..=(h - th) {
        let mut sad = 0u64;
        let mut ty = 0;
        'rows: while ty < th {
            let (prow, crow) = ((ym + ty) * w, (y0 + ty) * w);
            let mut x = 0;
            while x < w {
                sad += (prev[prow + x] as i32 - cur[crow + x] as i32).unsigned_abs() as u64;
                x += cstep;
            }
            if sad >= best.0 {
                break 'rows; // cannot beat the best match any more
            }
            ty += rstep;
        }
        if sad < best.0 {
            best = (sad, ym);
        }
    }
    let samples = ((th + rstep - 1) / rstep) as u64 * ((w + cstep - 1) / cstep) as u64;
    let mean_diff = best.0 as f64 / samples as f64;
    if mean_diff > 6.0 {
        return Shift::Unknown;
    }
    if best.1 == y0 {
        Shift::Same
    } else if best.1 > y0 {
        Shift::Moved(best.1 - y0)
    } else {
        Shift::Unknown
    }
}

pub struct ScrollJob {
    stop: Arc<AtomicBool>,
    steps: Arc<AtomicU32>,
    handle: Option<JoinHandle<Result<RgbaImage, String>>>,
}

impl ScrollJob {
    /// region = (x, y, width, height) in screen pixels
    pub fn start(ctx: egui::Context, region: (i32, i32, u32, u32)) -> ScrollJob {
        let stop = Arc::new(AtomicBool::new(false));
        let steps = Arc::new(AtomicU32::new(0));
        let (s2, st2) = (stop.clone(), steps.clone());
        let handle = thread::spawn(move || {
            let r = run(&ctx, region, &s2, &st2);
            ctx.request_repaint();
            r
        });
        ScrollJob { stop, steps, handle: Some(handle) }
    }

    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn steps(&self) -> u32 {
        self.steps.load(Ordering::Relaxed)
    }

    pub fn poll(&mut self) -> Option<Result<RgbaImage, String>> {
        let done = self.handle.as_ref().map(|h| h.is_finished()).unwrap_or(false);
        if !done {
            return None;
        }
        let h = self.handle.take()?;
        Some(h.join().unwrap_or_else(|_| Err("scrolling capture crashed".to_string())))
    }
}

fn run(ctx: &egui::Context, region: (i32, i32, u32, u32), stop: &AtomicBool, steps: &AtomicU32) -> Result<RgbaImage, String> {
    let (x, y, w, h) = region;
    let (wu, hu) = (w as usize, h as usize);
    let mut grab = GdiGrabber::new(x, y, w as i32, h as i32)?;

    // let the selection window shrink out of the way, then park the pointer over the page
    thread::sleep(Duration::from_millis(900));
    winapi::set_cursor(x + w as i32 / 2, y + h as i32 / 2);
    thread::sleep(Duration::from_millis(250));

    let first = grab.grab().to_vec();
    let mut prev_luma = luma(&first);
    let mut pieces: Vec<Vec<u8>> = vec![first];
    let mut rows = hu;
    let mut done_steps = 0u32;

    while !stop.load(Ordering::SeqCst) && done_steps < MAX_STEPS {
        // only scroll while the pointer is over the page, so you can move away to press Stop
        let (px, py) = winapi::cursor_pos();
        if px < x || px >= x + w as i32 || py < y || py >= y + h as i32 {
            thread::sleep(Duration::from_millis(150));
            continue;
        }
        winapi::wheel(-360); // three notches down
        thread::sleep(Duration::from_millis(450));
        let cur = grab.grab().to_vec();
        let cur_luma = luma(&cur);
        match find_shift(&prev_luma, &cur_luma, wu, hu) {
            Shift::Moved(dy) if dy > 0 && dy <= hu => {
                pieces.push(cur[(hu - dy) * wu * 4..].to_vec());
                rows += dy;
                prev_luma = cur_luma;
                done_steps += 1;
                steps.store(done_steps, Ordering::Relaxed);
                ctx.request_repaint();
            }
            _ => break, // end of the page (or no reliable match)
        }
    }

    let mut rgba = Vec::with_capacity(rows * wu * 4);
    for piece in &pieces {
        for px in piece.chunks_exact(4) {
            rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
        }
    }
    RgbaImage::from_raw(w, rows as u32, rgba).ok_or_else(|| "could not build the stitched image".to_string())
}
