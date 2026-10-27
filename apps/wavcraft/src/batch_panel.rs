//! File › Batch Process…: a list of input files, a chain of operations, an output folder and
//! name pattern, run file by file on a worker thread (f73). The work is `wavcraft_core::batch`;
//! this is the panel, the worker and the `BATCH …` control verb.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};

use wavcraft_core::batch::{self, Event, Job, Op, Outcome};
use wavcraft_core::io::OpenOptions;
use wavcraft_core::save::SampleType;

/// Extensions picked up when a folder is added.
const AUDIO_EXTENSIONS: &[&str] = &["wav", "wave", "aif", "aiff", "flac", "mp3", "ogg", "oga", "m4a", "aac"];

enum Msg {
    Event(Event),
    Done,
}

struct Running {
    rx: Receiver<Msg>,
    cancel: Arc<AtomicBool>,
}

pub struct BatchPanel {
    pub open: bool,
    job: Job,
    /// The peak target typed for a new Normalize step.
    peak_text: String,
    gain_text: String,
    /// One per input once a run has started (`None` until that file finishes).
    results: Vec<Option<Outcome>>,
    current: Option<usize>,
    file_progress: f64,
    overall: f64,
    run: Option<Running>,
    message: Option<String>,
}

impl Default for BatchPanel {
    fn default() -> Self {
        let out = dirs::audio_dir().or_else(dirs::home_dir).unwrap_or_default().join("WavCraft batch");
        BatchPanel {
            open: false,
            job: Job::new(out),
            peak_text: "-1".to_string(),
            gain_text: "0".to_string(),
            results: Vec::new(),
            current: None,
            file_progress: 0.0,
            overall: 0.0,
            run: None,
            message: None,
        }
    }
}

