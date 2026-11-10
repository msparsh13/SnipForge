//! History: browse screenshots, recordings and extracted text; search by name, date or by the
//! text found inside screenshots (after "Index text").
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::SystemTime;

use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, RichText, Sense, Stroke};

use crate::config::Settings;
use crate::ui::{self, ACCENT, BG, BORDER, CARD, DANGER, PANEL, TEXT_DIM};
use crate::util::no_window;
use crate::{ocr, setup, util};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Image,
    Video,
    Text,
}

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    name: String,
    kind: Kind,
    modified: SystemTime,
    size: u64,
}

type Index = Arc<Mutex<HashMap<String, String>>>;

fn index_file() -> PathBuf {
    let d = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SnipForge");
    let _ = std::fs::create_dir_all(&d);
    d.join("history_index.tsv")
}

fn load_index() -> HashMap<String, String> {
    let mut m = HashMap::new();
    if let Ok(text) = std::fs::read_to_string(index_file()) {
        for line in text.lines() {
            if let Some((p, t)) = line.split_once('\t') {
                m.insert(p.to_string(), t.to_string());
            }
        }
    }
    m
}

fn scan(settings: &Settings) -> Vec<Entry> {
    let mut v: Vec<Entry> = Vec::new();
    for dir in [
        settings.shots_path(),
        settings.videos_path(),
        settings.text_path(),
    ] {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                let ext = p
                    .extension()
                    .and_then(|x| x.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                let kind = match ext.as_str() {
                    "png" | "jpg" | "jpeg" | "bmp" => Kind::Image,
                    "mp4" | "webm" | "mkv" | "mov" | "gif" => Kind::Video,
                    "txt" => Kind::Text,
                    _ => continue,
                };
                if let Ok(meta) = e.metadata() {
                    v.push(Entry {
                        name: p
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        path: p,
                        kind,
                        modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                        size: meta.len(),
                    });
                }
            }
        }
    }
    v.sort_by(|a, b| b.modified.cmp(&a.modified));
    v.dedup_by(|a, b| a.path == b.path);
    v.truncate(400);
    v
}

fn video_thumb(ff: &Path, p: &Path) -> Option<egui::ColorImage> {
    let mut c = Command::new(ff);
    c.args(["-loglevel", "error", "-ss", "0.5", "-i"]).arg(p);
    c.args([
        "-frames:v",
        "1",
        "-vf",
        "scale=220:-2",
        "-f",
        "image2pipe",
        "-vcodec",
        "png",
        "pipe:1",
    ]);
    c.stdout(Stdio::piped()).stderr(Stdio::null());
    no_window(&mut c);
    let out = c.output().ok()?;
    let img = image::load_from_memory(&out.stdout).ok()?.to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [img.width() as usize, img.height() as usize],
        img.as_raw(),
    ))
}

fn spawn_thumbs(
    items: Vec<(PathBuf, Kind)>,
    tx: mpsc::Sender<(PathBuf, egui::ColorImage)>,
    ctx: egui::Context,
) {
    thread::spawn(move || {
        let ff = setup::ffmpeg_path();
        for (p, kind) in items {
            let ci = match kind {
                Kind::Image => image::open(&p).ok().map(|i| {
                    let t = i.thumbnail(220, 124).to_rgba8();
                    egui::ColorImage::from_rgba_unmultiplied(
                        [t.width() as usize, t.height() as usize],
                        t.as_raw(),
                    )
                }),
                Kind::Video => ff.as_ref().and_then(|f| video_thumb(f, &p)),
                Kind::Text => None,
            };
            if let Some(ci) = ci {
                if tx.send((p, ci)).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        }
    });
}

struct HistApp {
    settings: Settings,
    entries: Vec<Entry>,
    query: String,
    filter: u8, // 0 all, 1 images, 2 videos, 3 text
    selected: Option<PathBuf>,
    confirm_delete: Option<PathBuf>,
    thumbs: HashMap<PathBuf, egui::TextureHandle>,
    thumb_tx: mpsc::Sender<(PathBuf, egui::ColorImage)>,
    thumb_rx: mpsc::Receiver<(PathBuf, egui::ColorImage)>,
    index: Index,
    idx_running: Arc<AtomicBool>,
    idx_cancel: Arc<AtomicBool>,
    idx_done: Arc<AtomicUsize>,
    idx_total: Arc<AtomicUsize>,
    status: String,
}

pub fn run() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SnipForge - History")
            .with_inner_size([1040.0, 740.0]),
        ..Default::default()
    };
    eframe::run_native(
        "SnipForge",
        options,
        Box::new(|cc| {
            ui::apply_theme(&cc.egui_ctx);
            Box::new(HistApp::new(&cc.egui_ctx))
        }),
    )
}

