//! The home window, organised in tabs: Capture, Record, Tools, Settings.
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, vec2, RichText};

use crate::config::{self, Settings};
use crate::ui::{self, ACCENT, BG, BORDER, CARD, DANGER, PANEL, SUCCESS, TEXT_DIM, WARN};
use crate::{audio, hotkeys, perms, setup, util, winapi};

enum Msg {
    Ffmpeg(Result<PathBuf, String>),
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Capture,
    Record,
    Tools,
    Settings,
}

const TABS: [(Tab, &str); 4] = [(Tab::Capture, "Capture"), (Tab::Record, "Record"), (Tab::Tools, "Tools"), (Tab::Settings, "Settings")];

#[derive(Clone, Default)]
struct TestSlot {
    running: bool,
    msg: String,
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
    tab: Tab,
    monitors: Vec<(i32, i32, u32, u32)>,
    mon_checked: Instant,
    mic_state: Option<Result<(), String>>, // None = not checked yet, Ok = allowed, Err(reason) = blocked
    mic_test: Arc<Mutex<TestSlot>>,
    sys_test: Arc<Mutex<TestSlot>>,
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
    ui.label(RichText::new(title).font(ui::semibold(14.0)));
    ui.label(RichText::new(short_path(&shown, 60)).size(12.0).color(TEXT_DIM));
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
    ui.add_space(4.0);
    changed
}

fn target_text(t: &str, monitors: usize, all: bool) -> String {
    match t {
        "area" => "You drag a rectangle on the screen.".to_string(),
        "full" if monitors > 1 && all => "Everything on all of your screens at once, with no selection.".to_string(),
        "full" => "The whole screen, with no selection.".to_string(),
        m => format!("All of screen {}, with no selection.", m.trim_start_matches('m')),
    }
}

/// "Listen for 3 seconds and tell me what you heard" - shows whether audio is really arriving.
fn test_row(ui: &mut egui::Ui, ctx: &egui::Context, slot: &Arc<Mutex<TestSlot>>, label: &str, loopback: bool, silent_msg: &'static str) {
    let state = slot.lock().map(|g| g.clone()).unwrap_or_default();
    ui.horizontal(|ui| {
        if state.running {
            ui.spinner();
            ui.label("Listening for 3 seconds...");
        } else if ui.button(label).clicked() {
            if let Ok(mut g) = slot.lock() {
                g.running = true;
                g.msg = String::new();
            }
            let (slot2, ctx2) = (slot.clone(), ctx.clone());
            thread::spawn(move || {
                let msg = match audio::test_level(loopback, 3.0) {
                    Ok(p) if p > 0.02 => format!("Heard sound (loudest {}%).", (p * 100.0).round() as u32),
                    Ok(_) => silent_msg.to_string(),
                    Err(e) => e,
                };
                if let Ok(mut g) = slot2.lock() {
                    g.running = false;
                    g.msg = msg;
                }
                ctx2.request_repaint();
            });
        }
        if !state.msg.is_empty() {
            let good = state.msg.starts_with("Heard sound");
            ui.label(RichText::new(state.msg.as_str()).color(if good { SUCCESS } else { WARN }));
        }
    });
}