fn ext_ok(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

impl BatchPanel {
    pub fn running(&self) -> bool {
        self.run.is_some()
    }

    /// Add a file, or the audio files directly inside a folder (sorted by name). Returns how many were added.
    pub fn add_path(&mut self, path: &Path) -> Result<usize, String> {
        if self.running() {
            return Err("a batch is running".to_string());
        }
        let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut found = Vec::new();
        if meta.is_dir() {
            let entries = std::fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
            found.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_file() && ext_ok(p)));
            found.sort();
        } else {
            found.push(path.to_path_buf());
        }
        let mut added = 0;
        for p in found {
            if !self.job.inputs.contains(&p) {
                self.job.inputs.push(p);
                added += 1;
            }
        }
        self.results.clear();
        Ok(added)
    }

    /// Start the run on a worker thread.
    pub fn start(&mut self, sessions_root: &Path, ctx: &egui::Context) -> Result<(), String> {
        if self.running() {
            return Err("a batch is already running".to_string());
        }
        if self.job.inputs.is_empty() {
            return Err("no files to process".to_string());
        }
        if self.job.out_dir.as_os_str().is_empty() {
            return Err("no output folder".to_string());
        }
        self.results = vec![None; self.job.inputs.len()];
        (self.current, self.file_progress, self.overall, self.message) = (None, 0.0, 0.0, None);
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let (job, flag, ctx) = (self.job.clone(), cancel.clone(), ctx.clone());
        let opts = OpenOptions { sessions_root: Some(sessions_root.to_path_buf()), ..OpenOptions::default() };
        std::thread::Builder::new()
            .name("batch".into())
            .spawn(move || {
                batch::run(&job, &opts, &flag, &mut |e| {
                    let _ = tx.send(Msg::Event(e));
                    ctx.request_repaint();
                });
                let _ = tx.send(Msg::Done);
                ctx.request_repaint();
            })
            .map_err(|e| format!("could not start the worker: {e}"))?;
        self.run = Some(Running { rx, cancel });
        log::info!(target: "wavcraft", "batch started: {} files, {} steps, output {}", self.job.inputs.len(), self.job.ops.len(), self.job.out_dir.display());
        Ok(())
    }

    pub fn cancel(&self) {
        if let Some(r) = &self.run {
            r.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Take what the worker has sent (called every frame, window shown or not).
    pub fn poll(&mut self) {
        let Some(run) = &self.run else { return };
        let mut done = false;
        while let Ok(m) = run.rx.try_recv() {
            match m {
                Msg::Event(Event::Started { index, .. }) => (self.current, self.file_progress) = (Some(index), 0.0),
                Msg::Event(Event::Progress { file, overall, .. }) => (self.file_progress, self.overall) = (file, overall),
                Msg::Event(Event::Finished { index, outcome }) => {
                    if let Some(slot) = self.results.get_mut(index) {
                        *slot = Some(outcome);
                    }
                }
                Msg::Done => done = true,
            }
        }
        if done || run.rx.try_recv().is_err_and(|e| matches!(e, std::sync::mpsc::TryRecvError::Disconnected)) {
            self.run = None;
            self.current = None;
            let (ok, failed, _) = self.counts();
            // Files never reached after a cancel have no result yet.
            for r in self.results.iter_mut().filter(|r| r.is_none()) {
                *r = Some(Outcome::Cancelled);
            }
            if self.results.contains(&Some(Outcome::Cancelled)) {
                self.message = Some("Cancelled.".to_string());
            } else {
                self.overall = 1.0;
                self.message = Some(format!("Done: {ok} ok, {failed} failed."));
            }
            log::info!(target: "wavcraft", "batch finished: {}", self.status_line());
        }
    }

    fn counts(&self) -> (usize, usize, usize) {
        let n = |f: fn(&Outcome) -> bool| self.results.iter().flatten().filter(|o| f(o)).count();
        (n(|o| matches!(o, Outcome::Ok { .. })), n(|o| matches!(o, Outcome::Failed { .. })), n(|o| matches!(o, Outcome::Cancelled)))
    }

    /// The answer to `BATCH status`.
    pub fn status_line(&self) -> String {
        let (ok, failed, cancelled) = self.counts();
        let done = self.results.iter().flatten().count();
        let results: Vec<String> = self
            .job
            .inputs
            .iter()
            .zip(&self.results)
            .map(|(i, r)| {
                let name = i.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                match r {
                    Some(Outcome::Ok { output }) => format!("{name}:ok:{}", output.display()),
                    Some(Outcome::Failed { reason }) => format!("{name}:failed:{}", reason.replace('|', "/")),
                    Some(Outcome::Cancelled) => format!("{name}:cancelled"),
                    None => format!("{name}:pending"),
                }
            })
            .collect();
        format!(
            "running={} files={} done={done} ok={ok} failed={failed} cancelled={cancelled} overall={:.2} steps={} overwrite={} results={}",
            u8::from(self.running()),
            self.job.inputs.len(),
            self.overall,
            self.job.ops.iter().map(|o| o.describe().replace(' ', "_")).collect::<Vec<_>>().join(";"),
            u8::from(self.job.overwrite),
            results.join(" | ")
        )
    }

    /// `BATCH <rest>`: add, op, out, overwrite, clear, run, cancel, status.
    pub fn control(&mut self, rest: &str, sessions_root: &Path, ctx: &egui::Context) -> String {
        let (verb, arg) = rest.trim().split_once(' ').map_or((rest.trim(), ""), |(v, a)| (v, a.trim()));
        let reply = |r: Result<String, String>| r.map_or_else(|e| format!("ERR {e}"), |s| format!("OK{}{s}", if s.is_empty() { "" } else { " " }));
        match verb {
            "add" if !arg.is_empty() => reply(self.add_path(Path::new(arg)).map(|n| format!("added={n} files={}", self.job.inputs.len()))),
            "op" if arg == "clear" => {
                self.job.ops.clear();
                "OK".to_string()
            }
            "op" => reply(Op::parse(arg).map(|op| {
                self.job.ops.push(op);
                format!("steps={}", self.job.ops.len())
            })),
            "out" if !arg.is_empty() => {
                let (folder, pattern) = match arg.rsplit_once(" pattern=") {
                    Some((f, p)) => (f.trim(), Some(p.trim())),
                    None => (arg, None),
                };
                if let Some(p) = pattern {
                    if let Err(e) = batch::output_name(p, Path::new("x.wav"), 0) {
                        return format!("ERR {e}");
                    }
                    self.job.pattern = p.to_string();
                }
                self.job.out_dir = PathBuf::from(folder);
                "OK".to_string()
            }
            "overwrite" => match arg {
                "on" | "off" => {
                    self.job.overwrite = arg == "on";
                    "OK".to_string()
                }
                _ => "ERR BATCH overwrite on|off".to_string(),
            },
            "clear" if !self.running() => {
                self.job.inputs.clear();
                self.results.clear();
                "OK".to_string()
            }
            "run" => reply(self.start(sessions_root, ctx).map(|()| String::new())),
            "cancel" => {
                self.cancel();
                "OK".to_string()
            }
            "status" => format!("OK {}", self.status_line()),
            _ => "ERR BATCH add <path> | op <normalize peak=-1|gain db=3|mono|clear> | out <folder> [pattern=<p>] | overwrite on|off | clear | run | cancel | status".to_string(),
        }
    }

    /// The panel window.
    pub fn ui(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut open = true;
        let idle = !self.running();
        egui::Window::new("Batch Process").open(&mut open).default_width(520.0).show(ctx, |ui| {
            ui.add_enabled_ui(idle, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Files");
                    if ui.button("Add Files…").clicked()
                        && let Some(files) = rfd::FileDialog::new().add_filter("Audio", AUDIO_EXTENSIONS).pick_files()
                    {
                        for f in files {
                            let _ = self.add_path(&f);
                        }
                    }
                    if ui.button("Add Folder…").clicked()
                        && let Some(dir) = rfd::FileDialog::new().pick_folder()
                    {
                        match self.add_path(&dir) {
                            Ok(0) => self.message = Some("No audio files in that folder.".to_string()),
                            Ok(_) => {}
                            Err(e) => self.message = Some(e),
                        }
                    }
                    if ui.button("Clear").clicked() {
                        self.job.inputs.clear();
                        self.results.clear();
                    }
                });
            });
            egui::ScrollArea::vertical().id_salt("batch files").max_height(160.0).show(ui, |ui| {
                let mut remove = None;
                for (i, f) in self.job.inputs.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if idle && ui.small_button("✕").clicked() {
                            remove = Some(i);
                        }
                        let name = f.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                        let status = match self.results.get(i).cloned().flatten() {
                            Some(Outcome::Ok { .. }) => "  ok".to_string(),
                            Some(Outcome::Failed { reason }) => format!("  failed: {reason}"),
                            Some(Outcome::Cancelled) => "  cancelled".to_string(),
                            None if self.current == Some(i) => format!("  {:.0}%", self.file_progress * 100.0),
                            None => String::new(),
                        };
                        ui.label(format!("{name}{status}")).on_hover_text(f.display().to_string());
                    });
                }
                if let Some(i) = remove {
                    self.job.inputs.remove(i);
                    self.results.clear();
                }
            });
            ui.separator();
            ui.add_enabled_ui(idle, |ui| {
                ui.strong("Steps (in order)");
                let mut remove = None;
                for (i, op) in self.job.ops.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.small_button("✕").clicked() {
                            remove = Some(i);
                        }
                        ui.label(op.describe());
                    });
                }
                if let Some(i) = remove {
                    self.job.ops.remove(i);
                }
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.peak_text).desired_width(40.0));
                    if ui.button("Add Normalize to peak (dBFS)").clicked()
                        && let Ok(Op::NormalizePeak { target_db }) = Op::parse(&format!("normalize peak={}", self.peak_text.trim()))
                    {
                        self.job.ops.push(Op::NormalizePeak { target_db });
                    }
                });
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.gain_text).desired_width(40.0));
                    if ui.button("Add Gain (dB)").clicked()
                        && let Ok(op) = Op::parse(&format!("gain db={}", self.gain_text.trim()))
                    {
                        self.job.ops.push(op);
                    }
                    if ui.button("Add Mix to Mono").clicked() {
                        self.job.ops.push(Op::ToMono);
                    }
                });
                ui.separator();
                ui.strong("Save as WAV");
                ui.horizontal(|ui| {
                    ui.label("Folder");
                    ui.label(self.job.out_dir.display().to_string());
                    if ui.button("Choose…").clicked()
                        && let Some(dir) = rfd::FileDialog::new().pick_folder()
                    {
                        self.job.out_dir = dir;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Name").on_hover_text("{name} is the input's name, {n} its place in the list");
                    ui.add(egui::TextEdit::singleline(&mut self.job.pattern).desired_width(180.0));
                    ui.label(".wav");
                });
                ui.horizontal(|ui| {
                    ui.label("Sample type");
                    let types = [
                        (None, "Same as source"),
                        (Some(SampleType::Int16), "16-bit"),
                        (Some(SampleType::Int24), "24-bit"),
                        (Some(SampleType::Float32), "32-bit float"),
                    ];
                    let label = types.iter().find(|(t, _)| *t == self.job.sample_type).map_or("", |(_, l)| *l);
                    egui::ComboBox::from_id_salt("batch type").selected_text(label).show_ui(ui, |ui| {
                        for (t, l) in types {
                            ui.selectable_value(&mut self.job.sample_type, t, l);
                        }
                    });
                });
                ui.checkbox(&mut self.job.overwrite, "Replace existing files (including the originals)")
                    .on_hover_text("Off: a file that already exists in the output folder is skipped and reported as failed.");
            });
            ui.separator();
            ui.horizontal(|ui| {
                if idle {
                    if ui.add_enabled(!self.job.inputs.is_empty(), egui::Button::new("Run")).clicked() {
                        let root = wavcraft_core::store::default_sessions_root();
                        if let Err(e) = self.start(&root, ui.ctx()) {
                            self.message = Some(e);
                        }
                    }
                } else if ui.button("Cancel").clicked() {
                    self.cancel();
                }
                if let Some(m) = &self.message {
                    ui.label(m);
                }
            });
            if !idle || self.overall > 0.0 {
                ui.add(egui::ProgressBar::new(self.overall as f32).text(format!("Overall {:.0}%", self.overall * 100.0)));
            }
        });
        self.open = open;
    }
}
