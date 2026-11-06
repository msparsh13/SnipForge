//! The home window: big action buttons, settings, and one-time setup.
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use eframe::egui::{self, vec2, Color32, RichText};

use crate::config::{self, Settings};
use crate::ui::{self, Icon, ACCENT, BG, BORDER, CARD, DANGER, SUCCESS, TEXT_DIM};
use crate::{hotkeys, setup, util, winapi};

enum Msg {
    Ffmpeg(Result<PathBuf, String>),
}

struct Launcher {
    settings: Settings,
    saved: Settings,
    status: String,
    tx: mpsc::Sender<Msg>,
    rx: mpsc::Receiver<Msg>,
    ffmpeg_ok: bool,
    installing: bool,
    progress: Arc<setup::Progress>,
    setup_error: Option<String>,
    quitting: bool,
    _hotkeys: Option<global_hotkey::GlobalHotKeyManager>,
}

fn short_path(p: &str, max: usize) -> String {
    let n = p.chars().count();
    if n <= max {
        p.to_string()
    } else {
        let tail: String = p.chars().skip(n - (max - 3)).collect();
        format!("...{tail}")
    }
}

/// One "folder" setting: shows the current folder with Change / Use default / Open buttons.
/// Returns true when the value changed.
fn folder_row(ui: &mut egui::Ui, title: &str, value: &mut String, default: &Path) -> bool {
    let mut changed = false;
    let shown = if value.is_empty() { default.to_string_lossy().to_string() } else { value.clone() };
    ui.label(RichText::new(title).strong());
    ui.label(RichText::new(short_path(&shown, 52)).size(12.0).color(TEXT_DIM));
    ui.horizontal(|ui| {
        if ui.button("Change...").clicked() {
            if let Some(p) = rfd::FileDialog::new().set_directory(&shown).pick_folder() {
                *value = p.to_string_lossy().to_string();
                changed = true;
            }
        }
        if ui.add_enabled(!value.is_empty(), egui::Button::new("Use default")).clicked() {
            value.clear();
            changed = true;
        }
        if ui.button("Open").clicked() {
            let _ = std::fs::create_dir_all(&shown);
            util::open_in_explorer(Path::new(&shown));
        }
    });
    ui.add_space(2.0);
    changed
}

pub fn run() -> Result<(), eframe::Error> {
    if winapi::show_existing_launcher() {
        return Ok(()); // already running: just bring it to the front
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([560.0, 800.0])
            .with_min_inner_size([480.0, 600.0])
            .with_title("SnipForge"),
        ..Default::default()
    };
    eframe::run_native(
        "SnipForge",
        options,
        Box::new(|cc| {
            ui::apply_theme(&cc.egui_ctx);
            Box::new(Launcher::new(&cc.egui_ctx))
        }),
    )
}

impl Launcher {
    fn new(ctx: &egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        let settings = Settings::load();
        let mut l = Launcher {
            saved: settings.clone(),
            settings,
            status: "Ready. Your hotkeys work even when this window is minimised.".to_string(),
            tx,
            rx,
            ffmpeg_ok: setup::ffmpeg_path().is_some(),
            installing: false,
            progress: Arc::new(setup::Progress::default()),
            setup_error: None,
            quitting: false,
            _hotkeys: hotkeys::setup(),
        };
        if !l.ffmpeg_ok {
            l.start_install(ctx); // set up the recording engine automatically
        }
        l
    }

    fn start_install(&mut self, ctx: &egui::Context) {
        if self.installing {
            return;
        }
        self.installing = true;
        self.setup_error = None;
        let tx = self.tx.clone();
        let ctx2 = ctx.clone();
        let prog = self.progress.clone();
        prog.reset();
        thread::spawn(move || {
            let _ = tx.send(Msg::Ffmpeg(setup::install_ffmpeg(prog)));
            ctx2.request_repaint();
        });
    }
}

