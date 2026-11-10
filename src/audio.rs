//! Audio capture with cpal (WASAPI): microphone, or "device audio" (loopback of whatever
//! the default speakers/headphones are playing). Samples are delivered as raw f32 little-endian bytes.
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub struct AudioCapture {
    stream: Option<cpal::Stream>,
    pub rx: mpsc::Receiver<Vec<u8>>,
    pub rate: u32,
    pub channels: u16,
}

impl AudioCapture {
    /// Stop the device (the already-delivered samples stay readable from `rx`).
    pub fn stop(&mut self) {
        self.stream = None;
    }
}

fn friendly(e: impl std::fmt::Display, loopback: bool) -> String {
    if loopback {
        format!("{e}. Check that a playback device is selected and switched on in Windows sound settings.")
    } else {
        format!("{e}. Check Windows Settings > Privacy & security > Microphone, and that no other app has exclusive control of the microphone.")
    }
}

/// `loopback = true` records what the computer is playing; `false` records the default microphone.
/// While `paused` is set, incoming samples are dropped. While `enabled` is false they are replaced by silence
/// (so muting never shifts the timeline). `level` always shows the loudest recent sample (0..1000).
pub fn start_capture(
    loopback: bool,
    paused: Arc<AtomicBool>,
    enabled: Arc<AtomicBool>,
    level: Arc<AtomicU32>,
) -> Result<AudioCapture, String> {
    let host = cpal::default_host();
    let (device, cfg) = if loopback {
        let d = host
            .default_output_device()
            .ok_or("No playback device found. Plug in or enable speakers / headphones.")?;
        let c = d.default_output_config().map_err(|e| friendly(e, true))?;
        (d, c)
    } else {
        let d = host
            .default_input_device()
            .ok_or("No microphone found. Plug one in or enable it in Windows sound settings.")?;
        let c = d.default_input_config().map_err(|e| friendly(e, false))?;
        (d, c)
    };
    let fmt = cfg.sample_format();
    let rate = cfg.sample_rate().0;
    let channels = cfg.channels();
    let sample_format = cfg.sample_format();

    let channels = if loopback { 2 } else { cfg.channels() };

    let mut config: cpal::StreamConfig = cfg.into();
    config.channels = channels;

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let push = move |samples: Vec<f32>| {
        let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
        level.store((peak.min(1.0) * 1000.0) as u32, Ordering::Relaxed);
        if paused.load(Ordering::Relaxed) {
            return;
        }
        let on = enabled.load(Ordering::Relaxed);
        let mut bytes = Vec::with_capacity(samples.len() * 4);
        for s in samples {
            let v = if on { s } else { 0.0 };
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let _ = tx.send(bytes);
    };
    let err_fn = |e: cpal::StreamError| eprintln!("audio stream error: {e}");

    let built = match fmt {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config,
            move |d: &[f32], _: &cpal::InputCallbackInfo| push(d.to_vec()),
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config,
            move |d: &[i16], _: &cpal::InputCallbackInfo| {
                push(d.iter().map(|&s| s as f32 / 32768.0).collect())
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config,
            move |d: &[u16], _: &cpal::InputCallbackInfo| {
                push(d.iter().map(|&s| (s as f32 - 32768.0) / 32768.0).collect())
            },
            err_fn,
            None,
        ),
        other => return Err(format!("unsupported audio format {other:?}")),
    };
    let stream = built.map_err(|e| friendly(e, loopback))?;
    stream.play().map_err(|e| friendly(e, loopback))?;
    Ok(AudioCapture {
        stream: Some(stream),
        rx,
        rate,
        channels,
    })
}

/// Listen for a few seconds and report the loudest sound heard (0.0 .. 1.0). Used by the "Test" buttons.
pub fn test_level(loopback: bool, secs: f32) -> Result<f32, String> {
    let level = Arc::new(AtomicU32::new(0));
    let cap = start_capture(
        loopback,
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(true)),
        level.clone(),
    )?;
    let start = Instant::now();
    let mut peak = 0u32;
    while start.elapsed().as_secs_f32() < secs {
        thread::sleep(Duration::from_millis(30));
        peak = peak.max(level.load(Ordering::Relaxed));
    }
    drop(cap);
    Ok(peak as f32 / 1000.0)
}
