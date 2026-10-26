//! VST3 plugin hosting in the Effects Rack (f128, alpha), on SoundCraft's `vst3-host` crate.
//!
//! **Scanning never loads a plugin in the app's own process.** [`scan_with`] finds the `.vst3`
//! bundles in the standard folders by looking at the file system only, then asks a worker
//! process (the app itself started with `--vst3-scan <bundle>`) about each bundle, one process
//! per bundle, with a timeout: a plugin that hangs or crashes costs only itself, and is reported
//! in [`ScanReport::skipped`]. The worker prints [`WORKER_TAG`]-tagged lines, so anything a
//! plugin writes to its standard output is ignored.
//!
//! A rack slot holds a plugin as `vst3:<name>` (or `vst3:<class id>`); the name is looked up in
//! the list the last scan found. Loading the chosen plugin into a rack ([`open_stream`]) happens
//! in the app's process (a plugin that crashes there takes the app with it: the host cannot
//! sandbox native code). Plugin parameters and editor windows are not exposed yet.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

use soundcraft_dsp::Plugin;
use soundcraft_vst3_host as host;

use crate::effects::EffectStream;
use crate::rack::MAX_BLOCK;

/// A rack slot's effect text starts with this for a hosted plugin.
pub const PREFIX: &str = "vst3:";
/// The command-line flag that makes the app act as a scan worker for one bundle.
pub const WORKER_FLAG: &str = "--vst3-scan";
/// Starts every result line of a worker.
pub const WORKER_TAG: &str = "WAVCRAFT-VST3";
/// How long one bundle may take to report before its worker is killed.
pub const BUNDLE_TIMEOUT: Duration = Duration::from_secs(10);

/// One plugin (an audio effect class) a scan found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// `vst3:<32 hex digits>`.
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub path: String,
}

/// What a scan found.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ScanReport {
    pub plugins: Vec<Found>,
    /// Bundles that hung, crashed or could not be read, with the reason.
    pub skipped: Vec<String>,
}

/// The standard VST3 folders of this machine (macOS: `/Library/Audio/Plug-Ins/VST3` and the
/// user's `~/Library/Audio/Plug-Ins/VST3`).
pub fn default_dirs() -> Vec<PathBuf> {
    host::scan::default_search_paths()
}

/// The `.vst3` bundles under `dirs`, by file system only (nothing is loaded).
pub fn find_bundles(dirs: &[PathBuf]) -> Vec<PathBuf> {
    host::scan::find_bundles(dirs)
}

fn clean(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_string()
}

/// The worker side: load `bundle`, return its tagged result lines (effects only; instruments
/// are not listed). Runs in the worker process.
pub fn worker_lines(bundle: &Path) -> String {
    let mut out = String::new();
    for d in host::scan_paths(&[bundle.to_path_buf()]) {
        if d.is_instrument {
            continue;
        }
        out.push_str(&format!("{WORKER_TAG}\t{}\t{}\t{}\t{}\n", clean(&d.id), clean(&d.name), clean(&d.vendor), clean(&d.path.to_string_lossy())));
    }
    out
}

/// The plugins in a worker's output (lines without the tag, or malformed, are ignored).
pub fn parse_worker_output(text: &str) -> Vec<Found> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            if f.next()? != WORKER_TAG {
                return None;
            }
            let (id, name, vendor, path) = (f.next()?, f.next()?, f.next()?, f.next()?);
            (id.starts_with(PREFIX) && !name.is_empty()).then(|| Found { id: id.into(), name: name.into(), vendor: vendor.into(), path: path.into() })
        })
        .collect()
}

/// Run `cmd` as a worker: read its standard output, give it `timeout`, kill it after that.
/// A worker that fails or reports nothing is an `Err` saying why.
pub fn run_worker(mut cmd: Command, timeout: Duration) -> Result<Vec<Found>, String> {
    let mut child =
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| format!("cannot start the scan worker: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("no worker output")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        let _ = tx.send(String::from_utf8_lossy(&bytes).into_owned());
    });
    match rx.recv_timeout(timeout) {
        Ok(text) => {
            let status = child.wait().map_err(|e| format!("scan worker: {e}"))?;
            let found = parse_worker_output(&text);
            if found.is_empty() && !status.success() { Err(format!("the plugin crashed or failed while loading ({status})")) } else { Ok(found) }
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(format!("the plugin did not answer within {} s and was stopped", timeout.as_secs_f32()))
        }
    }
}

/// Scan `dirs` bundle by bundle with `exe` as the worker (the app's own executable).
pub fn scan_with(exe: &Path, dirs: &[PathBuf], timeout: Duration) -> ScanReport {
    let mut report = ScanReport::default();
    for bundle in find_bundles(dirs) {
        let mut cmd = Command::new(exe);
        cmd.arg(WORKER_FLAG).arg(&bundle);
        match run_worker(cmd, timeout) {
            Ok(found) => {
                for f in found {
                    if !report.plugins.iter().any(|p| p.id == f.id) {
                        report.plugins.push(f);
                    }
                }
            }
            Err(e) => report.skipped.push(format!("{}: {e}", bundle.display())),
        }
    }
    report.plugins.sort_by_key(|p| p.name.to_lowercase());
    report
}

static FOUND: Mutex<Vec<Found>> = Mutex::new(Vec::new());

