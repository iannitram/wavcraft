//! The real input device for recording: the input device the preferences name (`input_device`),
//! else the system default, opened for a take.
//!
//! The callback converts to `f32` in small stack chunks and calls `Ring::push`; it never blocks,
//! locks or allocates. The stream is built here but not started: the caller starts the writer
//! first, then calls `play()`.

use std::sync::{Arc, OnceLock};

use cpal::traits::DeviceTrait;
use wavcraft_core::record::{ChannelPick, MonitorRing, RecFormat, Ring};

/// Why the input stopped delivering audio. The stream's error callback only stores it here
/// (once), so it never waits; the take checks it once a frame.
pub type Lost = OnceLock<&'static str>;

pub struct Opened {
    pub stream: cpal::Stream,
    pub ring: Arc<Ring>,
    /// What the device actually gives: the request when the device can do it, else its default.
    pub fmt: RecFormat,
    pub device: String,
    /// The software monitoring queue, when asked for (C025).
    pub monitor: Option<Arc<MonitorRing>>,
    /// Set when the device is unplugged or the stream is invalidated.
    pub lost: Arc<Lost>,
}

/// Open the input named `wanted` (else the system default) for `want`. When the device can't do
/// that rate and channel count, its own default is used instead (the dialog's choice is a request,
/// not a promise). A choice of particular inputs (C034, f77) is not bent: the device must have
/// them, or it is refused.
pub fn open(want: RecFormat, monitor: bool, wanted: Option<&str>) -> Result<Opened, String> {
    let device = crate::input_monitor::find_input(&cpal::default_host(), wanted).ok_or("no audio input device")?;
    let name = crate::player::device_name(&device).unwrap_or_else(|| "input".to_string());
    let chosen = want.inputs != 0;
    let need = want.pick().needs_device_channels() as u16;
    let wanted = device.supported_input_configs().ok().and_then(|mut it| {
        it.find(|c| {
            (if chosen { c.channels() >= need } else { c.channels() == want.channels })
                && (c.min_sample_rate()..=c.max_sample_rate()).contains(&want.rate)
        })
        .map(|c| c.with_sample_rate(want.rate))
    });
    let supported = match wanted {
        Some(c) => c,
        None if chosen => return Err(format!("{name} has no {} Hz format with inputs up to {need}", want.rate)),
        None => device.default_input_config().map_err(|e| format!("the input device has no usable format: {e}"))?,
    };
    let fmt = if chosen {
        RecFormat { rate: supported.sample_rate(), ..want }.validate()?
    } else {
        RecFormat { rate: supported.sample_rate(), channels: supported.channels(), bits: want.bits, inputs: 0 }.validate()?
    };
    let config: cpal::StreamConfig = supported.into();
    let monitor = monitor.then(|| MonitorRing::new(monitor_frames(fmt.rate), fmt.channels));
    let ring = Ring::with_monitor(fmt.rate as usize * 4, fmt.channels, monitor.clone());
    let pick = fmt.pick();
    let lost = Arc::new(Lost::new());
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, &ring, pick, &lost),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, &ring, pick, &lost),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, &ring, pick, &lost),
        cpal::SampleFormat::I32 => build::<i32>(&device, &config, &ring, pick, &lost),
        other => Err(format!("unsupported input sample format {other:?}")),
    }?;
    Ok(Opened { stream, ring, fmt, device: name, monitor, lost })
}

/// The monitoring queue holds a quarter of a second: the most delay it can add.
pub fn monitor_frames(rate: u32) -> usize {
    rate as usize / 4
}

fn build<T>(device: &cpal::Device, config: &cpal::StreamConfig, ring: &Arc<Ring>, pick: ChannelPick, lost: &Arc<Lost>) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let ring = Arc::clone(ring);
    let lost = Arc::clone(lost);
    let dev = usize::from(config.channels.max(1));
    // Whole device frames per pass, so a pass fits the stack buffers (which hold 2048 samples).
    let step = (2048 / dev).max(1) * dev;
    device
        .build_input_stream(
            *config,
            move |data: &[T], info: &cpal::InputCallbackInfo| {
                // Capture to callback is the delay the driver reports for this buffer (C010).
                let ts = info.timestamp();
                if let Some(d) = ts.callback.checked_duration_since(ts.capture) {
                    ring.set_reported_latency_us(d.as_micros().min(u128::from(u64::MAX)) as u64);
                }
                let mut conv = [0.0f32; 2048];
                let mut out = [0.0f32; 2048];
                for part in data.chunks(step) {
                    for (c, s) in conv.iter_mut().zip(part) {
                        *c = cpal::Sample::to_sample::<f32>(*s);
                    }
                    // Only the chosen inputs (f77), in take order.
                    let frames = pick.gather(&conv[..part.len()], dev, &mut out);
                    ring.push(&out[..frames * pick.indices().len()]);
                }
            },
            move |err: cpal::Error| {
                log::warn!(target: "wavcraft", "audio input: {err}");
                // A stream on a named device reports DeviceNotAvailable when it is unplugged; a
                // glitch (Xrun) does not end the take.
                let why = match err.kind() {
                    cpal::ErrorKind::DeviceNotAvailable => "input device unplugged",
                    cpal::ErrorKind::StreamInvalidated => "input device stopped",
                    _ => return,
                };
                let _ = lost.set(why);
            },
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_named_input_that_is_gone_falls_back_to_the_default() {
        let names: Vec<String> = ["Built-in", "USB Mic"].iter().map(|s| s.to_string()).collect();
        assert_eq!(crate::player::pick(Some("USB Mic"), &names), Some(1));
        assert_eq!(crate::player::pick(Some("Unplugged Mic"), &names), None, "the default input is used");
    }
}
