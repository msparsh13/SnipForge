//! SnipForge (Rust edition) - screenshot, annotate, OCR and record on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod config;
mod editor;
mod hotkeys;
mod launcher;
mod ocr;
mod record;
mod render;
mod setup;
mod ui;
mod util;
mod winapi;
mod worker;

fn main() -> Result<(), eframe::Error> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "--worker" {
        return worker::run(&args[2], args.get(3).cloned());
    }
    launcher::run()
}
