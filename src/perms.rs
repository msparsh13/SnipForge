//! Windows privacy switches that block audio. An app cannot grant itself microphone access;
//! the person has to turn it on in Windows settings, so we detect it and send them there.
use std::process::Command;

use crate::util::no_window;

pub enum MicAccess {
    Allowed,
    Blocked(String), // plain-language reason
}

fn reg_value(key: &str) -> Option<String> {
    let mut c = Command::new("reg");
    c.args(["query", key, "/v", "Value"]);
    no_window(&mut c);
    let out = c.output().ok()?;
    if !out.status.success() {
        return None;ś
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    for line in text.lines() {
        if line.contains("REG_SZ") {
            return line.split_whitespace().last().map(|s| s.to_string());
        }
    }
    None
}

fn denied(v: Option<String>) -> bool {
    v.map(|s| s.eq_ignore_ascii_case("deny")).unwrap_or(false)
}

pub fn mic_access() -> MicAccess {
    let base = r"HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";
    if denied(reg_value(base)) {
        return MicAccess::Blocked("Microphone access is switched off for this whole PC.".to_string());
    }
    if denied(reg_value(&format!("{base}\\NonPackaged"))) {
        return MicAccess::Blocked("Windows is not letting desktop apps use the microphone.".to_string());
    }
    MicAccess::Allowed
}

pub fn open_mic_settings() {
    let mut c = Command::new("cmd");
    c.args(["/C", "start", "", "ms-settings:privacy-microphone"]);
    no_window(&mut c);
    let _ = c.spawn();
}

pub fn open_sound_settings() {
    let mut c = Command::new("cmd");
    c.args(["/C", "start", "", "ms-settings:sound"]);
    no_window(&mut c);
    let _ = c.spawn();
}
