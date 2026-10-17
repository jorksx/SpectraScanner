#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod config;
mod dsp;
mod dsp_worker;
mod gui;
mod scanner;
mod sdr;

use app::SpectraScannerApp;
use eframe::egui;

#[cfg(target_os = "windows")]
fn init_dpi_awareness() {
    #[link(name = "user32")]
    extern "system" {
        fn SetProcessDpiAwarenessContext(value: isize) -> i32;
    }
    // DPI_AWARENESS_CONTEXT_SYSTEM_AWARE = -2
    // Locks process DPI awareness to System DPI awareness before winit starts.
    // Works around rust-windowing/winit#4041 on Windows 11 where dragging between
    // monitors of different scale factors triggers an infinite WM_DPICHANGED resize storm.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(-2);
    }
}

fn main() -> Result<(), eframe::Error> {
    #[cfg(target_os = "windows")]
    init_dpi_awareness();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 880.0])
            .with_min_inner_size([900.0, 650.0])
            .with_title("SpectraScanner - Tactical Multi-VFO SDR Scanner"),
        ..Default::default()
    };

    eframe::run_native(
        "SpectraScanner",
        native_options,
        Box::new(|cc| Ok(Box::new(SpectraScannerApp::new(cc)))),
    )
}
