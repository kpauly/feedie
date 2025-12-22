#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! Entry point for the Feedie egui desktop application.

mod app;
mod export;
mod i18n;
mod manifest;
mod model;
mod roboflow;
mod settings_store;
mod util;

use app::UiApp;
use eframe::{NativeOptions, egui};
use egui::viewport::ViewportBuilder;
use std::sync::Arc;
use util::load_app_icon;

/// Bootstraps the egui application and installs tracing and the window icon.
fn main() {
    #[cfg(debug_assertions)]
    tracing_subscriber::fmt::init();

    let options = NativeOptions {
        viewport: ViewportBuilder::default().with_icon(Arc::new(load_app_icon())),
        ..Default::default()
    };

    if let Err(err) = eframe::run_native(
        "Feedie",
        options,
        Box::new(|_cc| {
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(Box::new(UiApp::default()))
        }),
    ) {
        eprintln!("Applicatie gestopt met fout: {err}");
    }
}