impl HistApp {
    fn new(ctx: &egui::Context) -> Self {
        let settings = Settings::load();
        let (thumb_tx, thumb_rx) = mpsc::channel();
        let mut app = HistApp {
            entries: Vec::new(),
            query: String::new(),
            filter: 0,
            selected: None,
            confirm_delete: None,
            thumbs: HashMap::new(),
            thumb_tx,
            thumb_rx,
            index: Arc::new(Mutex::new(load_index())),
            idx_running: Arc::new(AtomicBool::new(false)),
            idx_cancel: Arc::new(AtomicBool::new(false)),
            idx_done: Arc::new(AtomicUsize::new(0)),
            idx_total: Arc::new(AtomicUsize::new(0)),
            status: String::new(),
            settings,
        };
        app.refresh(ctx);
        app
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        self.entries = scan(&self.settings);
        let missing: Vec<(PathBuf, Kind)> = self
            .entries
            .iter()
            .filter(|e| !self.thumbs.contains_key(&e.path))
            .map(|e| (e.path.clone(), e.kind))
            .collect();
        spawn_thumbs(missing, self.thumb_tx.clone(), ctx.clone());
        self.status = format!("{} items", self.entries.len());
    }

    fn start_indexing(&mut self, ctx: &egui::Context) {
        let todo: Vec<PathBuf> = {
            let idx = self.index.lock().map(|g| g.clone()).unwrap_or_default();
            self.entries
                .iter()
                .filter(|e| {
                    e.kind == Kind::Image
                        && !idx.contains_key(&e.path.to_string_lossy().to_string())
                })
                .map(|e| e.path.clone())
                .collect()
        };
        if todo.is_empty() {
            return;
        }
        self.idx_total.store(todo.len(), Ordering::SeqCst);
        self.idx_done.store(0, Ordering::SeqCst);
        self.idx_cancel.store(false, Ordering::SeqCst);
        self.idx_running.store(true, Ordering::SeqCst);
        let (index, running, cancel, done) = (
            self.index.clone(),
            self.idx_running.clone(),
            self.idx_cancel.clone(),
            self.idx_done.clone(),
        );
        let ctx2 = ctx.clone();
        thread::spawn(move || {
            for p in todo {
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                let text = match image::open(&p) {
                    Ok(i) => ocr::ocr(&i.to_rgba8()).unwrap_or_default(),
                    Err(_) => String::new(),
                };
                let flat = text.replace(['\n', '\r', '\t'], " ");
                let key = p.to_string_lossy().to_string();
                if let Ok(mut g) = index.lock() {
                    g.insert(key.clone(), flat.clone());
                }
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(index_file())
                {
                    let _ = writeln!(f, "{key}\t{flat}");
                }
                done.fetch_add(1, Ordering::SeqCst);
                ctx2.request_repaint();
            }
            running.store(false, Ordering::SeqCst);
            ctx2.request_repaint();
        });
    }
}

fn open_entry(e: &Entry) {
    match e.kind {
        Kind::Image => util::spawn_worker_path("open", &e.path),
        Kind::Video => {
            if e.path
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x.eq_ignore_ascii_case("gif"))
                .unwrap_or(false)
            {
                util::open_file(&e.path);
            } else {
                util::spawn_worker_path("video", &e.path);
            }
        }
        Kind::Text => util::open_file(&e.path),
    }
}

fn human_size(b: u64) -> String {
    if b >= 1_000_000 {
        format!("{:.1} MB", b as f64 / 1_000_000.0)
    } else {
        format!("{} KB", (b / 1000).max(1))
    }
}

