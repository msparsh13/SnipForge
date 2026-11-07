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
    fn SetWindowPos(hwnd: Hwnd, after: Hwnd, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    fn SetForegroundWindow(hwnd: Hwnd) -> i32;
    fn GetSystemMetrics(index: i32) -> i32;
    fn SetCursorPos(x: i32, y: i32) -> i32;
    fn GetCursorPos(p: *mut Point) -> i32;
    fn mouse_event(flags: u32, dx: i32, dy: i32, data: i32, extra: usize);
    fn SetProcessDpiAwarenessContext(value: isize) -> i32;
    fn EnumDisplayMonitors(hdc: isize, clip: *const Rect4, cb: MonitorEnumProc, data: isize) -> i32;
    fn GetMonitorInfoW(monitor: isize, info: *mut MonitorInfo) -> i32;
}

#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Rect4 {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct MonitorInfo {
    size: u32,
    monitor: Rect4,
    work: Rect4,
    flags: u32,
}

type MonitorEnumProc = unsafe extern "system" fn(isize, isize, *mut Rect4, isize) -> i32;

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

/// Make a window cover exactly this screen rectangle (physical pixels), above everything
/// including the taskbar. Used for the capture overlay so it always fills the whole screen.
pub fn cover_screen(title: &str, x: i32, y: i32, w: i32, h: i32) -> bool {
    unsafe {
        let t = wide(title);
        let hwnd = FindWindowW(ptr::null(), t.as_ptr());
        if hwnd == 0 {
            return false;
        }
        // HWND_TOPMOST = -1, SWP_SHOWWINDOW = 0x0040
        let ok = SetWindowPos(hwnd, -1, x, y, w, h, 0x0040) != 0;
        SetForegroundWindow(hwnd);
        ok
    }
}

/// Undo "always on top" for a window (HWND_NOTOPMOST = -2; NOSIZE|NOMOVE|NOACTIVATE).
pub fn release_topmost(title: &str) {
    unsafe {
        let t = wide(title);
        let hwnd = FindWindowW(ptr::null(), t.as_ptr());
        if hwnd != 0 {
            SetWindowPos(hwnd, -2, 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010);
        }
    }
}

/// Make the process per-monitor DPI aware so every coordinate is in real pixels.
pub fn set_dpi_aware() {
    unsafe {
        SetProcessDpiAwarenessContext(-4); // PER_MONITOR_AWARE_V2
    }
}

/// Bounding box of ALL monitors together: (x, y, width, height) in physical pixels. x/y can be negative.
pub fn virtual_rect() -> (i32, i32, u32, u32) {
    unsafe {
        (
            GetSystemMetrics(76),
            GetSystemMetrics(77),
            GetSystemMetrics(78).max(1) as u32,
            GetSystemMetrics(79).max(1) as u32,
        )
    }
}

unsafe extern "system" fn monitor_cb(h: isize, _hdc: isize, _r: *mut Rect4, data: isize) -> i32 {
    let list = &mut *(data as *mut Vec<(i32, i32, u32, u32)>);
    let zero = Rect4 { left: 0, top: 0, right: 0, bottom: 0 };
    let mut mi = MonitorInfo { size: std::mem::size_of::<MonitorInfo>() as u32, monitor: zero, work: zero, flags: 0 };
    if GetMonitorInfoW(h, &mut mi) != 0 {
        list.push((
            mi.monitor.left,
            mi.monitor.top,
            (mi.monitor.right - mi.monitor.left).max(1) as u32,
            (mi.monitor.bottom - mi.monitor.top).max(1) as u32,
        ));
    }
    1
}

/// Every monitor as (x, y, width, height), ordered left to right.
pub fn monitors() -> Vec<(i32, i32, u32, u32)> {
    let mut v: Vec<(i32, i32, u32, u32)> = Vec::new();
    unsafe {
        EnumDisplayMonitors(0, ptr::null(), monitor_cb, &mut v as *mut Vec<(i32, i32, u32, u32)> as isize);
    }
    v.sort_by_key(|m| (m.0, m.1));
    v
}

/// Pointer position in screen pixels.
pub fn cursor_pos() -> (i32, i32) {
    let mut p = Point { x: 0, y: 0 };
    unsafe {
        GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

pub fn set_cursor(x: i32, y: i32) {
    unsafe {
        SetCursorPos(x, y);
    }
}

/// Turn the mouse wheel (negative = scroll down; 120 = one notch).
pub fn wheel(delta: i32) {
    unsafe {
        mouse_event(0x0800, 0, 0, delta, 0); // MOUSEEVENTF_WHEEL
    }
}

/// Keep a window above all others without moving or resizing it.
pub fn make_topmost(title: &str) {
    unsafe {
        let t = wide(title);
        let hwnd = FindWindowW(ptr::null(), t.as_ptr());
        if hwnd != 0 {
            SetWindowPos(hwnd, -1, 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010); // HWND_TOPMOST, NOSIZE|NOMOVE|NOACTIVATE
        }
    }
}
