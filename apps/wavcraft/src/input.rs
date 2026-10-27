//! The input monitor (f56): an input stream opened only while the owner has asked to meter the
//! input (Window › Meter Input Signal, or a mic check), feeding [`InputTap`] and nothing else.
//! Nothing is recorded.
//!
//! The callback never blocks or allocates: it hands the samples to `InputTap::feed`, a few atomic
//! operations. With `WAVCRAFT_FAKE_INPUT` set (`sine:440:-12`, `silence`) a thread plays the part
//! of the device instead and no real device is ever opened, so tests and scripted runs neither
//! need a microphone nor trigger a permission prompt.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use wavcraft_core::input_meter::{FAKE_RATE, FakeInput, InputTap};

enum Running {
    Stream(#[allow(dead_code)] cpal::Stream),
    Fake(Arc<AtomicBool>),
}

pub struct InputMonitor {
    pub tap: Arc<InputTap>,
    running: Option<Running>,
    /// What the open input is called (the device, or the fake's spec), for the panel.
    pub source: String,
    /// The device name the stream was opened for (`None`: the system default).
    opened_for: Option<Option<String>>,
    /// Why the input could not be opened.
    pub error: Option<String>,
    /// The fake input spec from the environment, if any.
    fake: Option<Result<FakeInput, String>>,
    fake_name: String,
}

impl InputMonitor {
    pub fn new() -> InputMonitor {
        InputMonitor::with_fake(std::env::var("WAVCRAFT_FAKE_INPUT").ok().as_deref())
    }

    /// A monitor on the synthetic input `spec` (`None` or blank: the real device).
    pub fn with_fake(spec: Option<&str>) -> InputMonitor {
        let fake = spec.filter(|s| !s.trim().is_empty()).map(FakeInput::parse);
        let source_name = spec.unwrap_or_default().trim().to_string();
        InputMonitor {
            fake_name: source_name,
            tap: Arc::new(InputTap::default()),
            running: None,
            source: String::new(),
            opened_for: None,
            error: None,
            fake,
        }
    }

    pub fn is_open(&self) -> bool {
        self.running.is_some()
    }

    /// Whether the input is a synthetic one (so no device is ever touched).
    #[cfg(test)]
    pub fn is_fake(&self) -> bool {
        self.fake.is_some()
    }

    /// Whether the open stream is for a different device than `wanted` (the preference changed).
    pub fn stale(&self, wanted: Option<&str>) -> bool {
        self.running.is_some() && self.fake.is_none() && self.opened_for.as_ref().is_some_and(|o| o.as_deref() != wanted)
    }

    /// Open the input (`wanted`: the device name, `None` for the system default). Does nothing
    /// when already open.
    pub fn start(&mut self, wanted: Option<&str>) {
        if self.running.is_some() {
            return;
        }
        self.error = None;
        match &self.fake {
            Some(Ok(spec)) => {
                let stop = Arc::new(AtomicBool::new(false));
                let (tap, flag, mut fake) = (self.tap.clone(), stop.clone(), *spec);
                let spawned = std::thread::Builder::new().name("fake input".into()).spawn(move || {
                    // 10 ms blocks at the pace of a real device.
                    let block = (FAKE_RATE / 100) as usize;
                    while !flag.load(Ordering::Relaxed) {
                        fake.feed(&tap, block);
                        std::thread::sleep(Duration::from_millis(10));
                    }
                });
                match spawned {
                    Ok(_) => {
                        self.running = Some(Running::Fake(stop));
                        self.source = format!("fake input {}", self.fake_name);
                    }
                    Err(e) => self.error = Some(format!("fake input: {e}")),
                }
            }
            Some(Err(e)) => self.error = Some(e.clone()),
            None => match open_device(&self.tap, wanted) {
                Ok((stream, name)) => {
                    self.running = Some(Running::Stream(stream));
                    self.source = name;
                    self.opened_for = Some(wanted.map(str::to_string));
                }
                Err(e) => {
                    log::warn!(target: "wavcraft", "input: {e}");
                    self.error = Some(e);
                }
            },
        }
    }

    /// Close the input.
    pub fn stop(&mut self) {
        if let Some(Running::Fake(flag)) = &self.running {
            flag.store(true, Ordering::Relaxed);
        }
        self.running = None;
        self.opened_for = None;
        // Peaks left from before must not light the bars when the input opens again.
        let _ = self.tap.take_peaks();
    }
}

fn open_device(tap: &Arc<InputTap>, wanted: Option<&str>) -> Result<(cpal::Stream, String), String> {
    let host = cpal::default_host();
    let named = wanted.and_then(|w| {
        let found = host.input_devices().ok()?.find(|d| d.name().is_ok_and(|n| n == w));
        if found.is_none() {
            log::warn!(target: "wavcraft", "audio input {w:?} is not available; using the system default");
        }
        found
    });
    let device = match named {
        Some(d) => d,
        None => host.default_input_device().ok_or("no audio input device")?,
    };
    let name = device.name().unwrap_or_else(|_| "input".to_string());
    let supported = device.default_input_config().map_err(|e| e.to_string())?;
    let config: cpal::StreamConfig = supported.clone().into();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, tap),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, tap),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, tap),
        cpal::SampleFormat::I32 => build::<i32>(&device, &config, tap),
        other => Err(format!("unsupported input sample format {other:?}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, name))
}

fn build<T>(device: &cpal::Device, config: &cpal::StreamConfig, tap: &Arc<InputTap>) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let tap = Arc::clone(tap);
    let channels = usize::from(config.channels).max(1);
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| tap.feed(data.iter().map(|s| cpal::Sample::to_sample::<f32>(*s)), channels),
            |err| log::warn!(target: "wavcraft", "audio input: {err}"),
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake input fills the tap like a device would, stops when closed, and never touches a device.
    #[test]
    fn a_fake_input_feeds_the_tap_until_stopped() {
        let mut m = InputMonitor::with_fake(Some("sine:440:-12"));
        assert!(m.is_fake() && !m.is_open());
        m.start(None);
        assert!(m.is_open() && m.error.is_none());
        std::thread::sleep(Duration::from_millis(250));
        let s = m.tap.stats();
        assert!(s.samples > 4_000, "about 250 ms of stereo at 48 kHz: {s:?}");
        assert!((s.peak_db() - -12.0).abs() < 0.2, "{s:?}");
        m.stop();
        std::thread::sleep(Duration::from_millis(60));
        let n = m.tap.stats().samples;
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(m.tap.stats().samples, n, "closed: nothing more arrives");
        assert!(!m.stale(Some("anything")), "a fake input is never stale");
    }

    #[test]
    fn a_bad_fake_spec_is_an_error_not_a_device() {
        let mut m = InputMonitor::with_fake(Some("banana"));
        m.start(None);
        assert!(!m.is_open() && m.error.is_some());
    }
}
