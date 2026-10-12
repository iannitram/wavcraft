//! WaveCraft: a single-file waveform editor on SoundCraft's engine.
//!
//! `wavecraft [FILE]` opens the window, with FILE loaded if given.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
// A release build is a windowed app: no console window behind it on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod player;
mod spectrum;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let file = std::env::args_os().nth(1).map(PathBuf::from);
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("WaveCraft").with_inner_size([1280.0, 720.0]).with_min_inner_size([640.0, 360.0]),
        ..Default::default()
    };
    configure_graphics(&mut options);
    eframe::run_native("WaveCraft", options, Box::new(move |cc| Ok(Box::new(app::WaveApp::new(cc, file)))))
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
