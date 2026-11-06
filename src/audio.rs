//! Audio capture with cpal (WASAPI): microphone, or "device audio" (loopback of whatever
//! the default speakers/headphones are playing). Samples are delivered as raw f32 little-endian bytes.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

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

/// `loopback = true` records what the computer is playing; `false` records the default microphone.
/// While `paused` is set, incoming samples are dropped. While `enabled` is false they are replaced by silence
/// (so muting never shifts the timeline).
pub fn start_capture(loopback: bool, paused: Arc<AtomicBool>, enabled: Arc<AtomicBool>) -> Result<AudioCapture, String> {
    let host = cpal::default_host();
    let (device, cfg) = if loopback {
        let d = host.default_output_device().ok_or("No audio output device found")?;
        let c = d.default_output_config().map_err(|e| e.to_string())?;
        (d, c)
    } else {
        let d = host.default_input_device().ok_or("No microphone found")?;
        let c = d.default_input_config().map_err(|e| e.to_string())?;
        (d, c)
    };
    let fmt = cfg.sample_format();
    let rate = cfg.sample_rate().0;
    let channels = cfg.channels();
    let config: cpal::StreamConfig = cfg.into();

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let push = move |samples: Vec<f32>| {
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
            move |d: &[i16], _: &cpal::InputCallbackInfo| push(d.iter().map(|&s| s as f32 / 32768.0).collect()),
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config,
            move |d: &[u16], _: &cpal::InputCallbackInfo| push(d.iter().map(|&s| (s as f32 - 32768.0) / 32768.0).collect()),
            err_fn,
            None,
        ),
        other => return Err(format!("unsupported audio format {other:?}")),
    };
    let stream = built.map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(AudioCapture { stream: Some(stream), rx, rate, channels })
}
