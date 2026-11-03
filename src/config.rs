//! Settings shared by the launcher and the capture workers (simple key=value file).
use std::path::PathBuf;

#[derive(Clone, PartialEq)]
pub struct Settings {
    pub delay: u32,
    pub fps: u32,
    pub mic: bool,
    pub auto_copy: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { delay: 0, fps: 30, mic: true, auto_copy: true }
    }
}

fn path() -> PathBuf {
    let d = dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("SnipForge");
    let _ = std::fs::create_dir_all(&d);
    d.join("settings.txt")
}

impl Settings {
    pub fn load() -> Settings {
        let mut s = Settings::default();
        if let Ok(text) = std::fs::read_to_string(path()) {
            for line in text.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    let v = v.trim();
                    match k.trim() {
                        "delay" => s.delay = v.parse().unwrap_or(s.delay),
                        "fps" => s.fps = v.parse().unwrap_or(s.fps),
                        "mic" => s.mic = v == "1",
                        "auto_copy" => s.auto_copy = v == "1",
                        _ => {}
                    }
                }
            }
        }
        s
    }

    pub fn save(&self) {
        let text = format!(
            "delay={}\nfps={}\nmic={}\nauto_copy={}\n",
            self.delay,
            self.fps,
            if self.mic { 1 } else { 0 },
            if self.auto_copy { 1 } else { 0 }
        );
        let _ = std::fs::write(path(), text);
    }
}
