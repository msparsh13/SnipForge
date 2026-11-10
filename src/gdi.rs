//! GDI screen grabbing. Unlike DXGI, it can capture the whole virtual desktop
//! (every monitor in one image) and any rectangle that spans several monitors.
use std::ffi::c_void;
use std::ptr;

type Hdc = isize;

#[repr(C)]
struct BitmapInfoHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_ppm: i32,
    y_ppm: i32,
    clr_used: u32,
    clr_important: u32,
}

#[repr(C)]
struct BitmapInfo {
    header: BitmapInfoHeader,
    colors: [u32; 1],
}

#[link(name = "user32")]
extern "system" {
    fn GetDC(hwnd: isize) -> Hdc;
    fn ReleaseDC(hwnd: isize, hdc: Hdc) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    fn CreateCompatibleDC(hdc: Hdc) -> Hdc;
    fn CreateDIBSection(
        hdc: Hdc,
        bmi: *const BitmapInfo,
        usage: u32,
        bits: *mut *mut c_void,
        section: isize,
        offset: u32,
    ) -> isize;
    fn SelectObject(hdc: Hdc, obj: isize) -> isize;
    fn BitBlt(
        dst: Hdc,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        src: Hdc,
        sx: i32,
        sy: i32,
        rop: u32,
    ) -> i32;
    fn DeleteObject(obj: isize) -> i32;
    fn DeleteDC(hdc: Hdc) -> i32;
}

/// Reusable grabber for one rectangle of the screen (physical pixels, may be on any monitor).
pub struct GdiGrabber {
    screen: Hdc,
    mem: Hdc,
    bmp: isize,
    old: isize,
    bits: *mut u8,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

unsafe impl Send for GdiGrabber {}

impl GdiGrabber {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Result<GdiGrabber, String> {
        if w <= 0 || h <= 0 {
            return Err("empty capture area".to_string());
        }
        unsafe {
            let screen = GetDC(0);
            if screen == 0 {
                return Err("could not access the screen".to_string());
            }
            let mem = CreateCompatibleDC(screen);
            let bmi = BitmapInfo {
                header: BitmapInfoHeader {
                    size: 40,
                    width: w,
                    height: -h, // negative = top-down rows
                    planes: 1,
                    bit_count: 32,
                    compression: 0,
                    size_image: 0,
                    x_ppm: 0,
                    y_ppm: 0,
                    clr_used: 0,
                    clr_important: 0,
                },
                colors: [0],
            };
            let mut bits: *mut c_void = ptr::null_mut();
            let bmp = CreateDIBSection(screen, &bmi, 0, &mut bits, 0, 0);
            if bmp == 0 || bits.is_null() {
                DeleteDC(mem);
                ReleaseDC(0, screen);
                return Err("could not allocate the capture buffer".to_string());
            }
            let old = SelectObject(mem, bmp);
            Ok(GdiGrabber {
                screen,
                mem,
                bmp,
                old,
                bits: bits as *mut u8,
                x,
                y,
                w,
                h,
            })
        }
    }

    /// Copies the rectangle from the screen; returns BGRA pixels (top-down, alpha unused).
    pub fn grab(&mut self) -> &[u8] {
        unsafe {
            // SRCCOPY | CAPTUREBLT (also captures layered / transparent windows)
            BitBlt(
                self.mem,
                0,
                0,
                self.w,
                self.h,
                self.screen,
                self.x,
                self.y,
                0x00CC0020 | 0x4000_0000,
            );
            std::slice::from_raw_parts(self.bits, (self.w as usize) * (self.h as usize) * 4)
        }
    }
}

impl Drop for GdiGrabber {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.mem, self.old);
            DeleteObject(self.bmp);
            DeleteDC(self.mem);
            ReleaseDC(0, self.screen);
        }
    }
}
