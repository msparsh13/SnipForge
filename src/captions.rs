//! Captions: SRT read/write, plus re-timing after cuts / speed changes (pure logic).
use std::cmp::Ordering;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct Caption {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// "m:ss.cc" for the user interface.
pub fn fmt_time(t: f64) -> String {
    let t = t.max(0.0);
    let m = (t / 60.0) as u64;
    format!("{}:{:05.2}", m, t - m as f64 * 60.0)
}

fn srt_time(t: f64) -> String {
    let ms = (t.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        ms % 1000
    )
}

fn parse_srt_time(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    let mut total = 0.0;
    for part in s.split(':') {
        total = total * 60.0 + part.trim().parse::<f64>().ok()?;
    }
    Some(total)
}

pub fn write_srt(caps: &[Caption], path: &Path) -> std::io::Result<()> {
    let mut out = String::new();
    for (i, c) in caps.iter().enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            srt_time(c.start),
            srt_time(c.end),
            c.text.trim()
        ));
    }
    std::fs::write(path, out)
}

pub fn read_srt(path: &Path) -> Result<Vec<Caption>, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let text = raw.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut caps = Vec::new();
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block.lines().collect();
        if let Some(k) = lines.iter().position(|l| l.contains("-->")) {
            let (a, b) = lines[k].split_once("-->").unwrap_or(("", ""));
            if let (Some(start), Some(end)) = (parse_srt_time(a), parse_srt_time(b)) {
                caps.push(Caption {
                    start,
                    end,
                    text: lines[k + 1..].join("\n").trim().to_string(),
                });
            }
        }
    }
    Ok(caps)
}

/// Re-time captions for the edited video (cuts + speed). `keeps` are source ranges in seconds.
pub fn map_captions(caps: &[Caption], keeps: &[(f64, f64)], speed: f64) -> Vec<Caption> {
    let mut out = Vec::new();
    let mut offset = 0.0;
    for &(ks, ke) in keeps {
        for c in caps {
            let s = c.start.max(ks);
            let e = c.end.min(ke);
            if e - s > 0.05 {
                out.push(Caption {
                    start: (offset + s - ks) / speed,
                    end: (offset + e - ks) / speed,
                    text: c.text.clone(),
                });
            }
        }
        offset += ke - ks;
    }
    out.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(Ordering::Equal));
    out
}

/// Remove [a, b] from the kept ranges.
pub fn keeps_subtract(keeps: &[(f64, f64)], a: f64, b: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for &(s, e) in keeps {
        if b <= s || a >= e {
            out.push((s, e));
            continue;
        }
        if s < a {
            out.push((s, a));
        }
        if b < e {
            out.push((b, e));
        }
    }
    out
}

/// Keep only the part of the kept ranges that lies inside [a, b].
pub fn keeps_intersect(keeps: &[(f64, f64)], a: f64, b: f64) -> Vec<(f64, f64)> {
    keeps
        .iter()
        .map(|&(s, e)| (s.max(a), e.min(b)))
        .filter(|&(s, e)| e > s)
        .collect()
}
