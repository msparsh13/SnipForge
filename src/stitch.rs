//! Scrolling capture: scroll the content under the pointer, grab frames, and stitch them.
//! The scroll distance is calibrated automatically (about a third of the area per step), each step waits
//! for the page to stop moving, and the page is brought to the front first so it accepts the scrolling.
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use eframe::egui;
use image::RgbaImage;

use crate::gdi::GdiGrabber;
use crate::winapi;

const MAX_STEPS: u32 = 150;

enum Shift {
    Moved(usize), // content moved up by this many pixels
    Same,         // nothing moved
    Unknown,      // could not line the frames up
}

pub struct ScrollOutcome {
    pub image: RgbaImage,
    pub note: String, // empty when everything went fine
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
    if mean_diff > 12.0 {
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

/// True when two frames are (nearly) identical, i.e. smooth scrolling has finished.
fn frames_close(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() || a.is_empty() {
        return false;
    }
    let (mut diff, mut n, mut i) = (0u64, 0u64, 0usize);
    while i < a.len() {
        diff += (a[i] as i32 - b[i] as i32).unsigned_abs() as u64;
        n += 1;
        i += 4 * 97 + 1;
    }
    (diff as f64 / n.max(1) as f64) < 0.8
}

/// Wait until the area stops changing, then return that frame.
fn settle(grab: &mut GdiGrabber) -> Vec<u8> {
    thread::sleep(Duration::from_millis(140));
    let mut last = grab.grab().to_vec();
    for _ in 0..14 {
        thread::sleep(Duration::from_millis(110));
        let cur = grab.grab().to_vec();
        if frames_close(&last, &cur) {
            return cur;
        }
        last = cur;
    }
    last
}

pub struct ScrollJob {
    stop: Arc<AtomicBool>,
    steps: Arc<AtomicU32>,
    rows: Arc<AtomicU32>,
    handle: Option<JoinHandle<Result<ScrollOutcome, String>>>,
}

impl ScrollJob {
    /// region = (x, y, width, height) in screen pixels
    pub fn start(ctx: egui::Context, region: (i32, i32, u32, u32)) -> ScrollJob {
        let stop = Arc::new(AtomicBool::new(false));
        let steps = Arc::new(AtomicU32::new(0));
        let rows = Arc::new(AtomicU32::new(region.3));
        let (s2, st2, r2) = (stop.clone(), steps.clone(), rows.clone());
        let handle = thread::spawn(move || {
            let r = run(&ctx, region, &s2, &st2, &r2);
            ctx.request_repaint();
            r
        });
        ScrollJob {
            stop,
            steps,
            rows,
            handle: Some(handle),
        }
    }

    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn steps(&self) -> u32 {
        self.steps.load(Ordering::Relaxed)
    }

    /// Height of the stitched picture so far, in pixels.
    pub fn rows(&self) -> u32 {
        self.rows.load(Ordering::Relaxed)
    }

    pub fn poll(&mut self) -> Option<Result<ScrollOutcome, String>> {
        let done = self
            .handle
            .as_ref()
            .map(|h| h.is_finished())
            .unwrap_or(false);
        if !done {
            return None;
        }
        let h = self.handle.take()?;
        Some(
            h.join()
                .unwrap_or_else(|_| Err("scrolling capture crashed".to_string())),
        )
    }
}

fn run(
    ctx: &egui::Context,
    region: (i32, i32, u32, u32),
    stop: &AtomicBool,
    steps: &AtomicU32,
    rows_out: &AtomicU32,
) -> Result<ScrollOutcome, String> {
    let (x, y, w, h) = region;
    let (wu, hu) = (w as usize, h as usize);
    let mut grab = GdiGrabber::new(x, y, w as i32, h as i32)?;

    // let the selection window shrink out of the way, then bring the page to the front and park the pointer on it
    thread::sleep(Duration::from_millis(1200));
    let (cx, cy) = (x + w as i32 / 2, y + h as i32 / 2);
    winapi::set_cursor(cx, cy);
    winapi::focus_window_at(cx, cy);
    thread::sleep(Duration::from_millis(400));

    let first = settle(&mut grab);
    let mut prev_luma = luma(&first);
    let mut pieces: Vec<Vec<u8>> = vec![first];
    let mut rows = hu;
    let mut notches: i32 = 1; // start gently, then calibrate
    let mut done_steps = 0u32;
    let mut note = String::new();

    while !stop.load(Ordering::SeqCst) && done_steps < MAX_STEPS {
        // only scroll while the pointer is over the page, so you can move away to press Stop
        let (px, py) = winapi::cursor_pos();
        if px < x || px >= x + w as i32 || py < y || py >= y + h as i32 {
            thread::sleep(Duration::from_millis(150));
            continue;
        }
        winapi::wheel(-120 * notches);
        let cur = settle(&mut grab);
        let cur_luma = luma(&cur);
        match find_shift(&prev_luma, &cur_luma, wu, hu) {
            Shift::Moved(dy) if dy > 0 && dy <= hu => {
                pieces.push(cur[(hu - dy) * wu * 4..].to_vec());
                rows += dy;
                prev_luma = cur_luma;
                done_steps += 1;
                steps.store(done_steps, Ordering::Relaxed);
                rows_out.store(rows as u32, Ordering::Relaxed);
                ctx.request_repaint();
                // aim for about a third of the area per step: far enough to be quick, short enough to line up
                let per_notch = (dy as f32 / notches as f32).max(1.0);
                notches = ((hu as f32 * 0.33) / per_notch).round().clamp(1.0, 12.0) as i32;
            }
            Shift::Same => {
                if done_steps == 0 {
                    note = "Nothing scrolled. Point at the part of the page that scrolls, or the page may already fit in the area.".to_string();
                }
                break; // end of the page
            }
            _ => {
                note = format!("Stopped after {done_steps} steps: the page changed too much to line up the next piece (video, animation or a pop-up?).");
                break;
            }
        }
    }

    let mut rgba = Vec::with_capacity(rows * wu * 4);
    for piece in &pieces {
        for px in piece.chunks_exact(4) {
            rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
        }
    }
    let image = RgbaImage::from_raw(w, rows as u32, rgba)
        .ok_or_else(|| "could not build the stitched image".to_string())?;
    Ok(ScrollOutcome { image, note })
}