fn when(t: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(t)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

impl eframe::App for HistApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok((p, ci)) = self.thumb_rx.try_recv() {
            let tex = ctx.load_texture(
                format!("t_{}", p.display()),
                ci,
                egui::TextureOptions::LINEAR,
            );
            self.thumbs.insert(p, tex);
        }
        let running = self.idx_running.load(Ordering::SeqCst);
        let mut do_refresh = false;
        let mut do_index = false;

        egui::TopBottomPanel::top("bar")
            .frame(
                egui::Frame::none()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .hint_text("Search by name, date, or text inside screenshots...")
                            .desired_width(380.0),
                    );
                    for (i, label) in ["All", "Screenshots", "Videos", "Text files"]
                        .iter()
                        .enumerate()
                    {
                        ui.selectable_value(&mut self.filter, i as u8, *label);
                    }
                    if ui.button("Refresh").clicked() {
                        do_refresh = true;
                    }
                });
                ui.horizontal(|ui| {
                    if running {
                        let total = self.idx_total.load(Ordering::SeqCst).max(1);
                        let done = self.idx_done.load(Ordering::SeqCst);
                        ui.add(
                            egui::ProgressBar::new(done as f32 / total as f32)
                                .desired_width(260.0)
                                .text(format!("Reading text {done} / {total}")),
                        );
                        if ui.button("Stop").clicked() {
                            self.idx_cancel.store(true, Ordering::SeqCst);
                        }
                    } else {
                        if ui
                            .button("Index text in screenshots")
                            .on_hover_text(
                                "Reads the text in each screenshot so you can search for it",
                            )
                            .clicked()
                        {
                            do_index = true;
                        }
                        ui.label(
                            RichText::new(self.status.as_str())
                                .size(12.0)
                                .color(TEXT_DIM),
                        );
                    }
                });
            });

        // ---- selected item actions
        let mut open: Option<Entry> = None;
        let mut pin: Option<PathBuf> = None;
        let mut copy: Option<PathBuf> = None;
        let mut reveal: Option<PathBuf> = None;
        let mut delete: Option<PathBuf> = None;
        if let Some(sel) = self.selected.clone() {
            if let Some(e) = self.entries.iter().find(|e| e.path == sel).cloned() {
                egui::TopBottomPanel::bottom("detail")
                    .frame(
                        egui::Frame::none()
                            .fill(PANEL)
                            .inner_margin(egui::Margin::symmetric(14.0, 10.0)),
                    )
                    .show(ctx, |ui| {
                        ui.label(RichText::new(e.name.as_str()).strong());
                        ui.label(
                            RichText::new(format!(
                                "{}   {}   {}",
                                when(e.modified),
                                human_size(e.size),
                                e.path.display()
                            ))
                            .size(12.0)
                            .color(TEXT_DIM),
                        );
                        ui.horizontal_wrapped(|ui| {
                            if ui::primary_button(
                                ui,
                                if e.kind == Kind::Text {
                                    "Open"
                                } else {
                                    "Open / edit"
                                },
                            )
                            .clicked()
                            {
                                open = Some(e.clone());
                            }
                            if e.kind == Kind::Image {
                                if ui.button("Pin to screen").clicked() {
                                    pin = Some(e.path.clone());
                                }
                                if ui.button("Copy image").clicked() {
                                    copy = Some(e.path.clone());
                                }
                            }
                            if ui.button("Show in folder").clicked() {
                                reveal = Some(e.path.clone());
                            }
                            if self.confirm_delete.as_ref() == Some(&e.path) {
                                if ui::danger_button(ui, "Really delete?").clicked() {
                                    delete = Some(e.path.clone());
                                }
                                if ui.button("Keep").clicked() {
                                    self.confirm_delete = None;
                                }
                            } else if ui.button("Delete").clicked() {
                                self.confirm_delete = Some(e.path.clone());
                            }
                        });
                    });
            }
        }

        // ---- grid
        let q = self.query.trim().to_lowercase();
        let visible: Vec<usize> = {
            let idx = self.index.lock().map(|g| g.clone()).unwrap_or_default();
            self.entries
                .iter()
                .enumerate()
                .filter(|(_, e)| match self.filter {
                    1 => e.kind == Kind::Image,
                    2 => e.kind == Kind::Video,
                    3 => e.kind == Kind::Text,
                    _ => true,
                })
                .filter(|(_, e)| {
                    q.is_empty()
                        || e.name.to_lowercase().contains(&q)
                        || idx
                            .get(&e.path.to_string_lossy().to_string())
                            .map(|t| t.to_lowercase().contains(&q))
                            .unwrap_or(false)
                })
                .map(|(i, _)| i)
                .collect()
        };
        let mut clicked: Option<PathBuf> = None;
        let mut dbl: Option<Entry> = None;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(BG)
                    .inner_margin(egui::Margin::same(14.0)),
            )
            .show(ctx, |ui| {
                if visible.is_empty() {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("Nothing found").size(18.0).color(TEXT_DIM))
                    });
                    return;
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
                            for &i in &visible {
                                let e = &self.entries[i];
                                let (rect, resp) =
                                    ui.allocate_exact_size(vec2(220.0, 178.0), Sense::click());
                                let p = ui.painter_at(rect);
                                let is_sel = self.selected.as_ref() == Some(&e.path);
                                p.rect_filled(rect, 10.0, CARD);
                                p.rect_stroke(
                                    rect,
                                    10.0,
                                    Stroke::new(
                                        if is_sel { 2.0 } else { 1.0 },
                                        if is_sel { ACCENT } else { BORDER },
                                    ),
                                );
                                let area = Rect::from_min_size(
                                    rect.min + vec2(0.0, 0.0),
                                    vec2(220.0, 126.0),
                                );
                                if let Some(tex) = self.thumbs.get(&e.path) {
                                    let s = tex.size_vec2();
                                    let k = (216.0 / s.x).min(122.0 / s.y).min(1.0);
                                    p.image(
                                        tex.id(),
                                        Rect::from_center_size(area.center(), s * k),
                                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                        Color32::WHITE,
                                    );
                                } else {
                                    let label = match e.kind {
                                        Kind::Image => "IMAGE",
                                        Kind::Video => "VIDEO",
                                        Kind::Text => "TEXT",
                                    };
                                    p.text(
                                        area.center(),
                                        Align2::CENTER_CENTER,
                                        label,
                                        FontId::proportional(16.0),
                                        TEXT_DIM,
                                    );
                                }
                                let mut name = e.name.clone();
                                if name.chars().count() > 30 {
                                    name =
                                        format!("{}...", name.chars().take(27).collect::<String>());
                                }
                                p.text(
                                    pos2(rect.min.x + 10.0, rect.min.y + 142.0),
                                    Align2::LEFT_CENTER,
                                    name,
                                    FontId::proportional(13.0),
                                    Color32::from_rgb(28, 32, 41),
                                );
                                p.text(
                                    pos2(rect.min.x + 10.0, rect.min.y + 162.0),
                                    Align2::LEFT_CENTER,
                                    when(e.modified),
                                    FontId::proportional(11.0),
                                    TEXT_DIM,
                                );
                                if resp.clicked() {
                                    clicked = Some(e.path.clone());
                                }
                                if resp.double_clicked() {
                                    dbl = Some(e.clone());
                                }
                            }
                        });
                    });
            });

        // ---- apply actions
        if let Some(p) = clicked {
            if self.selected.as_ref() != Some(&p) {
                self.confirm_delete = None;
            }
            self.selected = Some(p);
        }
        if let Some(e) = dbl {
            open_entry(&e);
        }
        if let Some(e) = open {
            open_entry(&e);
        }
        if let Some(p) = pin {
            util::spawn_worker_path("pin", &p);
        }
        if let Some(p) = copy {
            if let Ok(i) = image::open(&p) {
                let _ = util::copy_image(&i.to_rgba8());
                self.status = "Copied to clipboard".to_string();
            }
        }
        if let Some(p) = reveal {
            util::reveal_in_explorer(&p);
        }
        if let Some(p) = delete {
            let _ = std::fs::remove_file(&p);
            self.entries.retain(|e| e.path != p);
            self.thumbs.remove(&p);
            self.selected = None;
            self.confirm_delete = None;
        }
        if do_refresh {
            self.refresh(ctx);
        }
        if do_index {
            self.start_indexing(ctx);
        }
        let _ = DANGER;
    }
}
