//! Screen grabbing through DXGI desktop duplication (the `scrap` crate).
use std::io::ErrorKind;
use std::thread;
use std::time::{Duration, Instant};

use image::RgbaImage;
use scrap::{Capturer, Display};

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
    Display::primary().ok().map(|d| (d.width() as u32, d.height() as u32))
}
