//! WavCraft: a single-file waveform editor on SoundCraft's engine.
//!
//! `wavcraft [--session-dir DIR] [--reset-preferences] [FILE]` opens the window, with FILE loaded if given.
//! `--session-dir` sets the folder opened files' session stores go in, for this run (over the
//! `WAVCRAFT_SESSION_DIR` environment variable and the setting made with File › Session Folder…).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
// A release build is a windowed app: no console window behind it on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod control;
mod effect_dialog;
mod multitrack_ui;
mod mixer;
mod meters_ui;
mod player;
// Not called yet: the Reveal in file manager menu item will use it.
#[allow(dead_code)]
mod reveal;
mod rack_panel;
mod selflog;
mod spectrum;
mod video;
mod statspanel;
#[cfg(windows)]
mod win_acl;

use std::ffi::OsString;
use std::path::PathBuf;

fn main() -> eframe::Result {
    // Scan worker (f128): report one VST3 bundle on standard output and exit; no window, no log.
    {
        let mut raw = std::env::args_os().skip(1);
        if raw.next().is_some_and(|a| a == wavcraft_core::vst3::WORKER_FLAG) {
            if let Some(bundle) = raw.next() {
                use std::io::Write;
                let text = wavcraft_core::vst3::worker_lines(std::path::Path::new(&bundle));
                let mut out = std::io::stdout();
                let _ = out.write_all(text.as_bytes());
                let _ = out.flush();
            }
            std::process::exit(0);
        }
    }
    let Args { file, session_dir, reset_preferences } = parse_args(std::env::args_os().skip(1));
    selflog::install();
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("WavCraft").with_inner_size([1280.0, 720.0]).with_min_inner_size([640.0, 360.0]),
        ..Default::default()
    };
    configure_graphics(&mut options);
    eframe::run_native("WavCraft", options, Box::new(move |cc| Ok(Box::new(app::WaveApp::new(cc, file, session_dir, reset_preferences)))))
}

/// On Windows, start wgpu with DirectX 12 only unless `WGPU_BACKEND` says otherwise. SoundCraft
/// does the same: letting wgpu also create an OpenGL instance crashes some graphics drivers
/// before the window appears.
fn configure_graphics(options: &mut eframe::NativeOptions) {
    if cfg!(windows)
        && std::env::var_os("WGPU_BACKEND").is_none()
        && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
    {
        create.instance_descriptor.backends = eframe::wgpu::Backends::DX12;
    }
}

/// The command line.
#[derive(Debug, Default, PartialEq, Eq)]
struct Args {
    file: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    /// `--reset-preferences`: move the settings file aside and start with the defaults.
    reset_preferences: bool,
}

/// `[--session-dir DIR | --session-dir=DIR] [--reset-preferences] [FILE]`, in any order. The first other argument is
/// the file; anything after it is ignored.
fn parse_args(args: impl IntoIterator<Item = OsString>) -> Args {
    let mut out = Args::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy();
        if text == "--session-dir" {
            out.session_dir = args.next().map(PathBuf::from);
        } else if text == "--reset-preferences" {
            out.reset_preferences = true;
        } else if let Some(dir) = text.strip_prefix("--session-dir=") {
            out.session_dir = Some(PathBuf::from(dir));
        } else if out.file.is_none() {
            out.file = Some(PathBuf::from(arg));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Args {
        parse_args(a.iter().map(OsString::from))
    }

    #[test]
    fn the_session_dir_flag_and_the_file_in_any_order() {
        let both = Args { file: Some("a.wav".into()), session_dir: Some("D:/s".into()), reset_preferences: false };
        assert_eq!(args(&["--session-dir", "D:/s", "a.wav"]), both);
        assert_eq!(args(&["a.wav", "--session-dir=D:/s"]), both);
        assert_eq!(args(&["a.wav"]), Args { file: Some("a.wav".into()), ..Args::default() });
        assert_eq!(args(&[]), Args::default());
        assert_eq!(args(&["--session-dir"]), Args::default());
        assert!(args(&["--reset-preferences", "a.wav"]).reset_preferences);
        assert!(!args(&["a.wav"]).reset_preferences);
    }
}
