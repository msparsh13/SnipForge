//! Settings shared by the launcher and the capture workers (simple key=value file).
use std::path::{Path, PathBuf};

#[derive(Clone, PartialEq)]
pub struct Settings {
    pub delay: u32,
    pub fps: u32,
    pub rec_countdown: u32,
    pub mic: bool,
    pub system_audio: bool,
    pub auto_copy: bool,
    pub all_screens: bool,
    pub save_ocr_text: bool,
    pub cap_target: String, // "area", "full" or "m1", "m2"...
    pub rec_target: String,
    // custom folders; empty string = use the default
    pub shots_dir: String,
    pub videos_dir: String,
    pub text_dir: String,
    pub engine_dir: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            delay: 0,
            fps: 30,
            rec_countdown: 3,
            mic: true,
            system_audio: false,
            auto_copy: true,
            all_screens: true,
            save_ocr_text: false,
            cap_target: "area".to_string(),
            rec_target: "area".to_string(),
            shots_dir: String::new(),
            videos_dir: String::new(),
            text_dir: String::new(),
            engine_dir: String::new(),
        }
    }
}

fn config_file() -> PathBuf {
    let d = dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("SnipForge");
    let _ = std::fs::create_dir_all(&d);
    d.join("settings.txt")
}

pub fn default_shots_dir() -> PathBuf {
    dirs::picture_dir().unwrap_or_else(|| PathBuf::from(".")).join("SnipForge")
}

pub fn default_videos_dir() -> PathBuf {
    dirs::video_dir()
        .or_else(dirs::picture_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SnipForge")
}

pub fn default_text_dir() -> PathBuf {
    dirs::document_dir().unwrap_or_else(|| PathBuf::from(".")).join("SnipForge Text")
}

pub fn default_engine_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SnipForge")
        .join("bin")
}

/// Use the custom folder if set and usable, otherwise the default. The folder is created.
fn pick_dir(custom: &str, default: PathBuf) -> PathBuf {
    if !custom.trim().is_empty() {
        let p = Path::new(custom.trim()).to_path_buf();
        if std::fs::create_dir_all(&p).is_ok() {
            return p;
        }
    }
    let _ = std::fs::create_dir_all(&default);
    default
}

impl Settings {
    pub fn load() -> Settings {
        let mut s = Settings::default();
        if let Ok(text) = std::fs::read_to_string(config_file()) {
            for line in text.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    let v = v.trim();
                    match k.trim() {
                        "delay" => s.delay = v.parse().unwrap_or(s.delay),
                        "fps" => s.fps = v.parse().unwrap_or(s.fps),
                        "rec_countdown" => s.rec_countdown = v.parse().unwrap_or(s.rec_countdown),
                        "mic" => s.mic = v == "1",
                        "system_audio" => s.system_audio = v == "1",
                        "auto_copy" => s.auto_copy = v == "1",
                        "all_screens" => s.all_screens = v == "1",
                        "save_ocr_text" => s.save_ocr_text = v == "1",
                        "cap_target" => s.cap_target = v.to_string(),
                        "rec_target" => s.rec_target = v.to_string(),
                        "shots_dir" => s.shots_dir = v.to_string(),
                        "videos_dir" => s.videos_dir = v.to_string(),
                        "text_dir" => s.text_dir = v.to_string(),
                        "engine_dir" => s.engine_dir = v.to_string(),
                        _ => {}
                    }
                }
            }
        }
        s
    }

    pub fn save(&self) {
        let b = |x: bool| if x { 1 } else { 0 };
        let text = format!(
            "delay={}\nfps={}\nrec_countdown={}\nmic={}\nsystem_audio={}\nauto_copy={}\nall_screens={}\nsave_ocr_text={}\ncap_target={}\nrec_target={}\nshots_dir={}\nvideos_dir={}\ntext_dir={}\nengine_dir={}\n",
            self.delay,
            self.fps,
            self.rec_countdown,
            b(self.mic),
            b(self.system_audio),
            b(self.auto_copy),
            b(self.all_screens),
            b(self.save_ocr_text),
            self.cap_target,
            self.rec_target,
            self.shots_dir,
            self.videos_dir,
            self.text_dir,
            self.engine_dir
        );
        let _ = std::fs::write(config_file(), text);
    }

    pub fn shots_path(&self) -> PathBuf {
        pick_dir(&self.shots_dir, default_shots_dir())
    }
    pub fn videos_path(&self) -> PathBuf {
        pick_dir(&self.videos_dir, default_videos_dir())
    }
    pub fn text_path(&self) -> PathBuf {
        pick_dir(&self.text_dir, default_text_dir())
    }
    pub fn engine_path(&self) -> PathBuf {
        pick_dir(&self.engine_dir, default_engine_dir())
    }
}
