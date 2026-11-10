//! SnipForge (Rust edition) - screenshot, annotate, OCR and record on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod captions;
mod capture;
mod config;
mod editor;
mod gdi;
mod histview;
mod hotkeys;
mod launcher;
mod media;
mod ocr;
mod perms;
mod pinview;
mod quicktray;
mod record;
mod render;
mod setup;
mod stitch;
mod ui;
mod util;
mod videoeditor;
mod winapi;
mod worker;

fn main() -> Result<(), eframe::Error> {
    winapi::set_dpi_aware();
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "--worker" {
        let extra = args.get(3).cloned();
        return match args[2].as_str() {
            "pin" => pinview::run(extra.unwrap_or_default()),
            "tray" => quicktray::run(),
            "history" => histview::run(),
            "video" => videoeditor::run(extra),
            other => worker::run(other, extra),
        };
    }
    launcher::run()
}
