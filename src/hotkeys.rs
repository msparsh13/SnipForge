//! Global hotkeys. Each one simply starts a worker process, so it works
//! even while the main window is minimised.
use std::collections::HashMap;
use std::thread;
use std::time::{Duration, Instant};

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
}

/// Keep the returned manager alive for as long as the app runs.
pub fn setup() -> Option<GlobalHotKeyManager> {
    let mgr = GlobalHotKeyManager::new().ok()?;
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