impl eframe::App for Launcher {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // closing the window only minimises it, so hotkeys keep working
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Ffmpeg(Ok(_)) => {
                    self.installing = false;
                    self.ffmpeg_ok = true;
                    self.status = "Recording engine installed.".to_string();
                }
                Msg::Ffmpeg(Err(e)) => {
                    self.installing = false;
                    self.setup_error = Some(e);
                }
            }
        }
        if self.installing {
            ctx.request_repaint_after(Duration::from_millis(250));
        }

        let mut act: Option<&'static str> = None;
        let mut retry_install = false;
        let mut pick_ffmpeg = false;
        let mut open_page = false;
        let mut engine_changed = false;
        let mut quit = false;

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::same(20.0)))
            .show(ctx, |ui| {
              egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("SnipForge").size(30.0).strong().color(ACCENT));
                    ui.label(RichText::new("capture - annotate - record").size(13.0).color(TEXT_DIM));
                });
                ui.add_space(8.0);

                let w = (ui.available_width() - 8.0) / 2.0;
                let sz = vec2(w, 78.0);
                ui.horizontal(|ui| {
                    if ui::card(ui, Icon::Region, "Region", "Ctrl+Alt+A", sz).clicked() {
                        act = Some("region");
                    }
                    if ui::card(ui, Icon::Screen, "Full screen", "Ctrl+Alt+F", sz).clicked() {
                        act = Some("full");
                    }
                });
                ui.horizontal(|ui| {
                    if ui::card(ui, Icon::Text, "Grab text", "Ctrl+Alt+T", sz).clicked() {
                        act = Some("ocr");
                    }
                    if ui::card(ui, Icon::Record, "Record", "Ctrl+Alt+R   Pause: Ctrl+Alt+P", sz).clicked() {
                        act = Some("record");
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Open image to edit...").clicked() {
                        act = Some("open");
                    }
                    if ui.button("Screenshots folder").clicked() {
                        act = Some("folder_shots");
                    }
                    if ui.button("Recordings folder").clicked() {
                        act = Some("folder_videos");
                    }
                });

                ui.add_space(6.0);
                egui::Frame::none().fill(CARD).stroke(egui::Stroke::new(1.0, BORDER)).rounding(12.0).inner_margin(egui::Margin::same(14.0)).show(ui, |ui| {
                    ui.label(RichText::new("Settings").strong());
                    ui.horizontal(|ui| {
                        ui.label("Delay before capture:");
                        ui.add(egui::Slider::new(&mut self.settings.delay, 0..=15).suffix(" s"));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Recording FPS:");
                        egui::ComboBox::from_id_source("fps")
                            .selected_text(self.settings.fps.to_string())
                            .show_ui(ui, |ui| {
                                for f in [10u32, 15, 24, 30, 60] {
                                    ui.selectable_value(&mut self.settings.fps, f, f.to_string());
                                }
                            });
                    });
                    ui.checkbox(&mut self.settings.mic, "Record microphone");
                    ui.checkbox(&mut self.settings.system_audio, "Record device audio (what you hear through speakers / headphones)");
                    ui.checkbox(&mut self.settings.auto_copy, "Copy screenshots to the clipboard automatically");
                    ui.add_space(2.0);
                    ui.label("Capture area (several monitors):");
                    ui.radio_value(&mut self.settings.all_screens, true, "All screens together (one image / one overlay)");
                    ui.radio_value(&mut self.settings.all_screens, false, "Primary screen only");
                });

                ui.add_space(6.0);
                egui::Frame::none().fill(CARD).stroke(egui::Stroke::new(1.0, BORDER)).rounding(12.0).inner_margin(egui::Margin::same(14.0)).show(ui, |ui| {
                    ui.label(RichText::new("Save locations").size(15.0).strong());
                    ui.add_space(2.0);
                    folder_row(ui, "Screenshots", &mut self.settings.shots_dir, &config::default_shots_dir());
                    folder_row(ui, "Screen recordings", &mut self.settings.videos_dir, &config::default_videos_dir());
                    folder_row(ui, "Extracted text (OCR)", &mut self.settings.text_dir, &config::default_text_dir());
                    ui.checkbox(&mut self.settings.save_ocr_text, "Also save extracted text as a .txt file");
                    ui.add_space(4.0);
                    if folder_row(ui, "Recording engine (ffmpeg)", &mut self.settings.engine_dir, &config::default_engine_dir()) {
                        engine_changed = true;
                    }
                });

                ui.add_space(6.0);
                egui::Frame::none().fill(CARD).stroke(egui::Stroke::new(1.0, BORDER)).rounding(12.0).inner_margin(egui::Margin::same(14.0)).show(ui, |ui| {
                    ui.label(RichText::new("Components").strong());
                    if self.ffmpeg_ok {
                        ui.horizontal(|ui| {
                            ui.colored_label(SUCCESS, "Ready");
                            ui.label("Recording engine (ffmpeg)");
                        });
                    } else if self.installing {
                        ui.label("Setting up the recording engine (one-time download)...");
                        ui.add(egui::ProgressBar::new(self.progress.fraction()).text(self.progress.text()));
                    } else {
                        ui.horizontal(|ui| {
                            ui.colored_label(DANGER, "Missing");
                            ui.label("Recording engine (ffmpeg)");
                        });
                        if let Some(e) = &self.setup_error {
                            ui.colored_label(DANGER, e.as_str());
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui::primary_button(ui, "Install now").clicked() {
                                retry_install = true;
                            }
                            if ui.button("Choose ffmpeg.exe...").clicked() {
                                pick_ffmpeg = true;
                            }
                            if ui.button("Open download page").clicked() {
                                open_page = true;
                            }
                        });
                    }
                    ui.horizontal(|ui| {
                        ui.colored_label(SUCCESS, "Ready");
                        ui.label("Text recognition (built into Windows, nothing to install)");
                    });
                });

                ui.add_space(6.0);
                ui.label(RichText::new(self.status.as_str()).size(12.0).color(TEXT_DIM));
                ui.label(
                    RichText::new("Closing this window minimises it so the hotkeys keep working. Use Quit to exit completely.")
                        .size(11.0)
                        .color(TEXT_DIM),
                );
                ui.add_space(4.0);
                if ui.button("Quit SnipForge").clicked() {
                    quit = true;
                }
              });
            });

        if self.settings != self.saved {
            self.settings.save();
            self.saved = self.settings.clone();
        }
        if engine_changed {
            self.settings.save();
            self.saved = self.settings.clone();
            self.ffmpeg_ok = setup::ffmpeg_path().is_some();
            if !self.ffmpeg_ok {
                self.start_install(ctx); // set the engine up in the new folder
            }
        }
        if retry_install {
            self.start_install(ctx);
        }
        if open_page {
            util::open_url(setup::DOWNLOAD_PAGE);
        }
        if pick_ffmpeg {
            if let Some(p) = rfd::FileDialog::new().add_filter("ffmpeg", &["exe"]).pick_file() {
                match setup::import_ffmpeg(&p) {
                    Ok(_) => {
                        self.ffmpeg_ok = true;
                        self.setup_error = None;
                        self.status = "Recording engine ready.".to_string();
                    }
                    Err(e) => self.setup_error = Some(e),
                }
            }
        }
        match act {
            Some("open") => {
                if let Some(p) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "bmp"]).pick_file() {
                    let path_str = p.to_string_lossy().to_string();
                    util::spawn_worker("open", Some(&path_str));
                }
            }
            Some("folder_shots") => util::open_in_explorer(&self.settings.shots_path()),
            Some("folder_videos") => util::open_in_explorer(&self.settings.videos_path()),
            Some(kind) => util::spawn_worker(kind, None),
            None => {}
        }
        if quit {
            self.quitting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
