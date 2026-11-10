//! Global hotkeys. Each one simply starts a worker process, so it works
//! even while the main window is minimised.
use std::collections::HashMap;
use std::thread;
use std::time::{Duration, Instant};
use std::sync::atomic::{AtomicBool, Ordering};

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::util;

#[derive(Clone, Copy)]
enum Cmd {
    Region,
    Full,
    Ocr,
    Record,
    Pause,
    RecordFull,
    Scroll,
    PinRegion,
    History,
    Snap,
    Tray,
}

/// Keep the returned manager alive for as long as the app runs.
pub fn setup() -> Option<GlobalHotKeyManager> {
    let mgr = GlobalHotKeyManager::new().ok()?;
    start_printscreen_s_hook();
    let mods = Modifiers::CONTROL | Modifiers::ALT;
    let mut map: HashMap<u32, Cmd> = HashMap::new();
    for (code, cmd) in [
        (Code::KeyA, Cmd::Region),
        (Code::KeyF, Cmd::Full),
        (Code::KeyT, Cmd::Ocr),
        (Code::KeyR, Cmd::Record),
        (Code::KeyP, Cmd::Pause),
        (Code::KeyV, Cmd::RecordFull),
        (Code::KeyS, Cmd::Scroll),
        (Code::KeyN, Cmd::PinRegion),
        (Code::KeyH, Cmd::History),
        (Code::KeyK, Cmd::Snap),
    ] {
        let hk = HotKey::new(Some(mods), code);
        let id = hk.id();
        if mgr.register(hk).is_ok() {
            map.insert(id, cmd);
        }
    }
    thread::spawn(move || {
        let receiver = GlobalHotKeyEvent::receiver();
        let mut last: Option<Instant> = None;
        while let Ok(ev) = receiver.recv() {
            if ev.state != HotKeyState::Pressed {
                continue;
            }
            if let Some(t) = last {
                if t.elapsed() < Duration::from_millis(900) {
                    continue; // ignore key repeat / double press
                }
            }
            if let Some(cmd) = map.get(&ev.id) {
                last = Some(Instant::now());
                match cmd {
                    Cmd::Tray => util::spawn_worker("tray", None),
                    Cmd::Region => util::spawn_worker("region", None),
                    Cmd::Full => util::spawn_worker("full", None),
                    Cmd::Ocr => util::spawn_worker("ocr", None),
                    Cmd::Scroll => {
                        if util::recording_active() {
                            util::request_stop(); // also stops a running scrolling capture
                        } else {
                            util::spawn_worker("scroll", None);
                        }
                    }
                    Cmd::PinRegion => util::spawn_worker("pinregion", None),
                    Cmd::History => util::spawn_worker("history", None),
                    Cmd::Snap => {
                        if util::recording_active() {
                            util::request_snap();
                        }
                    }
                    Cmd::RecordFull => {
                        if util::recording_active() {
                            util::request_stop();
                        } else {
                            util::spawn_worker("record", Some("full"));
                        }
                    }
                    Cmd::Pause => {
                        if util::recording_active() {
                            util::request_pause();
                        }
                    }
                    Cmd::Record => {
                        if util::recording_active() {
                            util::request_stop();
                        } else {
                            util::spawn_worker("record", None);
                        }
                    }
                }
            }
        }
    });
    Some(mgr)
}


// Print Screen is not a supported modifier in RegisterHotKey/global-hotkey.
// Use a low-level keyboard hook to detect the physical chord: hold PrtSc, then press S.
static PRINTSCREEN_HELD: AtomicBool = AtomicBool::new(false);
static TRAY_CHORD_FIRED: AtomicBool = AtomicBool::new(false);

type Hhook = isize;
type Hinstance = isize;
type Lresult = isize;

#[repr(C)]
struct KbdLlHookStruct {
    vk_code: u32,
    scan_code: u32,
    flags: u32,
    time: u32,
    extra_info: usize,
}

#[repr(C)]
struct Msg {
    hwnd: isize,
    message: u32,
    w_param: usize,
    l_param: isize,
    time: u32,
    pt_x: i32,
    pt_y: i32,
}

#[link(name = "user32")]
extern "system" {
    fn SetWindowsHookExW(id_hook: i32, callback: Option<unsafe extern "system" fn(i32, usize, isize) -> Lresult>, instance: Hinstance, thread_id: u32) -> Hhook;
    fn CallNextHookEx(hook: Hhook, code: i32, w_param: usize, l_param: isize) -> Lresult;
    fn GetMessageW(msg: *mut Msg, hwnd: isize, min: u32, max: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> Lresult;
}

unsafe extern "system" fn keyboard_hook(code: i32, w_param: usize, l_param: isize) -> Lresult {
    const HC_ACTION: i32 = 0;
    const WM_KEYDOWN: usize = 0x0100;
    const WM_KEYUP: usize = 0x0101;
    const WM_SYSKEYDOWN: usize = 0x0104;
    const WM_SYSKEYUP: usize = 0x0105;
    const VK_S: u32 = 0x53;
    const VK_SNAPSHOT: u32 = 0x2C;

    if code == HC_ACTION {
        let key = &*(l_param as *const KbdLlHookStruct);
        match w_param {
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                if key.vk_code == VK_SNAPSHOT {
                    PRINTSCREEN_HELD.store(true, Ordering::SeqCst);
                    TRAY_CHORD_FIRED.store(false, Ordering::SeqCst);
                } else if key.vk_code == VK_S
                    && PRINTSCREEN_HELD.load(Ordering::SeqCst)
                    && !TRAY_CHORD_FIRED.swap(true, Ordering::SeqCst)
                {
                    util::spawn_worker("tray", None);
                }
            }
            WM_KEYUP | WM_SYSKEYUP => {
                if key.vk_code == VK_SNAPSHOT {
                    PRINTSCREEN_HELD.store(false, Ordering::SeqCst);
                    TRAY_CHORD_FIRED.store(false, Ordering::SeqCst);
                }
            }
            _ => {}
        }
    }
    CallNextHookEx(0, code, w_param, l_param)
}

fn start_printscreen_s_hook() {
    thread::spawn(|| unsafe {
        const WH_KEYBOARD_LL: i32 = 13;
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), 0, 0);
        if hook == 0 {
            return;
        }
        let mut msg = std::mem::MaybeUninit::<Msg>::zeroed();
        loop {
            let result = GetMessageW(msg.as_mut_ptr(), 0, 0, 0);
            if result <= 0 {
                break;
            }
            let message = msg.assume_init_ref();
            TranslateMessage(message);
            DispatchMessageW(message);
        }
    });
}