pub fn run() -> Result<(), eframe::Error> {
    if winapi::show_existing_launcher() {
        return Ok(()); // already running: just bring it to the front
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([660.0, 820.0])
            .with_min_inner_size([560.0, 640.0])
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
            status: "Ready. The shortcuts work even while this window is minimised.".to_string(),
            tx,
            rx,
            ffmpeg_ok: setup::ffmpeg_path().is_some(),
            installing: false,
            progress: Arc::new(setup::Progress::default()),
            setup_error: None,
            quitting: false,
            _hotkeys: hotkeys::setup(),
            tab: Tab::Capture,
            monitors: winapi::monitors(),
            mon_checked: Instant::now(),
            mic_state: None,
            mic_test: Arc::new(Mutex::new(TestSlot::default())),
            sys_test: Arc::new(Mutex::new(TestSlot::default())),
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

    fn refresh_mic(&mut self) {
        self.mic_state = Some(match perms::mic_access() {
            perms::MicAccess::Allowed => Ok(()),
            perms::MicAccess::Blocked(reason) => Err(reason),
        });
    }

    // ------------------------------------------------------------ Capture tab
    fn tab_capture(&mut self, ui: &mut egui::Ui) {
        ui::section(ui, "What do you want to capture?");
        ui::screen_map(ui, &self.monitors, &mut self.settings.cap_target);
        ui.add_space(8.0);
        ui::target_pills(ui, &mut self.settings.cap_target, self.monitors.len().max(1), self.settings.all_screens);
        let t = self.settings.cap_target.clone();
        let what = target_text(&t, self.monitors.len(), self.settings.all_screens);
        ui.label(RichText::new(what.as_str()).size(12.5).color(TEXT_DIM));
        ui.add_space(14.0);

        let keys_shot = match t.as_str() {
            "area" => "Ctrl+Alt+A",
            "full" => "Ctrl+Alt+F",
            _ => "",
        };
        if ui::action_row(ui, "Take a screenshot", &what, keys_shot, true).clicked() {
            if t == "area" {
                util::spawn_worker("region", None);
            } else {
                util::spawn_worker("full", Some(&t));
            }
        }
        ui.add_space(8.0);
        let keys_scroll = if t == "area" { "Ctrl+Alt+S" } else { "" };
        if ui::action_row(ui, "Scrolling screenshot", "Scrolls the page for you and joins it into one tall picture.", keys_scroll, false).clicked() {
            util::spawn_worker("scroll", Some(&t));
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button("Copy text from the screen").on_hover_text("Drag over text and it goes to the clipboard (Ctrl+Alt+T)").clicked() {
                util::spawn_worker("ocr", None);
            }
            if ui.button("Pin part of the screen").on_hover_text("Keeps a piece of the screen floating above other windows (Ctrl+Alt+N)").clicked() {
                util::spawn_worker("pinregion", None);
            }
        });
        ui::section(ui, "Timing");
        ui.horizontal(|ui| {
            ui.label("Wait");
            ui.add(egui::Slider::new(&mut self.settings.delay, 0..=15).suffix(" s"));
            ui.label("before a screenshot, with a countdown on screen.");
        });
    }

    // ------------------------------------------------------------ Record tab
    fn tab_record(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.mic_state.is_none() {
            self.refresh_mic();
        }
        ui::section(ui, "What do you want to record?");
        ui::screen_map(ui, &self.monitors, &mut self.settings.rec_target);
        ui.add_space(8.0);
        ui::target_pills(ui, &mut self.settings.rec_target, self.monitors.len().max(1), self.settings.all_screens);
        let t = self.settings.rec_target.clone();
        let what = target_text(&t, self.monitors.len(), self.settings.all_screens);
        ui.label(RichText::new(what.as_str()).size(12.5).color(TEXT_DIM));
        ui.add_space(14.0);
        let keys = match t.as_str() {
            "area" => "Ctrl+Alt+R",
            "full" => "Ctrl+Alt+V",
            _ => "",
        };
        if ui::action_row(ui, "Start recording", "A short countdown first. A bar with Pause, Screenshot and Stop appears while you record.", keys, true).clicked() {
            util::spawn_worker("record", Some(&t));
        }

        ui::section(ui, "Sound");
        ui.checkbox(&mut self.settings.mic, "Record my voice (microphone)");
        let mut recheck = false;
        match &self.mic_state {
            Some(Ok(())) => {
                ui.label(RichText::new("Windows allows SnipForge to use the microphone.").size(12.5).color(SUCCESS));
            }
            Some(Err(reason)) => {
                ui.label(RichText::new(reason.as_str()).color(DANGER));
                ui.horizontal(|ui| {
                    if ui::primary_button(ui, "Open microphone settings").clicked() {
                        perms::open_mic_settings();
                    }
                    if ui.button("Check again").clicked() {
                        recheck = true;
                    }
                });
            }
            None => {}
        }
        if recheck {
            self.mic_state = None;
        }
        test_row(
            ui,
            ctx,
            &self.mic_test.clone(),
            "Test microphone",
            false,
            "Heard only silence. Say something while testing, or choose the right microphone in Windows sound settings.",
        );
        ui.add_space(8.0);
        ui.checkbox(&mut self.settings.system_audio, "Record device audio (what you hear through speakers or headphones)");
        test_row(
            ui,
            ctx,
            &self.sys_test.clone(),
            "Test device audio",
            true,
            "Heard only silence. Play some music or a video while testing, and check that your speakers or headphones are the default playback device.",
        );
        if ui.small_button("Open Windows sound settings").clicked() {
            perms::open_sound_settings();
        }

        ui::section(ui, "Quality and timing");
        ui.horizontal(|ui| {
            ui.label("Frames per second");
            egui::ComboBox::from_id_source("fps").selected_text(self.settings.fps.to_string()).show_ui(ui, |ui| {
                for f in [10u32, 15, 24, 30, 60] {
                    ui.selectable_value(&mut self.settings.fps, f, f.to_string());
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("Countdown before recording or scrolling");
            ui.add(egui::Slider::new(&mut self.settings.rec_countdown, 0..=10).suffix(" s"));
        });
        ui.add_space(6.0);
        ui.label(RichText::new("While recording: Ctrl+Alt+P pauses, Ctrl+Alt+K takes a screenshot of the recorded area.").size(12.0).color(TEXT_DIM));
    }

    // ------------------------------------------------------------ Tools tab
    fn tab_tools(&mut self, ui: &mut egui::Ui) {
        ui::section(ui, "Your captures");
        if ui::tool_row(ui, "History", "Browse and search every screenshot, recording and extracted text. Ctrl+Alt+H", "Open history") {
            util::spawn_worker("history", None);
        }
        if ui::tool_row(ui, "Screenshots folder", &short_path(&self.settings.shots_path().to_string_lossy(), 64), "Open") {
            util::open_in_explorer(&self.settings.shots_path());
        }
        if ui::tool_row(ui, "Recordings folder", &short_path(&self.settings.videos_path().to_string_lossy(), 64), "Open") {
            util::open_in_explorer(&self.settings.videos_path());
        }
        ui::section(ui, "Edit and keep");
        if ui::tool_row(ui, "Edit a video", "Cut parts out, change the speed, add captions, export.", "Choose video...") {
            util::spawn_worker("video", None);
        }
        if ui::tool_row(ui, "Edit a picture", "Open any picture in the annotation editor.", "Choose picture...") {
            if let Some(p) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "bmp"]).pick_file() {
                util::spawn_worker_path("open", &p);
            }
        }
        if ui::tool_row(ui, "Pin the clipboard picture", "Keeps a picture you copied floating above other windows.", "Pin it") {
            match util::clipboard_image() {
                Some(img) => {
                    let path = util::temp_png("snipforge_pin");
                    if img.save(&path).is_ok() {
                        util::spawn_worker_path("pin", &path);
                    }
                }
                None => self.status = "There is no picture on the clipboard.".to_string(),
            }
        }
    }

    // ------------------------------------------------------------ Settings tab
    fn tab_settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> bool {
        let mut engine_changed = false;
        ui::section(ui, "Screens");
        ui.label("When you have several monitors, \"Full screen\" means:");
        ui.radio_value(&mut self.settings.all_screens, true, "All screens together (one picture, one selection overlay)");
        ui.radio_value(&mut self.settings.all_screens, false, "Only the main screen");

        ui::section(ui, "Pictures and text");
        ui.checkbox(&mut self.settings.auto_copy, "Copy every screenshot to the clipboard");
        ui.checkbox(&mut self.settings.save_ocr_text, "Save copied text as a .txt file too");

        ui::section(ui, "Where files are saved");
        folder_row(ui, "Screenshots", &mut self.settings.shots_dir, &config::default_shots_dir());
        folder_row(ui, "Recordings", &mut self.settings.videos_dir, &config::default_videos_dir());
        folder_row(ui, "Copied text", &mut self.settings.text_dir, &config::default_text_dir());

        ui::section(ui, "Recording engine");
        if self.ffmpeg_ok {
            ui.label(RichText::new("Installed. Recording and the video editor are ready.").color(SUCCESS));
        } else if self.installing {
            ui.label("Setting up the recording engine (one-time download)...");
            ui.add(egui::ProgressBar::new(self.progress.fraction()).text(self.progress.text()));
        } else {
            ui.label(RichText::new("Not installed yet. Recording and the video editor need it.").color(DANGER));
            if let Some(e) = &self.setup_error {
                ui.label(RichText::new(e.as_str()).size(12.0).color(DANGER));
            }
            ui.horizontal_wrapped(|ui| {
                if ui::primary_button(ui, "Download and install").clicked() {
                    self.start_install(ctx);
                }
                if ui.button("I have ffmpeg.exe...").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("ffmpeg", &["exe"]).pick_file() {
                        match setup::import_ffmpeg(&p) {
                            Ok(_) => {
                                self.ffmpeg_ok = true;
                                self.setup_error = None;
                            }
                            Err(e) => self.setup_error = Some(e),
                        }
                    }
                }
                if ui.button("Open download page").clicked() {
                    util::open_url(setup::DOWNLOAD_PAGE);
                }
            });
        }
        if folder_row(ui, "Where the recording engine is kept", &mut self.settings.engine_dir, &config::default_engine_dir()) {
            engine_changed = true;
        }

        ui::section(ui, "Keyboard shortcuts");
        egui::Grid::new("keys").num_columns(2).spacing([18.0, 6.0]).show(ui, |ui| {
            for (k, d) in [
                ("Ctrl+Alt+A", "Screenshot of an area"),
                ("Ctrl+Alt+F", "Screenshot of everything"),
                ("Ctrl+Alt+S", "Scrolling screenshot (press again to stop)"),
                ("Ctrl+Alt+T", "Copy text from the screen"),
                ("Ctrl+Alt+N", "Pin part of the screen"),
                ("Ctrl+Alt+H", "History"),
                ("Ctrl+Alt+R", "Record an area (press again to stop)"),
                ("Ctrl+Alt+V", "Record the whole screen"),
                ("Ctrl+Alt+P", "Pause or resume the recording"),
                ("Ctrl+Alt+K", "Screenshot while recording"),
            ] {
                ui.label(RichText::new(k).monospace().color(ACCENT));
                ui.label(d);
                ui.end_row();
            }
        });
        ui.add_space(14.0);
        ui.label(RichText::new("Closing this window only minimises it, so the shortcuts keep working.").size(12.0).color(TEXT_DIM));
        if ui.button("Quit SnipForge").clicked() {
            self.quitting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        engine_changed
    }
}

impl eframe::App for Launcher {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // closing the window only minimises it, so the shortcuts keep working
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
        // keep the screen map in step with plugged / unplugged monitors
        if self.mon_checked.elapsed() > Duration::from_secs(2) {
            self.mon_checked = Instant::now();
            self.monitors = winapi::monitors();
            for t in [&mut self.settings.cap_target, &mut self.settings.rec_target] {
                if let Some(n) = t.strip_prefix('m').and_then(|n| n.parse::<usize>().ok()) {
                    if n > self.monitors.len() {
                        *t = "area".to_string();
                    }
                }
            }
            ctx.request_repaint_after(Duration::from_secs(2));
        }

        egui::TopBottomPanel::top("head")
            .frame(egui::Frame::none().fill(PANEL).inner_margin(egui::Margin { left: 24.0, right: 24.0, top: 16.0, bottom: 0.0 }))
            .show(ctx, |ui| {
                ui.label(RichText::new("SnipForge").font(ui::semibold(26.0)).color(ACCENT));
                ui.add_space(6.0);
                let mut idx = TABS.iter().position(|(t, _)| *t == self.tab).unwrap_or(0);
                let labels: Vec<&str> = TABS.iter().map(|(_, l)| *l).collect();
                ui::tab_bar(ui, &labels, &mut idx);
                self.tab = TABS[idx].0;
            });
        egui::TopBottomPanel::bottom("foot")
            .frame(egui::Frame::none().fill(PANEL).stroke(egui::Stroke::new(1.0, BORDER)).inner_margin(egui::Margin::symmetric(24.0, 7.0)))
            .show(ctx, |ui| {
                ui.label(RichText::new(self.status.as_str()).size(12.0).color(TEXT_DIM));
            });

        let mut engine_changed = false;
        egui::CentralPanel::default().frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::symmetric(24.0, 6.0))).show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(ui.available_width());
                match self.tab {
                    Tab::Capture => self.tab_capture(ui),
                    Tab::Record => self.tab_record(ui, ctx),
                    Tab::Tools => self.tab_tools(ui),
                    Tab::Settings => engine_changed = self.tab_settings(ui, ctx),
                }
                ui.add_space(20.0);
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
        let _ = CARD;
    }
}
