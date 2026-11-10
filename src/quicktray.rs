//! Compact quick-actions panel opened by Print Screen.
use eframe::egui::{self, RichText};
use crate::{ui, util};

pub fn run() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SnipForge Quick Actions")
            .with_inner_size([430.0, 390.0])
            .with_min_inner_size([390.0, 340.0])
            .with_resizable(false)
            .with_always_on_top(),
        ..Default::default()
    };
    eframe::run_native(
        "SnipForge Quick Actions",
        options,
        Box::new(|cc| {
            ui::apply_theme(&cc.egui_ctx);
            Box::new(QuickTray)
        }),
    )
}

struct QuickTray;

impl QuickTray {
    fn action(ui: &mut egui::Ui, title: &str, subtitle: &str, key: &str) -> bool {
        ui.add_sized(
            [126.0, 76.0],
            egui::Button::new(
                RichText::new(format!("{title}\n{}", subtitle))
                    .size(13.0)
                    .strong(),
            )
            .wrap(true),
        )
        .on_hover_text(format!("{title} · {key}"))
        .clicked()
    }

    fn launch(kind: &str, arg: Option<&str>) {
        util::spawn_worker(kind, arg);
    }
}

impl eframe::App for QuickTray {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut close = false;
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("SnipForge");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").clicked() { close = true; }
                    ui.label(RichText::new("QUICK ACTIONS").size(10.0).color(ui::TEXT_DIM));
                });
            });
            ui.label(RichText::new("Capture, record and manage your screenshots").size(11.5).color(ui::TEXT_DIM));
            ui.add_space(12.0);

            ui.label(RichText::new("CAPTURE").size(10.0).strong().color(ui::TEXT_DIM));
            ui.add_space(5.0);
            egui::Grid::new("capture_actions").num_columns(3).spacing([7.0, 7.0]).show(ui, |ui| {
                if Self::action(ui, "▣  Region", "Select an area", "PrtSc") { Self::launch("region", None); close = true; }
                if Self::action(ui, "▢  Full screen", "Capture display", "Ctrl+Alt+F") { Self::launch("full", None); close = true; }
                if Self::action(ui, "▤  Scrolling", "Long screenshot", "Ctrl+Alt+S") { Self::launch("scroll", None); close = true; }
                ui.end_row();
            });
            ui.add_space(10.0);
            ui.label(RichText::new("TOOLS").size(10.0).strong().color(ui::TEXT_DIM));
            ui.add_space(5.0);
            egui::Grid::new("tool_actions").num_columns(3).spacing([7.0, 7.0]).show(ui, |ui| {
                if Self::action(ui, "●  Record", "Screen recording", "Ctrl+Alt+R") { Self::launch("record", None); close = true; }
                if Self::action(ui, "Aa  OCR", "Extract text", "Ctrl+Alt+T") { Self::launch("ocr", None); close = true; }
                if Self::action(ui, "⌖  Pin", "Pin a screen region", "Ctrl+Alt+N") { Self::launch("pinregion", None); close = true; }
                ui.end_row();
                if Self::action(ui, "◷  History", "Recent captures", "Ctrl+Alt+H") { Self::launch("history", None); close = true; }
                if Self::action(ui, "▧  Image editor", "Open an image", "") {
                    if let Some(path) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "bmp"]).pick_file() {
                        util::spawn_worker_path("open", &path);
                    }
                    close = true;
                }
                if Self::action(ui, "▤  Video editor", "Open a recording", "") {
                    if let Some(path) = rfd::FileDialog::new().add_filter("Videos", &["mp4", "mkv", "webm", "mov"]).pick_file() {
                        util::spawn_worker_path("video", &path);
                    }
                    close = true;
                }
                ui.end_row();
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Open SnipForge").clicked() {
                    if let Ok(exe) = std::env::current_exe() { let _ = std::process::Command::new(exe).spawn(); }
                    close = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("Hold PrtSc + press S to open").size(10.0).color(ui::TEXT_DIM));
                });
            });
        });
        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
