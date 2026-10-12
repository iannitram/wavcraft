//! The spectral display's pictures, rendered on a worker thread.
//!
//! Zoomed out, a picture analyses all the audio on screen: about 140 ms per channel for three
//! hours, far longer than a frame. So the window asks for a picture and goes on drawing the one
//! it has until the new one arrives.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use egui::{Color32, ColorImage};
use wavecraft_core::spectrogram::{self, Columns};
use wavecraft_core::{AudioBuffer, SpectrogramParams};

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

impl Renderer {
    /// Start the worker. It colours levels with `palette` and wakes `ctx` when a picture is ready.
    pub fn new(ctx: &egui::Context, palette: Vec<Color32>) -> Renderer {
        let (jobs, inbox) = channel::<(Job, Arc<AudioBuffer>)>();
        let (outbox, pictures) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            while let Ok(mut request) = inbox.recv() {
                // The view moved on while the last picture was being made: only the newest counts.
                while let Ok(newer) = inbox.try_recv() {
                    request = newer;
                }
                let (job, audio) = request;
                let Columns { start, end, cols } = job.columns;
                let images = audio.channels.iter().map(|samples| {
                    let levels = spectrogram::render(samples, audio.sample_rate, start, end, cols, job.rows, &job.params);
                    let pixels: Vec<Color32> = levels.iter().map(|l| palette.get(usize::from(*l)).copied().unwrap_or(Color32::BLACK)).collect();
                    // `render` clamps what it is asked for; an image must be exactly its stated size.
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
