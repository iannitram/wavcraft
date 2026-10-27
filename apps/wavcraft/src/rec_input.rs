//! The real input device for recording: the system default input, opened for a take.
//!
//! The callback converts to `f32` in small stack chunks and calls `Ring::push`; it never blocks,
//! locks or allocates. The stream is built here but not started: the caller starts the writer
//! first, then calls `play()`.

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait};
use wavcraft_core::record::{MonitorRing, RecFormat, Ring};

pub struct Opened {
    pub stream: cpal::Stream,
    pub ring: Arc<Ring>,
    /// What the device actually gives: the request when the device can do it, else its default.
    pub fmt: RecFormat,
    pub device: String,
    /// The software monitoring queue, when asked for (C025).
    pub monitor: Option<Arc<MonitorRing>>,
}

/// Open the default input device for `want`. When the device can't do that rate and channel
/// count, its own default is used instead (the dialog's choice is a request, not a promise).
pub fn open(want: RecFormat, monitor: bool) -> Result<Opened, String> {
    let device = cpal::default_host().default_input_device().ok_or("no audio input device")?;
    let name = device.name().unwrap_or_else(|_| "input".to_string());
    let wanted = device.supported_input_configs().ok().and_then(|mut it| {
        it.find(|c| c.channels() == want.channels && (c.min_sample_rate().0..=c.max_sample_rate().0).contains(&want.rate))
            .map(|c| c.with_sample_rate(cpal::SampleRate(want.rate)))
    });
    let supported = match wanted {
        Some(c) => c,
        None => device.default_input_config().map_err(|e| format!("the input device has no usable format: {e}"))?,
    };
    let fmt = RecFormat { rate: supported.sample_rate().0, channels: supported.channels(), bits: want.bits }.validate()?;
    let config: cpal::StreamConfig = supported.clone().into();
    let monitor = monitor.then(|| MonitorRing::new(monitor_frames(fmt.rate), fmt.channels));
    let ring = Ring::with_monitor(fmt.rate as usize * 4, fmt.channels, monitor.clone());
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, &ring),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, &ring),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, &ring),
        cpal::SampleFormat::I32 => build::<i32>(&device, &config, &ring),
        other => Err(format!("unsupported input sample format {other:?}")),
    }?;
    Ok(Opened { stream, ring, fmt, device: name, monitor })
}

/// The monitoring queue holds a quarter of a second: the most delay it can add.
pub fn monitor_frames(rate: u32) -> usize {
    rate as usize / 4
}

fn build<T>(device: &cpal::Device, config: &cpal::StreamConfig, ring: &Arc<Ring>) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let ring = Arc::clone(ring);
    let step = 256 * usize::from(config.channels.max(1));
    device
        .build_input_stream(
            config,
            move |data: &[T], info: &cpal::InputCallbackInfo| {
                // Capture to callback is the delay the driver reports for this buffer (C010).
                let ts = info.timestamp();
                if let Some(d) = ts.callback.duration_since(&ts.capture) {
                    ring.set_reported_latency_us(d.as_micros().min(u128::from(u64::MAX)) as u64);
                }
                let mut chunk = [0.0f32; 2048];
                for part in data.chunks(step.min(chunk.len())) {
                    for (out, s) in chunk.iter_mut().zip(part) {
                        *out = cpal::Sample::to_sample::<f32>(*s);
                    }
                    ring.push(&chunk[..part.len()]);
                }
            },
            |err| log::warn!(target: "wavcraft", "audio input: {err}"),
            None,
        )
        .map_err(|e| e.to_string())
}
