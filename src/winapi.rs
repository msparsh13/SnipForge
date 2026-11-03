//! Tiny hand-declared Win32 helpers (no extra crates needed).
use std::ptr;

type Hwnd = isize;

#[link(name = "user32")]
extern "system" {
    fn EnumWindows(cb: unsafe extern "system" fn(Hwnd, isize) -> i32, lparam: isize) -> i32;
    fn GetWindowTextW(hwnd: Hwnd, buf: *mut u16, max: i32) -> i32;
    fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
    fn IsWindowVisible(hwnd: Hwnd) -> i32;
    fn ShowWindow(hwnd: Hwnd, cmd: i32) -> i32;
    fn SetWindowDisplayAffinity(hwnd: Hwnd, affinity: u32) -> i32;
    fn FindWindowW(class: *const u16, title: *const u16) -> Hwnd;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcessId() -> u32;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn minimize_cb(hwnd: Hwnd, _l: isize) -> i32 {
    if IsWindowVisible(hwnd) != 0 {
        let mut buf = [0u16; 256];
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), 256);
        if n > 0 {
            let title = String::from_utf16_lossy(&buf[..n as usize]);
            if title.starts_with("SnipForge") {
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, &mut pid);
                if pid != GetCurrentProcessId() {
                    ShowWindow(hwnd, 6); // SW_MINIMIZE
                }
            }
        }
    }
    1
}

/// Minimise every other SnipForge window (launcher, old editors) so they are not in the screenshot.
pub fn minimize_other_windows() {
    unsafe {
        EnumWindows(minimize_cb, 0);
    }
}

/// Hide a window (by exact title) from screen capture / recording. Returns true on success.
pub fn exclude_from_capture(title: &str) -> bool {
    unsafe {
        let t = wide(title);
        let h = FindWindowW(ptr::null(), t.as_ptr());
        if h == 0 {
            return false;
        }
        SetWindowDisplayAffinity(h, 0x11) != 0 // WDA_EXCLUDEFROMCAPTURE
    }
}

/// If a launcher window already exists, bring it to the front and return true.
pub fn show_existing_launcher() -> bool {
    unsafe {
        let t = wide("SnipForge");
        let h = FindWindowW(ptr::null(), t.as_ptr());
        if h == 0 {
            false
        } else {
            ShowWindow(h, 9); // SW_RESTORE
            true
        }
    }
}
