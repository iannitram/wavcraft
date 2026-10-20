//! The real input device for recording: the system default input, opened for a take.
//!
//! The callback converts to `f32` in small stack chunks and calls `Ring::push`; it never blocks,
//! locks or allocates. The stream is built here but not started: the caller starts the writer
//! first, then calls `play()`.

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait};
use wavcraft_core::record::{ChannelPick, RecFormat, Ring};

pub struct Opened {
    pub stream: cpal::Stream,
    pub ring: Arc<Ring>,
    /// What the device actually gives: the request when the device can do it, else its default.
    pub fmt: RecFormat,
    pub device: String,
}

/// Open the default input device for `want`. When the device can't do that rate and channel
/// count, its own default is used instead (the dialog's choice is a request, not a promise). A
/// choice of particular inputs (C034) is not bent: the device must have them, or it is refused.
pub fn open(want: RecFormat) -> Result<Opened, String> {
    let device = cpal::default_host().default_input_device().ok_or("no audio input device")?;
    let name = device.name().unwrap_or_else(|_| "input".to_string());
    let chosen = want.inputs != 0;
    let need = want.pick().needs_device_channels() as u16;
    let wanted = device.supported_input_configs().ok().and_then(|mut it| {
        it.find(|c| {
            (if chosen { c.channels() >= need } else { c.channels() == want.channels })
                && (c.min_sample_rate().0..=c.max_sample_rate().0).contains(&want.rate)
        })
        .map(|c| c.with_sample_rate(cpal::SampleRate(want.rate)))
    });
    let supported = match wanted {
        Some(c) => c,
        None if chosen => return Err(format!("{name} has no {} Hz format with inputs up to {need}", want.rate)),
        None => device.default_input_config().map_err(|e| format!("the input device has no usable format: {e}"))?,
    };
    let dev_channels = supported.channels();
    let fmt = if chosen {
        RecFormat { rate: supported.sample_rate().0, ..want }.validate()?
    } else {
        RecFormat { rate: supported.sample_rate().0, channels: dev_channels, bits: want.bits, inputs: 0 }.validate()?
    };
    let config: cpal::StreamConfig = supported.clone().into();
    let ring = Ring::new(fmt.rate as usize * 4, fmt.channels);
    let pick = fmt.pick();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, &ring, pick),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, &ring, pick),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, &ring, pick),
        cpal::SampleFormat::I32 => build::<i32>(&device, &config, &ring, pick),
        other => Err(format!("unsupported input sample format {other:?}")),
    }?;
    Ok(Opened { stream, ring, fmt, device: name })
}

fn build<T>(device: &cpal::Device, config: &cpal::StreamConfig, ring: &Arc<Ring>, pick: ChannelPick) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let ring = Arc::clone(ring);
    let dev = usize::from(config.channels.max(1));
    // Whole device frames per pass, so a pass fits the stack buffers (which hold 2048 samples).
    let step = (2048 / dev).max(1) * dev;
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let mut conv = [0.0f32; 2048];
                let mut out = [0.0f32; 2048];
                for part in data.chunks(step) {
                    for (c, s) in conv.iter_mut().zip(part) {
                        *c = cpal::Sample::to_sample::<f32>(*s);
                    }
                    let frames = pick.gather(&conv[..part.len()], dev, &mut out);
                    ring.push(&out[..frames * pick.indices().len()]);
                }
            },
            |err| log::warn!(target: "wavcraft", "audio input: {err}"),
            None,
        )
        .map_err(|e| e.to_string())
}