/// Remember a scan's plugins for [`resolve`] and the slot menus.
pub fn set_found(plugins: Vec<Found>) {
    *FOUND.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = plugins;
}

/// The plugins the last scan found.
pub fn found() -> Vec<Found> {
    FOUND.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
}

/// The plugin a slot's effect text means: `vst3:<name>` (case-insensitive) or `vst3:<class id>`.
pub fn resolve(spec: &str) -> Result<Found, String> {
    let key = spec.strip_prefix(PREFIX).unwrap_or(spec).trim();
    let list = found();
    list.iter().find(|f| f.id.eq_ignore_ascii_case(&format!("{PREFIX}{key}")) || f.name.eq_ignore_ascii_case(key)).cloned().ok_or_else(|| {
        if list.is_empty() {
            format!("no VST3 plugin named {key:?}: none found yet (run VST3 scan)")
        } else {
            format!("no VST3 plugin named {key:?} (VST3 list shows what was found)")
        }
    })
}

/// Whether a slot's effect text names a hosted plugin.
pub fn is_vst3(effect: &str) -> bool {
    effect.starts_with(PREFIX)
}

/// A loaded plugin as a rack stream: planar blocks in, planar blocks out.
struct Vst3Stream {
    plugin: Box<dyn Plugin>,
    io: Vec<Vec<f32>>,
}

impl EffectStream for Vst3Stream {
    fn process_block(&mut self, input: &[&[f32]], output: &mut [&mut [f32]]) {
        let n = input.first().map_or(0, |c| c.len()).min(MAX_BLOCK);
        for (buf, src) in self.io.iter_mut().zip(input) {
            buf.clear();
            buf.extend_from_slice(&src[..n]);
        }
        self.plugin.process(&mut self.io, n);
        for (dst, buf) in output.iter_mut().zip(&self.io) {
            let k = buf.len().min(dst.len());
            dst[..k].copy_from_slice(&buf[..k]);
            dst[k..].fill(0.0);
        }
    }
}

/// Load the plugin `spec` names into a stream for `channels` channels at `rate` Hz. All memory is
/// allocated here (blocks of up to [`MAX_BLOCK`] frames).
pub fn open_stream(spec: &str, channels: usize, rate: u32) -> Result<Box<dyn EffectStream>, String> {
    let f = resolve(spec)?;
    // Registers just this bundle with the host, so it never scans the default folders itself.
    host::add_search_dir(Path::new(&f.path));
    let mut plugin = host::instantiate(&f.id).map_err(|e| format!("cannot load {}: {e}", f.name))?;
    plugin.prepare(rate as f32, MAX_BLOCK, channels);
    let io = (0..channels).map(|_| Vec::with_capacity(MAX_BLOCK)).collect();
    Ok(Box::new(Vst3Stream { plugin, io }))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn sh(script: &str) -> Command {
        let mut c = Command::new("sh");
        c.arg("-c").arg(script);
        c
    }

    #[test]
    fn an_empty_or_missing_folder_has_no_bundles() {
        let dir = std::env::temp_dir().join(format!("wavcraft-vst3-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(find_bundles(&[dir.clone(), dir.join("nope")]).is_empty());
        let r = scan_with(Path::new("/nonexistent/worker"), std::slice::from_ref(&dir), Duration::from_secs(1));
        assert_eq!(r, ScanReport::default());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn worker_output_is_parsed_and_noise_ignored() {
        let text = format!("plugin says hello\n{WORKER_TAG}\tvst3:ABCD\tVerb\tAcme\t/x/Verb.vst3\n{WORKER_TAG}\tbad\n\n");
        let found = parse_worker_output(&text);
        assert_eq!(found, vec![Found { id: "vst3:ABCD".into(), name: "Verb".into(), vendor: "Acme".into(), path: "/x/Verb.vst3".into() }]);
    }

    #[test]
    fn a_worker_that_hangs_is_killed_at_the_timeout() {
        let t = std::time::Instant::now();
        let r = run_worker(sh("sleep 30"), Duration::from_millis(300));
        assert!(r.unwrap_err().contains("did not answer"));
        assert!(t.elapsed() < Duration::from_secs(10), "took {:?}", t.elapsed());
    }

    #[test]
    fn a_worker_that_crashes_is_an_error_and_one_that_answers_is_read() {
        assert!(run_worker(sh("kill -SEGV $$"), Duration::from_secs(5)).unwrap_err().contains("crashed"));
        let line = format!("printf '{WORKER_TAG}\\tvst3:AA\\tGain\\tV\\t/p\\n'");
        let ok = run_worker(sh(&line), Duration::from_secs(5)).unwrap();
        assert_eq!(ok.len(), 1);
        assert_eq!(ok[0].name, "Gain");
    }

    #[test]
    fn resolve_by_name_or_id_and_unknown_names_say_so() {
        set_found(vec![Found { id: "vst3:00FF".into(), name: "Fixture Gain".into(), vendor: String::new(), path: "/p".into() }]);
        assert_eq!(resolve("vst3:fixture gain").unwrap().id, "vst3:00FF");
        assert_eq!(resolve("vst3:00ff").unwrap().name, "Fixture Gain");
        assert!(resolve("vst3:Nope").unwrap_err().contains("VST3 list"));
        set_found(Vec::new());
        assert!(resolve("vst3:Nope").unwrap_err().contains("VST3 scan"));
    }
}
