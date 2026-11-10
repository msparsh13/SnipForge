//! Screen grabbing through DXGI desktop duplication (the `scrap` crate).
use std::io::ErrorKind;
use std::thread;
use std::time::{Duration, Instant};

use image::RgbaImage;
use scrap::{Capturer, Display};

use crate::gdi::GdiGrabber;
use crate::winapi;

/// Grab the whole primary display as RGBA (physical pixels).
pub fn grab_primary() -> Result<RgbaImage, String> {
    let display = Display::primary().map_err(|e| e.to_string())?;
    let mut cap = Capturer::new(display).map_err(|e| e.to_string())?;
    let (w, h) = (cap.width(), cap.height());
    let start = Instant::now();
    loop {
        match cap.frame() {
            Ok(frame) => {
                let stride = frame.len() / h;
                let mut out = vec![0u8; w * h * 4];
                for y in 0..h {
                    let row = &frame[y * stride..y * stride + w * 4];
                    let dst = &mut out[y * w * 4..(y + 1) * w * 4];
                    for x in 0..w {
                        dst[x * 4] = row[x * 4 + 2]; // BGRA -> RGBA
                        dst[x * 4 + 1] = row[x * 4 + 1];
                        dst[x * 4 + 2] = row[x * 4];
                        dst[x * 4 + 3] = 255;
                    }
                }
                return RgbaImage::from_raw(w as u32, h as u32, out)
                    .ok_or_else(|| "bad frame".to_string());
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                if start.elapsed() > Duration::from_secs(3) {
                    return Err("Timed out waiting for the screen".to_string());
                }
                thread::sleep(Duration::from_millis(15));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

pub fn crop(img: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
    image::imageops::crop_imm(img, x, y, w, h).to_image()
}

/// Physical pixel size of the primary display.
pub fn primary_size() -> Option<(u32, u32)> {
    Display::primary()
        .ok()
        .map(|d| (d.width() as u32, d.height() as u32))
}

fn looks_black(rgba: &[u8]) -> bool {
    // sample whole pixels (all colour channels); a real desktop is never completely black
    rgba.chunks_exact(4)
        .step_by(997)
        .all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0)
}

fn gdi_grab(x: i32, y: i32, w: u32, h: u32) -> Result<RgbaImage, String> {
    let mut g = GdiGrabber::new(x, y, w as i32, h as i32)?;
    let data = g.grab();
    let mut out = vec![0u8; data.len()];
    for (src, dst) in data.chunks_exact(4).zip(out.chunks_exact_mut(4)) {
        dst[0] = src[2]; // BGRA -> RGBA
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = 255;
    }
    if looks_black(&out) {
        return Err("screen grab returned a black image".to_string());
    }
    RgbaImage::from_raw(w, h, out).ok_or_else(|| "bad frame".to_string())
}

/// Capture every screen into ONE image (all = true) or only the primary screen.
/// Returns the image and the screen coordinates of its top-left corner.
pub fn grab_screens(all: bool) -> Result<(RgbaImage, (i32, i32)), String> {
    if all {
        let (x, y, w, h) = winapi::virtual_rect();
        if let Ok(img) = gdi_grab(x, y, w, h) {
            return Ok((img, (x, y)));
        }
    }
    grab_primary().map(|img| (img, (0, 0)))
}

/// Grab any rectangle of the screen (it may be on any monitor, or span several).
pub fn grab_rect(x: i32, y: i32, w: u32, h: u32) -> Result<RgbaImage, String> {
    match gdi_grab(x, y, w, h) {
        Ok(img) => Ok(img),
        Err(e) => {
            // GDI sometimes returns black for protected content; the primary screen has a second method
            if let Some((pw, ph)) = primary_size() {
                if (x, y, w, h) == (0, 0, pw, ph) {
                    return grab_primary();
                }
            }
            Err(e)
        }
    }
}
