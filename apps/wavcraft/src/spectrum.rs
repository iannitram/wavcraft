//! The spectral display's pictures, rendered on a worker thread.
//!
//! Zoomed out, a picture of the audio alone analyses all of it that is on screen: about 140 ms
//! per channel for three hours, far longer than a frame. So the window asks for a picture and
//! goes on drawing the one it has until the new one arrives.
//!
//! The worker also keeps the open file's overviews (see `wavcraft_core::spectrogram::Overview`),
//! from which a far view takes a few milliseconds. It builds them before the file's first
//! picture: that picture shows the whole file, so it would run the same FFTs anyway.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use egui::{Color32, ColorImage};
use wavcraft_core::spectrogram::{self, Columns, Overview};
use wavcraft_core::{AudioBuffer, SpectrogramParams};

/// Which picture: of which file, which columns of it, how tall, analysed how.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Job {
    /// The open file's number (see `WaveApp::generation`).
    pub generation: usize,
    pub columns: Columns,
    pub rows: usize,
    pub params: SpectrogramParams,
}

/// A finished job: one image per channel.
pub struct Picture {
    pub job: Job,
    pub images: Vec<ColorImage>,
}

pub struct Renderer {
    jobs: Sender<(Job, Arc<AudioBuffer>)>,
    pictures: Receiver<Picture>,
    /// The job last sent, so each one is asked for once.
    asked: Option<Job>,
}

/// One overview per channel of one file, for one FFT length, window and dB scale.
struct Overviews {
    generation: usize,
    params: SpectrogramParams,
    channels: Vec<Overview>,
}

impl Overviews {
    /// Whether these were built from `job`'s file with its analysis. The frequency axis may differ.
    fn serve(&self, job: &Job) -> bool {
        // Compared as bits, so a scale that is not a number is built for once, not for every picture.
        let analysis = |p: &SpectrogramParams| (p.fft_size, p.window, p.floor_db.to_bits(), p.ceil_db.to_bits());
        self.generation == job.generation && analysis(&self.params) == analysis(&job.params)
    }
}

impl Renderer {
    /// Start the worker. It colours levels with `palette` and wakes `ctx` when a picture is ready.
    pub fn new(ctx: &egui::Context, palette: Vec<Color32>) -> Renderer {
        let (jobs, inbox) = channel::<(Job, Arc<AudioBuffer>)>();
        let (outbox, pictures) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut overviews: Option<Overviews> = None;
            while let Ok(mut request) = inbox.recv() {
                // The view moved on while the last picture was being made: only the newest counts.
                while let Ok(newer) = inbox.try_recv() {
                    request = newer;
                }
                let (job, audio) = request;
                if !overviews.as_ref().is_some_and(|o| o.serve(&job)) {
                    // Drop the last file's before building this one's.
                    drop(overviews.take());
                    let channels = audio.channels.iter().map(|samples| Overview::build(samples, &job.params)).collect();
                    overviews = Some(Overviews { generation: job.generation, params: job.params, channels });
                }
                let cols = job.columns.cols;
                let images = audio.channels.iter().enumerate().map(|(ch, samples)| {
                    let overview = overviews.as_ref().and_then(|o| o.channels.get(ch));
                    let levels = spectrogram::render_columns(samples, audio.sample_rate, job.columns, job.rows, &job.params, overview);
                    let pixels: Vec<Color32> = levels.iter().map(|l| palette.get(usize::from(*l)).copied().unwrap_or(Color32::BLACK)).collect();
                    // `render_columns` clamps what it is asked for; an image must be exactly its stated size.
                    if pixels.len() == cols * job.rows {
                        ColorImage::new([cols, job.rows], pixels)
                    } else {
                        ColorImage::filled([1, 1], Color32::BLACK)
                    }
                });
                if outbox.send(Picture { job, images: images.collect() }).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Renderer { jobs, pictures, asked: None }
    }

    /// Ask for `job`'s picture of `audio`, unless it is the one already asked for.
    pub fn ask(&mut self, job: Job, audio: &Arc<AudioBuffer>) {
        if self.asked != Some(job) && job.columns.cols > 0 && job.rows > 0 && self.jobs.send((job, audio.clone())).is_ok() {
            self.asked = Some(job);
        }
    }

    /// The newest picture finished since the last call, if any.
    pub fn take(&mut self) -> Option<Picture> {
        self.pictures.try_iter().last()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Ask for `job` and wait for its picture.
    fn picture(renderer: &mut Renderer, job: Job, audio: &Arc<AudioBuffer>) -> Picture {
        renderer.ask(job, audio);
        let asked = Instant::now();
        loop {
            match renderer.take() {
                Some(picture) if picture.job == job => return picture,
                _ => assert!(asked.elapsed() < Duration::from_secs(60), "no picture of {job:?}"),
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn the_worker_draws_each_file_as_the_audio_alone_would() {
        let rate = 8_000;
        // Two files of the same length: an overview kept from the first would be wrong for the second.
        let file = |seed: u32| {
            let wave = |ch: u32| (0..600_000u32).map(|i| ((i / (997 + 131 * ch + seed) % 7) as f32 / 7.0 - 0.4) * (i as f32 * 0.37).sin()).collect();
            Arc::new(AudioBuffer { sample_rate: rate, channels: vec![wave(0), wave(1)] })
        };
        let palette: Vec<Color32> = (0..=255).map(Color32::from_gray).collect();
        let mut renderer = Renderer::new(&egui::Context::default(), palette);
        let params = SpectrogramParams { fft_size: 256, ..SpectrogramParams::default() };
        for (generation, audio) in [(1, file(0)), (2, file(40))] {
            // The whole file (37 FFTs a column), a closer view, the other frequency axis, one FFT a column.
            for (start, end, per_col, log_freq) in [
                (0.0, 600_000.0, 4_700.0, true),
                (250_000.0, 400_000.0, 1_100.0, true),
                (0.0, 600_000.0, 4_700.0, false),
                (300_000.0, 310_000.0, 50.0, true),
            ] {
                let params = SpectrogramParams { log_freq, ..params };
                let columns = Columns::covering(start, end, per_col, &params);
                let got = picture(&mut renderer, Job { generation, columns, rows: 48, params }, &audio);
                assert_eq!(got.images.len(), 2);
                for (samples, image) in audio.channels.iter().zip(&got.images) {
                    let alone = spectrogram::render(samples, rate, columns.start, columns.end, columns.cols, 48, &params);
                    assert!(alone.iter().any(|l| *l > 100), "there is something to see");
                    assert_eq!(image.size, [columns.cols, 48]);
                    assert!(image.pixels.iter().zip(&alone).all(|(px, l)| *px == Color32::from_gray(*l)), "file {generation}, view from {start}");
                }
            }
        }
    }
}
